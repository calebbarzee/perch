//! URL and hostname rules (ARCHITECTURE 4.5, `URL`; 7.4 domain matching).
//!
//! A Perch `URL` is a string of at most 2048 Unicode scalar values that the
//! [WHATWG URL parser](https://url.spec.whatwg.org/) accepts with scheme
//! `https`, no username or password, and a domain host (not an IP address).
//! The parser applies IDNA ToASCII
//! ([UTS #46](https://www.unicode.org/reports/tr46/)) to the host, so
//! [`https_url_host`] returns the host in ASCII (punycode) form.
//!
//! In addition, this module rejects strings that contain ASCII control
//! characters, spaces, or backslashes. The WHATWG parser silently strips or
//! rewrites those, so the displayed string and the parsed URL could differ.

use serde::Serialize;

/// Maximum length of a URL field, in Unicode scalar values.
pub const MAX_URL_CHARS: usize = 2048;
/// Maximum length of a hostname, in bytes (ASCII).
pub const MAX_HOSTNAME_LEN: usize = 253;

/// Why a string is not a valid Perch `URL` or hostname.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UrlError {
    /// Longer than [`MAX_URL_CHARS`].
    #[error("URL is longer than 2048 characters")]
    TooLong,
    /// Contains a space, ASCII control character, or backslash.
    #[error("URL contains a space, control character, or backslash")]
    BadCharacter,
    /// The WHATWG parser rejects it.
    #[error("URL does not parse")]
    Unparsable,
    /// The scheme is not `https`.
    #[error("URL scheme is not https")]
    NotHttps,
    /// The URL has a username or password.
    #[error("URL contains a username or password")]
    Credentials,
    /// The host is an IP address or missing.
    #[error("URL host is not a domain name")]
    NotDomainHost,
    /// A hostname is not lowercase ASCII labels of `a-z`, `0-9`, `-`.
    #[error("not a lowercase ASCII hostname")]
    BadHostname,
}

/// A parsed, valid Perch `URL`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpsUrl {
    url: url::Url,
}

impl HttpsUrl {
    /// Parses and checks `s`. See the module documentation for the rules.
    pub fn parse(s: &str) -> Result<HttpsUrl, UrlError> {
        if s.chars().count() > MAX_URL_CHARS {
            return Err(UrlError::TooLong);
        }
        if s.bytes().any(|b| b <= 0x20 || b == 0x7f || b == b'\\') {
            return Err(UrlError::BadCharacter);
        }
        let url = url::Url::parse(s).map_err(|_| UrlError::Unparsable)?;
        if url.scheme() != "https" {
            return Err(UrlError::NotHttps);
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err(UrlError::Credentials);
        }
        match url.host() {
            Some(url::Host::Domain(_)) => {}
            _ => return Err(UrlError::NotDomainHost),
        }
        Ok(HttpsUrl { url })
    }

    /// The host in ASCII form, after IDNA ToASCII and lowercasing.
    pub fn host(&self) -> &str {
        self.url.host_str().unwrap_or_default()
    }

    /// The serialized URL as the WHATWG parser normalizes it. Records keep
    /// the original string; use this only for comparison or navigation.
    pub fn normalized(&self) -> &str {
        self.url.as_str()
    }
}

/// Checks that `s` is a valid Perch `URL`.
pub fn validate_https_url(s: &str) -> Result<(), UrlError> {
    HttpsUrl::parse(s).map(|_| ())
}

/// Returns the ASCII host of a valid Perch `URL`, for domain matching
/// (ARCHITECTURE 7.4). Example: `https://Bücher.example/x` → `xn--bcher-kva.example`.
pub fn https_url_host(s: &str) -> Result<String, UrlError> {
    HttpsUrl::parse(s).map(|u| u.host().to_owned())
}

/// Checks an `attest.domain` `domain` value: a lowercase ASCII hostname of
/// at most 253 bytes, made of at least two dot-separated labels; each label
/// 1–63 characters of `a-z`, `0-9`, `-`, not starting or ending with `-`;
/// the last label not all digits (so it cannot be an IPv4 address).
/// Punycode labels (`xn--…`) are allowed; non-ASCII is not.
pub fn validate_hostname(domain: &str) -> Result<(), UrlError> {
    if domain.is_empty() || domain.len() > MAX_HOSTNAME_LEN {
        return Err(UrlError::BadHostname);
    }
    let labels: Vec<&str> = domain.split('.').collect();
    if labels.len() < 2 {
        return Err(UrlError::BadHostname);
    }
    for l in &labels {
        let b = l.as_bytes();
        if b.is_empty()
            || b.len() > 63
            || b[0] == b'-'
            || b[b.len() - 1] == b'-'
            || !b.iter().all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        {
            return Err(UrlError::BadHostname);
        }
    }
    if labels[labels.len() - 1].bytes().all(|c| c.is_ascii_digit()) {
        return Err(UrlError::BadHostname);
    }
    Ok(())
}

/// True if `host` equals `domain` or is a subdomain of it (ARCHITECTURE 7.4).
/// Both must already be ASCII lowercase, as returned by [`https_url_host`]
/// and accepted by [`validate_hostname`].
pub fn host_matches_domain(host: &str, domain: &str) -> bool {
    if host == domain {
        return true;
    }
    host.len() > domain.len()
        && host.ends_with(domain)
        && host.as_bytes()[host.len() - domain.len() - 1] == b'.'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_https_domain_urls() {
        assert!(validate_https_url("https://example.com").is_ok());
        assert!(validate_https_url("https://shop.example.com/p/1?x=2#f").is_ok());
        assert!(validate_https_url("HTTPS://EXAMPLE.COM/").is_ok());
        assert_eq!(https_url_host("https://EXAMPLE.com/").unwrap(), "example.com");
        assert_eq!(https_url_host("https://bücher.example/x").unwrap(), "xn--bcher-kva.example");
        assert_eq!(https_url_host("https://xn--bcher-kva.example/").unwrap(), "xn--bcher-kva.example");
        assert_eq!(https_url_host("https://example.com:8443/").unwrap(), "example.com");
    }

    #[test]
    fn rejects_bad_urls() {
        assert_eq!(validate_https_url("http://example.com/"), Err(UrlError::NotHttps));
        assert_eq!(validate_https_url("javascript:alert(1)"), Err(UrlError::NotHttps));
        assert_eq!(validate_https_url("ftp://example.com/"), Err(UrlError::NotHttps));
        assert_eq!(validate_https_url("https://user@example.com/"), Err(UrlError::Credentials));
        assert_eq!(validate_https_url("https://user:pw@example.com/"), Err(UrlError::Credentials));
        assert_eq!(validate_https_url("https://:pw@example.com/"), Err(UrlError::Credentials));
        assert_eq!(validate_https_url("https://127.0.0.1/"), Err(UrlError::NotDomainHost));
        assert_eq!(validate_https_url("https://0x7f.1/"), Err(UrlError::NotDomainHost));
        assert_eq!(validate_https_url("https://[::1]/"), Err(UrlError::NotDomainHost));
        assert_eq!(validate_https_url("https://"), Err(UrlError::Unparsable));
        assert_eq!(validate_https_url("example.com"), Err(UrlError::Unparsable));
        assert_eq!(validate_https_url(" https://example.com"), Err(UrlError::BadCharacter));
        assert_eq!(validate_https_url("https://exa\tmple.com"), Err(UrlError::BadCharacter));
        assert_eq!(validate_https_url("https:\\\\example.com"), Err(UrlError::BadCharacter));
        assert_eq!(validate_https_url("https://example.com/a b"), Err(UrlError::BadCharacter));
        let long = format!("https://example.com/{}", "a".repeat(2048));
        assert_eq!(validate_https_url(&long), Err(UrlError::TooLong));
        let exact = format!("https://example.com/{}", "a".repeat(2048 - 20));
        assert!(validate_https_url(&exact).is_ok());
    }

    #[test]
    fn hostnames() {
        for ok in ["example.com", "a.b.c.example", "xn--bcher-kva.example", "a-b.co", "1.example.com"] {
            assert!(validate_hostname(ok).is_ok(), "{ok}");
        }
        for bad in [
            "", "com", "Example.com", "example.com.", ".example.com", "a..b", "-a.com", "a-.com", "a_b.com",
            "bücher.example", "1.2.3.4", "a b.com",
        ] {
            assert!(validate_hostname(bad).is_err(), "{bad}");
        }
        let label = "a".repeat(63);
        assert!(validate_hostname(&format!("{label}.com")).is_ok());
        assert!(validate_hostname(&format!("{label}a.com")).is_err());
        let long = format!("{}.com", vec!["a".repeat(60); 5].join("."));
        assert!(long.len() > 253);
        assert!(validate_hostname(&long).is_err());
    }

    #[test]
    fn domain_matching() {
        assert!(host_matches_domain("example.com", "example.com"));
        assert!(host_matches_domain("shop.example.com", "example.com"));
        assert!(!host_matches_domain("badexample.com", "example.com"));
        assert!(!host_matches_domain("example.com.evil.net", "example.com"));
        assert!(!host_matches_domain("com", "example.com"));
    }
}
