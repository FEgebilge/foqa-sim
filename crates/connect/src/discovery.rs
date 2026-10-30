//! `GET <origin>/.well-known/foqa-challenges.json` (A1): at most 8 KiB of
//! JSON, every URL in it on the site's own origin, a protocol version in
//! common.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::PROTOCOL;
use crate::http::Http;
use crate::origin::Origin;

pub const PATH: &str = "/.well-known/foqa-challenges.json";
const MAX_BYTES: u64 = 8 * 1024;
/// The upload limit when the site states none.
pub const DEFAULT_MAX_UPLOAD: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WellKnown {
    pub protocol: String,
    pub versions: Vec<u32>,
    #[serde(default)]
    pub name: String,
    pub api: BTreeMap<String, String>,
    pub auth: Auth,
    #[serde(default)]
    pub links: Links,
    #[serde(default)]
    pub limits: Limits,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Auth {
    pub authorize: String,
    #[serde(default)]
    pub device_verification: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Links {
    #[serde(default)]
    pub challenge: Option<String>,
    #[serde(default)]
    pub attempt: Option<String>,
    #[serde(default)]
    pub account: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Limits {
    #[serde(default)]
    pub max_upload_bytes: Option<u64>,
}

impl WellKnown {
    /// The API base of the version this client speaks.
    pub fn api(&self) -> &str {
        &self.api[&PROTOCOL.to_string()]
    }

    pub fn max_upload(&self) -> u64 {
        self.limits.max_upload_bytes.unwrap_or(DEFAULT_MAX_UPLOAD)
    }

    /// Checks the document for `origin`; the name is trimmed to 60
    /// characters of text.
    pub fn check(mut self, origin: &Origin) -> Result<Self, String> {
        if self.protocol != "foqa-challenges" {
            return Err("This site doesn't publish FOQA challenges.".into());
        }
        if !self.versions.contains(&PROTOCOL) || !self.api.contains_key(&PROTOCOL.to_string()) {
            let newer = self.versions.iter().all(|&v| v > PROTOCOL);
            let which = if newer { "newer" } else { "older" };
            return Err(format!("This site needs a {which} FOQA-Sim."));
        }
        let links = [
            &self.links.challenge,
            &self.links.attempt,
            &self.links.account,
        ];
        let urls = self
            .api
            .values()
            .chain([&self.auth.authorize])
            .chain(self.auth.device_verification.iter())
            .chain(links.into_iter().flatten());
        for url in urls {
            if !origin.owns(url) {
                return Err(format!(
                    "This site points FOQA-Sim at another address ({url}), so it isn't used."
                ));
            }
        }
        self.name = self
            .name
            .chars()
            .filter(|c| !c.is_control())
            .take(60)
            .collect::<String>()
            .trim()
            .to_owned();
        Ok(self)
    }
}

/// Why a site's document can't be used, in the app's words; `missing`:
/// the site has none (404), so it doesn't publish challenges (yet).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failed {
    pub missing: bool,
    pub text: String,
}

impl From<String> for Failed {
    fn from(text: String) -> Self {
        Self {
            missing: false,
            text,
        }
    }
}

impl From<Failed> for String {
    fn from(f: Failed) -> Self {
        f.text
    }
}

/// Fetches and checks the document of `origin`.
pub fn discover(http: &Http, origin: &Origin) -> Result<WellKnown, Failed> {
    let host = origin.host();
    let url = format!("{origin}{PATH}");
    let r = http
        .get(&url, None, MAX_BYTES)
        .map_err(|e| format!("Can't reach {host}: {}", e.0))?;
    if r.status == 404 {
        return Err(Failed {
            missing: true,
            text: format!("{host} doesn't publish FOQA challenges."),
        });
    }
    if !r.ok() {
        return Err(format!("{host} answered {} for its challenge list.", r.status).into());
    }
    if !r
        .content_type
        .as_deref()
        .is_some_and(|t| t.starts_with("application/json"))
    {
        return Err(format!("{host} doesn't publish FOQA challenges (not JSON).").into());
    }
    let doc: WellKnown = serde_json::from_slice(&r.body)
        .map_err(|_| format!("{host} published a challenge document FOQA-Sim can't read."))?;
    Ok(doc.check(origin)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::{Mock, Reply, well_known as doc};
    use serde_json::json;

    #[test]
    fn a_site_is_discovered_from_its_well_known_document() {
        let mock = Mock::start(|req, base| match req.path.as_str() {
            PATH => Reply::json(200, doc(base)),
            _ => Reply::json(404, json!({})),
        });
        let origin = Origin::parse(&mock.base, true).unwrap();
        let wk = discover(&Http::new("0.2.0"), &origin).unwrap();
        assert_eq!(wk.name, "TestSite");
        assert_eq!(wk.api(), format!("{}/api/foqa/v1", mock.base));
        assert_eq!(wk.max_upload(), 4096);
        let req = &mock.requests()[0];
        assert_eq!(req.header("x-foqa-protocol"), Some("1"));
        assert_eq!(req.header("user-agent"), Some("FOQA-Sim/0.2.0"));
        assert_eq!(req.header("origin"), None, "the app never sends Origin");
    }

    #[test]
    fn a_document_pointing_elsewhere_or_without_a_common_version_is_refused() {
        let origin = Origin::parse("https://example.org", false).unwrap();
        let good: WellKnown = serde_json::from_value(doc("https://example.org")).unwrap();
        assert!(good.clone().check(&origin).is_ok());
        let mut other = good.clone();
        other.auth.authorize = "https://evil.example/authorize".into();
        assert!(other.check(&origin).unwrap_err().contains("evil.example"));
        let mut link = good.clone();
        link.links.account = Some("http://example.org/#/account".into());
        assert!(
            link.check(&origin).is_err(),
            "another scheme is another origin"
        );
        let mut newer = good.clone();
        newer.versions = vec![2];
        assert_eq!(
            newer.check(&origin).unwrap_err(),
            "This site needs a newer FOQA-Sim."
        );
        let mut nonsense = good;
        nonsense.protocol = "other".into();
        assert!(nonsense.check(&origin).is_err());
    }

    #[test]
    fn missing_large_or_non_json_documents_are_refused() {
        let mock = Mock::start(|req, base| match req.path.as_str() {
            PATH if req.header("host").is_some() => {
                let mut big = doc(base);
                big["name"] = json!("x".repeat(9000));
                Reply::json(200, big)
            }
            _ => Reply::json(404, json!({})),
        });
        let origin = Origin::parse(&mock.base, true).unwrap();
        assert!(discover(&Http::new("0"), &origin).is_err(), "over 8 KiB");
        let html = Mock::start(|_, _| Reply::raw(200, "text/html", b"<html>".to_vec()));
        let origin = Origin::parse(&html.base, true).unwrap();
        let not_json = discover(&Http::new("0"), &origin).unwrap_err();
        assert!(not_json.text.contains("not JSON") && !not_json.missing);
        let none = Mock::start(|_, _| Reply::json(404, json!({})));
        let origin = Origin::parse(&none.base, true).unwrap();
        let missing = discover(&Http::new("0"), &origin).unwrap_err();
        assert!(missing.text.contains("doesn't publish") && missing.missing);
    }
}
