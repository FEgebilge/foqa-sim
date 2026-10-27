//! Deep links from websites (A3): `foqa-sim://challenge?site=…&id=…[&v=…]`
//! and `foqa-sim://connect?site=…`, parsed as untrusted input, and the
//! gate that keeps a link from acting without the user: an unknown site
//! waits for "Connect", a known one only opens the briefing. The pending
//! link lives in memory for ten minutes, never on disk.

use url::Url;

use crate::origin::Origin;

pub const SCHEME: &str = "foqa-sim";
/// Longest link accepted, in bytes.
pub const MAX_LEN: usize = 512;
/// A link not acted on in this long is dropped.
pub const PENDING_MS: i64 = 10 * 60 * 1000;
const MAX_VERSION: u32 = 100_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    Challenge {
        site: Origin,
        id: String,
        version: Option<u32>,
    },
    Connect {
        site: Origin,
    },
}

impl Link {
    pub fn site(&self) -> &Origin {
        match self {
            Self::Challenge { site, .. } | Self::Connect { site } => site,
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Self::Challenge { .. } => "challenge",
            Self::Connect { .. } => "connect",
        }
    }
}

/// Why a link was refused: a code for the connect log and its words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Refused(pub &'static str);

impl Refused {
    pub fn text(self) -> &'static str {
        match self.0 {
            "too_long" => "The link is too long.",
            "characters" => "The link has characters a FOQA-Sim link never has.",
            "scheme" => "This isn't a FOQA-Sim link.",
            "kind" => "FOQA-Sim doesn't know this kind of link.",
            "duplicate_key" => "The link repeats a field.",
            "site" => "The link's site address isn't one FOQA-Sim can connect to.",
            "id" => "The link's challenge id isn't valid.",
            "version" => "The link's challenge version isn't valid.",
            _ => "The link isn't valid.",
        }
    }
}

/// Parses a link strictly (A3). `allow_local` is "Allow local sites": only
/// then may `site` be loopback or plain http; other IP literals never.
pub fn parse(input: &str, allow_local: bool) -> Result<Link, Refused> {
    if input.len() > MAX_LEN {
        return Err(Refused("too_long"));
    }
    // The URL parser drops tabs, newlines and edge spaces; nothing of the
    // kind may reach it.
    if !input.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(Refused("characters"));
    }
    let url = Url::parse(input).map_err(|_| Refused("scheme"))?;
    if url.scheme() != SCHEME {
        return Err(Refused("scheme"));
    }
    let bare = url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
        && url.fragment().is_none()
        && matches!(url.path(), "" | "/");
    if !bare {
        return Err(Refused("kind"));
    }
    let mut fields: Vec<(String, String)> = Vec::new();
    for (k, v) in url.query_pairs() {
        if fields.iter().any(|(seen, _)| *seen == k) {
            return Err(Refused("duplicate_key"));
        }
        fields.push((k.into_owned(), v.into_owned()));
    }
    let field = |name: &str| {
        fields
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    };
    let site = field("site").ok_or(Refused("site"))?;
    let site = Origin::parse(site, allow_local).map_err(|_| Refused("site"))?;
    match url.host_str() {
        Some("challenge") => {
            let id = field("id").filter(|id| valid_id(id)).ok_or(Refused("id"))?;
            let version = field("v").map(version).transpose()?;
            Ok(Link::Challenge {
                site,
                id: id.to_owned(),
                version,
            })
        }
        Some("connect") => Ok(Link::Connect { site }),
        _ => Err(Refused("kind")),
    }
}

/// `^[a-z0-9][a-z0-9-]{2,63}$`
fn valid_id(id: &str) -> bool {
    let b = id.as_bytes();
    let ok = |c: &u8| c.is_ascii_lowercase() || c.is_ascii_digit();
    (3..=64).contains(&b.len()) && ok(&b[0]) && b[1..].iter().all(|c| ok(c) || *c == b'-')
}

fn version(v: &str) -> Result<u32, Refused> {
    let digits = !v.is_empty() && v.len() <= 6 && v.bytes().all(|b| b.is_ascii_digit());
    let n = digits.then(|| v.parse().ok()).flatten();
    n.filter(|n| (1..=MAX_VERSION).contains(n))
        .ok_or(Refused("version"))
}

/// What a pending link waits for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Consent {
    /// The site isn't one of the user's: nothing is fetched from it until
    /// the user allows connecting.
    Ask,
    /// A site the user has: the challenge's briefing may open.
    Open,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    pub link: Link,
    pub consent: Consent,
    pub at_ms: i64,
}

/// The one link waiting for the user; a newer link replaces it.
#[derive(Debug, Default)]
pub struct Gate {
    pending: Option<Pending>,
}

impl Gate {
    /// A link arrived; `known` when its site is one of the user's.
    pub fn offer(&mut self, link: Link, known: bool, now_ms: i64) -> Pending {
        let consent = if known { Consent::Open } else { Consent::Ask };
        let pending = Pending {
            link,
            consent,
            at_ms: now_ms,
        };
        self.pending = Some(pending.clone());
        pending
    }

    /// The link waiting, unless it is older than `PENDING_MS`.
    pub fn pending(&mut self, now_ms: i64) -> Option<Pending> {
        if self
            .pending
            .as_ref()
            .is_some_and(|p| now_ms - p.at_ms > PENDING_MS)
        {
            self.pending = None;
        }
        self.pending.clone()
    }

    /// The user's answer to "Connect to <origin>?". Only the link waiting
    /// for it, for that very origin, is answered: allowed, it may open
    /// (and is returned); denied, it is dropped (`None`).
    pub fn consent(
        &mut self,
        origin: &str,
        allow: bool,
        now_ms: i64,
    ) -> Result<Option<Link>, String> {
        let asked = self
            .pending(now_ms)
            .filter(|p| p.consent == Consent::Ask && p.link.site().as_str() == origin);
        let Some(mut p) = asked else {
            return Err("That link is no longer waiting; open it from the website again.".into());
        };
        if !allow {
            self.pending = None;
            return Ok(None);
        }
        p.consent = Consent::Open;
        let link = p.link.clone();
        self.pending = Some(p);
        Ok(Some(link))
    }

    /// The link was acted on or dismissed.
    pub fn done(&mut self) {
        self.pending = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SITE: &str = "https%3A%2F%2Fpilotaptitude.example";

    fn ok(url: &str) -> Link {
        parse(url, false).unwrap_or_else(|e| panic!("{url}: {e:?}"))
    }

    fn refused(url: &str) -> &'static str {
        match parse(url, false) {
            Err(Refused(code)) => code,
            Ok(link) => panic!("{url} accepted: {link:?}"),
        }
    }

    #[test]
    fn challenge_and_connect_links_parse() {
        let link = ok(&format!(
            "foqa-sim://challenge?site={SITE}&id=ltfj-06-gusty-2026w44&v=3"
        ));
        let Link::Challenge { site, id, version } = &link else {
            panic!("{link:?}")
        };
        assert_eq!(site.as_str(), "https://pilotaptitude.example");
        assert_eq!((id.as_str(), *version), ("ltfj-06-gusty-2026w44", Some(3)));
        assert_eq!(link.kind(), "challenge");
        // No version, a trailing slash, an unknown key and an unencoded site.
        let plain = ok("foqa-sim://challenge/?id=abc&site=https://pilotaptitude.example&utm=x");
        assert!(matches!(plain, Link::Challenge { version: None, .. }));
        let connect = ok(&format!("FOQA-SIM://connect?site={SITE}"));
        assert_eq!(connect.site().host(), "pilotaptitude.example");
        // A look-alike host is shown in ASCII.
        let idn = ok("foqa-sim://connect?site=https%3A%2F%2Fp%D0%B0ypal.com");
        assert!(idn.site().host().starts_with("xn--"), "{idn:?}");
    }

    #[test]
    fn hostile_links_are_refused() {
        let long = format!(
            "foqa-sim://challenge?site={SITE}&id=abc&pad={}",
            "a".repeat(MAX_LEN)
        );
        assert_eq!(refused(&long), "too_long");
        for (url, code) in [
            (
                "foqa-sim://challenge?site=https://a.example&id=abc\n",
                "characters",
            ),
            (
                "foqa-sim://chal\tlenge?site=https://a.example&id=abc",
                "characters",
            ),
            (
                "foqa-sim://challenge?site=https://a.example&id=ab c",
                "characters",
            ),
            (
                "foqa-sim://challenge?site=https://a.example&id=abç",
                "characters",
            ),
            ("https://challenge?site=https://a.example&id=abc", "scheme"),
            (
                "foqa-simx://challenge?site=https://a.example&id=abc",
                "scheme",
            ),
            ("not a url", "characters"),
            ("foqa-sim:challenge?site=https://a.example&id=abc", "kind"),
            ("foqa-sim://place?site=https://a.example&id=abc", "kind"),
            ("foqa-sim://Challenge?site=https://a.example&id=abc", "kind"),
            (
                "foqa-sim://user@challenge?site=https://a.example&id=abc",
                "kind",
            ),
            (
                "foqa-sim://challenge:80?site=https://a.example&id=abc",
                "kind",
            ),
            (
                "foqa-sim://challenge/x?site=https://a.example&id=abc",
                "kind",
            ),
            (
                "foqa-sim://challenge?site=https://a.example&id=abc#f",
                "kind",
            ),
            (
                "foqa-sim://challenge?site=https://a.example&id=abc&id=abd",
                "duplicate_key",
            ),
            (
                "foqa-sim://challenge?site=https://a.example&site=https://b.example&id=abc",
                "duplicate_key",
            ),
            ("foqa-sim://challenge?id=abc", "site"),
            ("foqa-sim://challenge?site=http://a.example&id=abc", "site"),
            (
                "foqa-sim://challenge?site=https://93.184.216.34&id=abc",
                "site",
            ),
            (
                "foqa-sim://challenge?site=https://[2001:db8::1]&id=abc",
                "site",
            ),
            (
                "foqa-sim://challenge?site=https://a.example/path&id=abc",
                "site",
            ),
            (
                "foqa-sim://challenge?site=https://a.example%3Fq%3D1&id=abc",
                "site",
            ),
            ("foqa-sim://challenge?site=https://localhost&id=abc", "site"),
            (
                "foqa-sim://challenge?site=http://127.0.0.1:8080&id=abc",
                "site",
            ),
            ("foqa-sim://challenge?site=https://a.example", "id"),
            ("foqa-sim://challenge?site=https://a.example&id=ab", "id"),
            ("foqa-sim://challenge?site=https://a.example&id=-abc", "id"),
            ("foqa-sim://challenge?site=https://a.example&id=ABC", "id"),
            (
                "foqa-sim://challenge?site=https://a.example&id=a%2F..%2Fb",
                "id",
            ),
            (
                "foqa-sim://challenge?site=https://a.example&id=abc&v=0",
                "version",
            ),
            (
                "foqa-sim://challenge?site=https://a.example&id=abc&v=100001",
                "version",
            ),
            (
                "foqa-sim://challenge?site=https://a.example&id=abc&v=-1",
                "version",
            ),
            (
                "foqa-sim://challenge?site=https://a.example&id=abc&v=1.5",
                "version",
            ),
            (
                "foqa-sim://challenge?site=https://a.example&id=abc&v=",
                "version",
            ),
            (
                "foqa-sim://challenge?site=https://a.example&id=abc&v=99999999999",
                "version",
            ),
        ] {
            assert_eq!(refused(url), code, "{url}");
            assert!(!Refused(code).text().is_empty());
        }
        let id64 = format!("a{}", "-".repeat(63));
        assert!(
            parse(
                &format!("foqa-sim://challenge?site={SITE}&id={id64}"),
                false
            )
            .is_ok()
        );
        let id65 = format!("a{}", "b".repeat(64));
        assert_eq!(
            refused(&format!("foqa-sim://challenge?site={SITE}&id={id65}")),
            "id"
        );
    }

    #[test]
    fn loopback_sites_only_with_allow_local() {
        let url = "foqa-sim://connect?site=http%3A%2F%2F127.0.0.1%3A8080";
        assert!(parse(url, false).is_err());
        assert_eq!(
            parse(url, true).unwrap().site().as_str(),
            "http://127.0.0.1:8080"
        );
        let lan = "foqa-sim://connect?site=https%3A%2F%2F10.0.0.1";
        assert_eq!(parse(lan, true), Err(Refused("site")));
    }

    fn link(site: &str) -> Link {
        Link::Challenge {
            site: Origin::parse(site, false).unwrap(),
            id: "abc".into(),
            version: None,
        }
    }

    #[test]
    fn an_unknown_site_waits_for_consent_and_a_known_one_only_opens() {
        let mut gate = Gate::default();
        let known = gate.offer(link("https://a.example"), true, 0);
        assert_eq!(known.consent, Consent::Open);
        assert!(
            gate.consent("https://a.example", true, 1).is_err(),
            "a known site needs no consent"
        );

        let asked = gate.offer(link("https://b.example"), false, 10);
        assert_eq!(asked.consent, Consent::Ask);
        assert!(
            gate.consent("https://a.example", true, 11).is_err(),
            "only the link waiting is answered"
        );
        let allowed = gate
            .consent("https://b.example", true, 12)
            .unwrap()
            .unwrap();
        assert_eq!(allowed.site().as_str(), "https://b.example");
        assert_eq!(
            gate.pending(13).unwrap().consent,
            Consent::Open,
            "kept to resume"
        );
        gate.done();
        assert!(gate.pending(14).is_none());

        gate.offer(link("https://c.example"), false, 20);
        assert_eq!(gate.consent("https://c.example", false, 21), Ok(None));
        assert!(gate.pending(22).is_none(), "denied is dropped");
    }

    #[test]
    fn a_pending_link_expires_after_ten_minutes() {
        let mut gate = Gate::default();
        gate.offer(link("https://b.example"), false, 0);
        assert!(gate.pending(PENDING_MS).is_some());
        assert!(
            gate.consent("https://b.example", true, PENDING_MS + 1)
                .is_err()
        );
        assert!(gate.pending(PENDING_MS + 1).is_none());
    }
}
