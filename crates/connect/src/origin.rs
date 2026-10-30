//! A site is an origin, `https://host[:port]` (A1): a DNS host name, shown
//! in ASCII so a look-alike (IDN) host is visible as `xn--…`. IP literals
//! are refused; plain `http` and loopback only with "Allow local sites".

use std::fmt;

use serde::{Deserialize, Serialize};
use url::{Host, Url};

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Origin(String);

impl Origin {
    /// Parses what the user typed (or a stored origin). A path, query,
    /// fragment or user name is refused, not dropped.
    pub fn parse(input: &str, allow_local: bool) -> Result<Self, String> {
        let input = input.trim();
        let url = Url::parse(input).map_err(|_| format!("“{input}” isn't a web address."))?;
        if !url.username().is_empty() || url.password().is_some() {
            return Err("A site address can't carry a user name.".into());
        }
        if !matches!(url.path(), "" | "/") || url.query().is_some() || url.fragment().is_some() {
            return Err("Give the site's address only, like https://example.org.".into());
        }
        let local = match url.host() {
            // `a.example.` is another origin than `a.example`, and
            // `localhost.` or `x.localhost` is loopback all the same.
            Some(Host::Domain(d)) if d.ends_with('.') => {
                return Err(format!("“{input}” ends its host with a dot."));
            }
            Some(Host::Domain(d)) => is_localhost(d),
            Some(Host::Ipv4(ip)) if ip.is_loopback() => true,
            Some(Host::Ipv6(ip)) if ip.is_loopback() => true,
            Some(_) => return Err("A site needs a host name, not an IP address.".into()),
            None => return Err(format!("“{input}” has no host.")),
        };
        if local && !allow_local {
            return Err("Local sites need the developer switch.".into());
        }
        match url.scheme() {
            "https" => {}
            "http" if local => {}
            _ => return Err("A site must use https.".into()),
        }
        Ok(Self(url.origin().ascii_serialization()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The host in ASCII, with its port if any.
    pub fn host(&self) -> &str {
        self.0.split_once("://").map_or(&self.0, |(_, h)| h)
    }

    /// Whether `url` is on this origin (scheme, host and port).
    pub fn owns(&self, url: &str) -> bool {
        Url::parse(url).is_ok_and(|u| u.origin().ascii_serialization() == self.0)
    }
}

/// `localhost` and every name under it (RFC 6761), any case (the parser
/// lower-cases a domain already).
fn is_localhost(domain: &str) -> bool {
    let d = domain.to_ascii_lowercase();
    d == "localhost" || d.ends_with(".localhost")
}

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_origin_is_https_on_a_dns_host_in_ascii() {
        let o = Origin::parse(" https://Example.ORG/ ", false).unwrap();
        assert_eq!(
            (o.as_str(), o.host()),
            ("https://example.org", "example.org")
        );
        let port = Origin::parse("https://example.org:8443", false).unwrap();
        assert_eq!(port.host(), "example.org:8443");
        // A look-alike host is shown in punycode.
        let idn = Origin::parse("https://pаypal.com", false).unwrap();
        assert!(idn.host().starts_with("xn--"), "{idn}");
        assert!(o.owns("https://example.org/api/foqa/v1"));
        assert!(!o.owns("https://evil.example/api") && !o.owns("http://example.org/api"));
        assert!(!o.owns("https://example.org:444/api"));
    }

    #[test]
    fn ip_literals_paths_plain_http_and_local_hosts_are_refused() {
        for bad in [
            "http://example.org",
            "https://93.184.216.34",
            "https://[2001:db8::1]",
            "https://example.org/path",
            "https://example.org/?q=1",
            "https://example.org/#x",
            "https://user@example.org",
            "ftp://example.org",
            "example.org",
            "https://localhost",
            "http://127.0.0.1:8080",
        ] {
            assert!(Origin::parse(bad, false).is_err(), "{bad}");
        }
        // Loopback, over http too, only with the developer setting.
        assert!(Origin::parse("http://127.0.0.1:8080", true).is_ok());
        assert!(Origin::parse("http://localhost:3000", true).is_ok());
        assert!(Origin::parse("http://example.org", true).is_err());
        assert!(Origin::parse("https://10.0.0.1", true).is_err());
    }

    #[test]
    fn trailing_dots_and_names_under_localhost_are_not_public() {
        for bad in [
            "https://localhost.",
            "https://LOCALHOST.",
            "https://foo.localhost",
            "https://FOO.LocalHost:8443",
            "https://a.example.",
        ] {
            assert!(Origin::parse(bad, false).is_err(), "{bad}");
        }
        assert!(Origin::parse("https://foo.localhost", true).is_ok());
        assert!(Origin::parse("https://localhost.", true).is_err());
        assert!(Origin::parse("https://a.example.", true).is_err());
        assert!(Origin::parse("https://localhost.example", false).is_ok());
    }
}
