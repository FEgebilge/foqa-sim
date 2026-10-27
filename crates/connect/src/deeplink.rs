//! Deep links from websites (A3): `foqa-sim://challenge?site=…&id=…[&v=…]`
//! and `foqa-sim://connect?site=…`, parsed as untrusted input, and the
//! gate that keeps a link from acting without the user: an unknown site
//! waits for "Connect", a known one for "Open"; every answer names the
//! link it was shown for. A pending link lives in memory for ten minutes,
//! never on disk, and is given to the window once.

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
    /// A site the user has: the challenge may open once the user clicks.
    Open,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    /// Names this link in every answer, so a click acts on the link it
    /// was shown for and on no other.
    pub seq: u64,
    pub link: Link,
    pub consent: Consent,
    pub at_ms: i64,
    /// Given to the window already; it is never given twice.
    handed: bool,
}

/// The link the window asks about, and the newest one that arrived while
/// it asked (shown after it). Nothing here acts: every answer names the
/// link's `seq`, which must be the one asked about.
#[derive(Debug, Default)]
pub struct Gate {
    current: Option<Pending>,
    next: Option<Pending>,
    seq: u64,
}

const GONE: &str = "That link is no longer waiting; open it from the website again.";

impl Gate {
    /// A link arrived; `known` when its site is one of the user's. A link
    /// already waiting is not queued again (false).
    pub fn offer(&mut self, link: Link, known: bool, now_ms: i64) -> bool {
        self.expire(now_ms);
        let waiting = [&self.current, &self.next];
        if waiting.into_iter().flatten().any(|p| p.link == link) {
            return false;
        }
        self.seq += 1;
        let pending = Pending {
            seq: self.seq,
            link,
            consent: if known { Consent::Open } else { Consent::Ask },
            at_ms: now_ms,
            handed: false,
        };
        if self.current.is_none() {
            self.current = Some(pending);
        } else {
            self.next = Some(pending);
        }
        true
    }

    fn expire(&mut self, now_ms: i64) {
        let old = |p: &Option<Pending>| p.as_ref().is_some_and(|p| now_ms - p.at_ms > PENDING_MS);
        if old(&self.next) {
            self.next = None;
        }
        if old(&self.current) {
            self.current = self.next.take();
        }
    }

    /// The link for the window to ask about, given once. The window asks
    /// only while it shows no link, so one given before is no longer shown
    /// (the window reloaded): it is dropped, not replayed.
    pub fn hand(&mut self, now_ms: i64) -> Option<Pending> {
        self.expire(now_ms);
        if self.current.as_ref().is_some_and(|p| p.handed) {
            self.current = self.next.take();
        }
        let p = self.current.as_mut()?;
        p.handed = true;
        Some(p.clone())
    }

    /// The link asked about, if it is `seq`.
    fn asked(&mut self, seq: u64, now_ms: i64) -> Result<&mut Pending, String> {
        self.expire(now_ms);
        self.current
            .as_mut()
            .filter(|p| p.seq == seq && p.handed)
            .ok_or_else(|| GONE.into())
    }

    /// The user's answer to "Connect to <origin>?" for link `seq`, which
    /// must be waiting for it: allowed, it may open (and is returned);
    /// denied, it is dropped (`None`).
    pub fn consent(&mut self, seq: u64, allow: bool, now_ms: i64) -> Result<Option<Link>, String> {
        let p = self.asked(seq, now_ms)?;
        if p.consent != Consent::Ask {
            return Err(GONE.into());
        }
        if !allow {
            self.done(seq);
            return Ok(None);
        }
        p.consent = Consent::Open;
        Ok(Some(p.link.clone()))
    }

    /// Link `seq`, for the click that opens it; its site is the user's by
    /// then. It stays until `done`.
    pub fn ready(&mut self, seq: u64, now_ms: i64) -> Result<Link, String> {
        let p = self.asked(seq, now_ms)?;
        if p.consent != Consent::Open {
            return Err(GONE.into());
        }
        Ok(p.link.clone())
    }

    /// Link `seq` was acted on or dismissed; the next one, if any, waits.
    pub fn done(&mut self, seq: u64) {
        if self.current.as_ref().is_some_and(|p| p.seq == seq) {
            self.current = self.next.take();
        }
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
                "foqa-sim://challenge?site=https://localhost.&id=abc",
                "site",
            ),
            (
                "foqa-sim://challenge?site=https://LOCALHOST.&id=abc",
                "site",
            ),
            (
                "foqa-sim://challenge?site=https://foo.localhost&id=abc",
                "site",
            ),
            (
                "foqa-sim://challenge?site=https://a.example.&id=abc",
                "site",
            ),
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
    fn an_unknown_site_waits_for_consent_and_a_known_one_for_a_click() {
        let mut gate = Gate::default();
        assert!(gate.offer(link("https://a.example"), true, 0));
        let known = gate.hand(1).unwrap();
        assert_eq!(known.consent, Consent::Open);
        assert!(
            gate.consent(known.seq, true, 1).is_err(),
            "a known site needs no consent"
        );
        assert_eq!(gate.ready(known.seq, 2).unwrap(), link("https://a.example"));
        assert!(
            gate.ready(known.seq + 1, 2).is_err(),
            "only the link asked about"
        );
        gate.done(known.seq);
        assert!(gate.ready(known.seq, 3).is_err() && gate.hand(3).is_none());

        gate.offer(link("https://b.example"), false, 10);
        let asked = gate.hand(10).unwrap();
        assert_eq!(asked.consent, Consent::Ask);
        assert!(
            gate.ready(asked.seq, 11).is_err(),
            "nothing opens before Connect"
        );
        let allowed = gate.consent(asked.seq, true, 12).unwrap().unwrap();
        assert_eq!(allowed.site().as_str(), "https://b.example");
        assert!(
            gate.ready(asked.seq, 13).is_ok(),
            "kept to resume after sign-in"
        );
        gate.done(asked.seq);

        gate.offer(link("https://c.example"), false, 20);
        let denied = gate.hand(20).unwrap();
        assert_eq!(gate.consent(denied.seq, false, 21), Ok(None));
        assert!(gate.hand(22).is_none(), "denied is dropped");
    }

    #[test]
    fn nothing_is_answered_before_the_window_was_given_the_link() {
        let mut gate = Gate::default();
        gate.offer(link("https://a.example"), true, 0);
        assert!(gate.ready(1, 0).is_err(), "a guessed seq opens nothing");
    }

    #[test]
    fn a_link_is_handed_once_and_a_duplicate_is_not_queued() {
        let mut gate = Gate::default();
        assert!(gate.offer(link("https://a.example"), true, 0));
        let first = gate.hand(1).unwrap();
        assert!(
            !gate.offer(link("https://a.example"), true, 2),
            "the same link again"
        );
        // Asked again: the window lost it (a reload); it is dropped, not replayed.
        assert!(gate.hand(3).is_none());
        assert!(gate.ready(first.seq, 4).is_err());
        // Once done with, the same link may come again, and needs its own click.
        assert!(gate.offer(link("https://a.example"), true, 5));
        assert_ne!(gate.hand(6).unwrap().seq, first.seq);
    }

    #[test]
    fn a_newer_link_waits_behind_the_one_asked_about() {
        let mut gate = Gate::default();
        gate.offer(link("https://a.example"), false, 0);
        let a = gate.hand(0).unwrap();
        gate.offer(link("https://b.example"), false, 1);
        gate.offer(link("https://c.example"), false, 2);
        // The window still asks about a: Connect answers a, never c.
        let allowed = gate.consent(a.seq, true, 3).unwrap().unwrap();
        assert_eq!(allowed.site().as_str(), "https://a.example");
        gate.done(a.seq + 7);
        assert_eq!(
            gate.ready(a.seq, 4).unwrap(),
            allowed,
            "done names its link"
        );
        gate.done(a.seq);
        let c = gate.hand(5).unwrap();
        assert_eq!(c.link, link("https://c.example"), "the newest replaced b");
        gate.done(c.seq);
        assert!(gate.hand(6).is_none());
    }

    #[test]
    fn a_pending_link_expires_after_ten_minutes() {
        let mut gate = Gate::default();
        gate.offer(link("https://b.example"), false, 0);
        let p = gate.hand(PENDING_MS).unwrap();
        assert!(gate.consent(p.seq, true, PENDING_MS + 1).is_err());
        assert!(gate.hand(PENDING_MS + 1).is_none());
        gate.offer(link("https://a.example"), false, 0);
        gate.offer(link("https://c.example"), false, PENDING_MS);
        let c = gate.hand(PENDING_MS + 1).unwrap();
        assert_eq!(
            c.link,
            link("https://c.example"),
            "the expired one gives way"
        );
    }
}
