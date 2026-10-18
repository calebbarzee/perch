//! Key backup format (ARCHITECTURE 8.2): `{ "secret": hex, "head": { "seq", "id" } | null }`.
//! `importKey` also accepts a bare 64-hex secret.

use crate::guard::HeadRef;
use serde_json::{Value, json};

#[derive(Debug, PartialEq, Eq)]
pub struct Backup {
    pub secret: [u8; 32],
    pub head: Option<HeadRef>,
}

fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn decode32(s: &str) -> [u8; 32] {
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).expect("checked hex");
    }
    out
}

pub fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 15) as usize] as char);
    }
    s
}

pub fn parse(input: &str) -> Result<Backup, String> {
    let text = input.trim();
    if !text.starts_with('{') {
        if !is_hex64(text) {
            return Err("A secret key is 64 lowercase hex characters.".into());
        }
        return Ok(Backup { secret: decode32(text), head: None });
    }
    let v: Value = serde_json::from_str(text).map_err(|_| "The key backup is not valid JSON.".to_string())?;
    let secret = v.get("secret").and_then(Value::as_str).ok_or("The key backup has no \"secret\".")?;
    if !is_hex64(secret) {
        return Err("The secret in the key backup must be 64 lowercase hex characters.".into());
    }
    let head = match v.get("head") {
        None | Some(Value::Null) => None,
        Some(h) => {
            let seq = h.get("seq").and_then(Value::as_u64).filter(|s| *s >= 1);
            let id = h.get("id").and_then(Value::as_str).filter(|s| is_hex64(s));
            match (seq, id) {
                (Some(seq), Some(id)) => Some(HeadRef { seq, id: id.to_string() }),
                _ => return Err("The key backup has an invalid head.".into()),
            }
        }
    };
    Ok(Backup { secret: decode32(secret), head })
}

pub fn format(secret: &[u8; 32], head: Option<&HeadRef>) -> String {
    let head = head.map(|h| json!({ "seq": h.seq, "id": h.id })).unwrap_or(Value::Null);
    json!({ "secret": encode_hex(secret), "head": head }).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: &str = "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60";

    #[test]
    fn bare_secret() {
        let b = parse(S).unwrap();
        assert_eq!(encode_hex(&b.secret), S);
        assert_eq!(b.head, None);
    }

    #[test]
    fn round_trip() {
        let head = HeadRef { seq: 7, id: "a".repeat(64) };
        let text = format(&parse(S).unwrap().secret, Some(&head));
        let b = parse(&text).unwrap();
        assert_eq!(b.head, Some(head));
    }

    #[test]
    fn null_head() {
        let b = parse(&format!("{{\"secret\":\"{S}\",\"head\":null}}")).unwrap();
        assert_eq!(b.head, None);
    }

    #[test]
    fn rejects_bad_input() {
        assert!(parse("abc").is_err());
        assert!(parse(&S.to_uppercase()).is_err());
        assert!(parse("{\"secret\":1}").is_err());
        assert!(parse(&format!("{{\"secret\":\"{S}\",\"head\":{{\"seq\":0,\"id\":\"{}\"}}}}", "a".repeat(64))).is_err());
        assert!(parse("{").is_err());
    }
}
