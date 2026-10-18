//! Append-only JSON Lines persistence (native only).
//!
//! ## File format
//!
//! One record per line in canonical wire form (ARCHITECTURE 5.4). The file
//! is the public export format.
//!
//! ## What is written
//!
//! | Event | Lines appended |
//! |---|---|
//! | Record stored | The record as stored |
//! | Body filled | The record with its body |
//! | Evidence pair created or lowered | Both pair records, `a` then `b`, without bodies |
//!
//! Records later truncated by a fork stay in the file. Replay re-derives the
//! truncation because inserts are deterministic.
//!
//! ## Durability (fsync policy)
//!
//! - Each line is written with one `write` call on a file opened in append mode, when the insert happens.
//! - [`MemoryStore::flush`] calls `sync_data`. The node calls it after each
//!   HTTP submission and each sync message that changed the store, before it
//!   reports success to a client.
//! - A crash can lose lines written since the last `flush`; peers resend them.
//! - A crash during a write can leave a partial last line. [`MemoryStore::open`]
//!   truncates the file to the last complete line.
//!
//! ## Compaction
//!
//! [`MemoryStore::compact`] rewrites the file in export order to a temporary
//! file, syncs it, and renames it over the original. It runs after
//! [`crate::Store::remove_body`] and after a replay that dropped bodies on the
//! removal list, so removed bodies do not remain on disk.

use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use perch_core::{Record, RecordId};

use crate::memory::{MemoryStore, Mode};
use crate::{Insert, StoreError};

/// An open JSONL file in append mode.
#[derive(Debug)]
pub struct JsonlFile {
    path: PathBuf,
    file: File,
    dirty: bool,
}

impl JsonlFile {
    /// The file path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Appends one record line.
    pub fn append(&mut self, r: &Record) -> io::Result<()> {
        let mut line = r.to_json_string();
        line.push('\n');
        self.file.write_all(line.as_bytes())?;
        self.dirty = true;
        Ok(())
    }

    /// Flushes written lines to stable storage.
    pub fn sync(&mut self) -> io::Result<()> {
        if self.dirty {
            self.file.sync_data()?;
            self.dirty = false;
        }
        Ok(())
    }

    /// Atomically replaces the file content with `records`.
    pub fn rewrite(&mut self, records: impl Iterator<Item = Record>) -> io::Result<()> {
        let tmp = self.path.with_extension("jsonl.tmp");
        {
            let f = File::create(&tmp)?;
            let mut w = io::BufWriter::new(f);
            for r in records {
                w.write_all(r.to_json_string().as_bytes())?;
                w.write_all(b"\n")?;
            }
            let f = w.into_inner().map_err(|e| e.into_error())?;
            f.sync_all()?;
        }
        std::fs::rename(&tmp, &self.path)?;
        if let Some(dir) = self.path.parent()
            && let Ok(d) = File::open(dir)
        {
            let _ = d.sync_all();
        }
        self.file = OpenOptions::new().append(true).open(&self.path)?;
        self.dirty = false;
        Ok(())
    }
}

/// Result of replaying a file at startup.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReplayReport {
    /// Non-blank lines read.
    pub lines: usize,
    /// Lines that changed the store.
    pub applied: usize,
    /// `(line number, error)` for lines that failed parsing or verification.
    pub invalid: Vec<(usize, String)>,
    /// Bytes of a partial last line that were truncated.
    pub truncated_bytes: u64,
    /// True if the file was rewritten after replay (removed bodies found).
    pub compacted: bool,
}

impl MemoryStore {
    /// Opens (or creates) a JSONL file, replays it with full verification,
    /// and keeps appending to it. IDs in `removed` are added to the removal
    /// list first, so their bodies are dropped during replay.
    pub fn open(
        path: impl AsRef<Path>,
        mode: Mode,
        removed: impl IntoIterator<Item = RecordId>,
    ) -> Result<(MemoryStore, ReplayReport), StoreError> {
        let path = path.as_ref().to_path_buf();
        if let Some(dir) = path.parent()
            && !dir.as_os_str().is_empty()
        {
            std::fs::create_dir_all(dir)?;
        }
        let mut store = MemoryStore::with_mode(mode);
        store.add_removed(removed);
        let mut report = ReplayReport::default();
        let file = OpenOptions::new().read(true).append(true).create(true).open(&path)?;
        let mut reader = BufReader::new(file.try_clone()?);
        let mut buf = String::new();
        let mut good_end: u64 = 0;
        let mut pos: u64 = 0;
        let mut line_no = 0usize;
        let mut prev: Option<Record> = None;
        loop {
            buf.clear();
            let n = reader.read_line(&mut buf)?;
            if n == 0 {
                break;
            }
            pos += n as u64;
            if !buf.ends_with('\n') {
                break; // partial last line
            }
            good_end = pos;
            line_no += 1;
            let line = buf.trim_end_matches(['\n', '\r']);
            if line.trim().is_empty() {
                continue;
            }
            report.lines += 1;
            match Record::from_json_str(line) {
                Ok(r) => {
                    let o = store.insert_with_prev(prev.as_ref(), r.clone());
                    if o.changed() || matches!(o, Insert::Fork { .. }) {
                        report.applied += 1;
                    }
                    prev = Some(r);
                }
                Err(e) => {
                    report.invalid.push((line_no, e.to_string()));
                    prev = None;
                }
            }
        }
        let len = file.metadata()?.len();
        if len > good_end {
            report.truncated_bytes = len - good_end;
            file.set_len(good_end)?;
            let mut f = &file;
            f.seek(SeekFrom::End(0))?;
            file.sync_all()?;
        }
        drop(reader);
        store.jslot.file = Some(JsonlFile { path, file, dirty: false });
        if store.needs_compaction {
            store.compact()?;
            report.compacted = true;
        }
        Ok((store, report))
    }

    /// Syncs appended lines to disk. Returns the first write error since the last call, if any.
    pub fn flush(&mut self) -> io::Result<()> {
        if let Some(e) = self.jslot.error.take() {
            return Err(e);
        }
        match self.jslot.file.as_mut() {
            Some(j) => j.sync(),
            None => Ok(()),
        }
    }

    /// Rewrites the persistence file in export order (see module docs).
    pub fn compact(&mut self) -> io::Result<()> {
        let Some(mut j) = self.jslot.file.take() else {
            self.needs_compaction = false;
            return Ok(());
        };
        let res = j.rewrite(self.export_records());
        self.jslot.file = Some(j);
        if res.is_ok() {
            self.needs_compaction = false;
        }
        res
    }

    /// Path of the persistence file, if any.
    pub fn journal_path(&self) -> Option<&Path> {
        self.jslot.file.as_ref().map(|j| j.path())
    }
}
