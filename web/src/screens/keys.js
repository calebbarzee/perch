import { h, announce, downloadText, copyText, field } from '../dom.js';
import * as fmt from '../format.js';
import { notice, hex, authorChip, routes } from '../components.js';
import { fingerprint } from '../fingerprint.js';
import { settings, saveSettings } from '../settings.js';
import { persistKey, setKeepKey } from '../app.js';
import * as persist from '../persist.js';

export function backupConfirmed(client) {
  const a = client.author();
  return Boolean(a) && settings().backupConfirmedFor === a;
}

export default {
  id: 'keys',
  live: (ctx) => JSON.stringify([ctx.client.author(), ctx.client.peers().map((p) => p.state)]),
  render(ctx) {
    const { client, app } = ctx;
    const author = client.author();
    const section = h('section', { class: 'screen screen-keys' }, h('h1', { text: 'Your key' }));
    section.append(
      h('p', { class: 'lede', text: 'Your key signs what you publish. It stays on this device; nodes never receive it. Anyone with your key can publish as you.' }),
    );

    if (!author) {
      section.append(createKeyPanel(ctx));
      section.append(importPanel(ctx));
      return section;
    }

    const confirmed = backupConfirmed(client);
    const view = client.authorView(author);
    section.append(
      h('div', { class: 'panel key-panel' },
        h('div', { class: 'key-id' },
          fingerprint(author, 40),
          h('div', {},
            h('p', { class: 'label', text: 'Public key (your author ID)' }),
            h('code', { class: 'hex hex-full', id: 'my-author', text: author }),
            h('p', {}, h('button', { type: 'button', class: 'button small', on: { click: () => copyText(author) } }, 'Copy public key'), ' ', h('a', { href: routes.author(author), text: 'Your author page' })),
          ),
        ),
        h('p', { class: 'muted', text: view ? `${fmt.plural(view.record_count, 'record')} published.` : 'Nothing published yet.' }),
      ),
    );

    // Storage choice.
    const keep = h('input', { type: 'checkbox', id: 'keep-key', checked: app.keepKey });
    keep.addEventListener('change', async () => {
      await setKeepKey(keep.checked);
      announce(keep.checked ? 'Key kept on this device.' : 'Key removed from device storage. It is lost when you close this page unless you have a backup.');
      ctx.refresh();
    });
    section.append(
      h('div', { class: 'panel' },
        h('h2', { text: 'Storage' }),
        h('div', { class: 'check' }, keep, h('label', { for: 'keep-key', text: 'Keep my key on this device' })),
        h('p', { class: 'hint', text: app.keepKey
          ? 'The key is stored unencrypted in this browser’s IndexedDB. Other people who use this browser profile can use it.'
          : 'The key is held in memory only. Reloading the page loses it unless you import your backup.' }),
      ),
    );

    // Backup.
    const confirm = h('input', { type: 'checkbox', id: 'backup-ok', checked: confirmed });
    confirm.addEventListener('change', () => {
      saveSettings({ backupConfirmedFor: confirm.checked ? author : null });
      announce(confirm.checked ? 'Backup confirmed. You can publish.' : 'Backup not confirmed.');
      ctx.refresh();
    });
    const backupPanel = h('div', { class: 'panel', id: 'backup' },
      h('h2', { text: 'Backup' }),
      h('p', { text: 'The backup holds your secret key and the position of your latest record. Import it on another device to continue the same log.' }),
      h('p', { class: 'actions' },
        h('button', {
          type: 'button',
          class: 'button primary',
          id: 'download-backup',
          on: {
            click: () => {
              downloadText(`perch-key-${author.slice(0, 8)}.json`, client.exportKey() + '\n');
              announce('Backup downloaded.');
            },
          },
        }, 'Download backup'),
        ' ',
        h('button', { type: 'button', class: 'button', on: { click: () => copyText(client.exportKey()) } }, 'Copy backup'),
      ),
      h('div', { class: 'check' }, confirm, h('label', { for: 'backup-ok', text: 'I saved my backup somewhere safe' })),
      confirmed ? null : h('p', { class: 'hint', text: 'Publishing stays off until you confirm a backup.' }),
    );
    section.append(backupPanel);

    // Publish readiness from the client guard.
    const blocker = publishBlocker(client);
    if (blocker) section.append(notice('warn', 'Publishing is paused', h('p', { text: blocker })));

    // Danger zone.
    section.append(
      h('div', { class: 'panel' },
        h('h2', { text: 'Switch key' }),
        h('p', { text: 'Removing the key from this page does not delete anything you published.' }),
        h('button', {
          type: 'button',
          class: 'button danger',
          on: {
            click: async () => {
              if (!confirm.checked && !window.confirm('You have not confirmed a backup. Without it this key is lost. Remove it anyway?')) return;
              await persist.forgetKey();
              saveSettings({ backupConfirmedFor: null });
              location.reload();
            },
          },
        }, 'Remove key from this device'),
      ),
    );
    return section;
  },
};

export function publishBlocker(client) {
  if (typeof client.publishStatus === 'function') {
    const s = client.publishStatus();
    return s && !s.ready ? s.reason : null;
  }
  return null;
}

function createKeyPanel(ctx) {
  const { client, app } = ctx;
  const keep = h('input', { type: 'checkbox', id: 'new-keep-key', checked: true });
  const panel = h('div', { class: 'panel' },
    h('h2', { text: 'Create a key' }),
    h('p', { text: 'You need a key to publish reviews, listings, or flags. Reading needs no key.' }),
    h('div', { class: 'check' }, keep, h('label', { for: 'new-keep-key', text: 'Keep my key on this device' })),
    h('p', { class: 'hint', text: 'Stored in this browser’s IndexedDB, unencrypted. Turn this off on a shared computer.' }),
    h('button', {
      type: 'button',
      class: 'button primary',
      id: 'generate-key',
      on: {
        click: async () => {
          client.generateKey();
          await setKeepKey(keep.checked);
          await persistKey();
          announce('Key created. Download your backup next.');
          ctx.refresh();
          requestAnimationFrame(() => document.getElementById('download-backup')?.focus());
        },
      },
    }, 'Create key'),
  );
  void app;
  return panel;
}

function importPanel(ctx) {
  const { client } = ctx;
  const input = h('textarea', { id: 'import-key', rows: 3, spellcheck: 'false', autocomplete: 'off', class: 'mono' });
  const file = h('input', { type: 'file', id: 'import-file', accept: '.json,application/json,text/plain' });
  const keep = h('input', { type: 'checkbox', id: 'import-keep-key', checked: true });
  const status = h('div', { class: 'form-status', role: 'status' });
  file.addEventListener('change', async () => {
    const f = file.files?.[0];
    if (f) input.value = (await f.text()).trim();
  });
  const form = h('form', { class: 'panel', id: 'import-form' },
    h('h2', { text: 'Import a key' }),
    field({ id: 'import-key', label: 'Backup or secret key', hint: 'Paste the backup JSON, or the 64-character secret key.', control: input }),
    field({ id: 'import-file', label: 'Or choose a backup file', control: file }),
    h('div', { class: 'check' }, keep, h('label', { for: 'import-keep-key', text: 'Keep my key on this device' })),
    h('button', { type: 'submit', class: 'button primary', text: 'Import key' }),
    status,
  );
  form.addEventListener('submit', async (ev) => {
    ev.preventDefault();
    try {
      const raw = input.value.trim();
      const author = client.importKey(raw);
      const merged = persist.mergeBackupHead(raw.startsWith('{') ? raw : JSON.stringify({ secret: raw, head: null }), author);
      if (merged !== raw) client.importKey(merged);
      await setKeepKey(keep.checked);
      await persistKey();
      // An imported backup counts as backed up.
      if (raw.startsWith('{')) saveSettings({ backupConfirmedFor: author });
      announce('Key imported. Syncing your log from the nodes before publishing.');
      ctx.refresh();
    } catch (e) {
      status.replaceChildren(notice('bad', 'Import failed', h('p', { text: e.message || String(e) })));
    }
  });
  void hex;
  void authorChip;
  return form;
}
