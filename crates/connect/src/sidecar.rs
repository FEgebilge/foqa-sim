//! An attempt's site state (B5): `site.json` beside the core's files in
//! the attempt folder, owned by connect alone, and its append-only
//! `site.log.jsonl`. The core's state machine never reads either. Nothing
//! secret is written here: the marker `ref` is not a secret.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::api::AttemptStatus;
use crate::jsonl;

pub const FILE: &str = "site.json";
pub const LOG: &str = "site.log.jsonl";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SiteState {
    /// Asking the site for an attempt.
    Issuing,
    /// The site issued it; the flight is on.
    Issued,
    /// No attempt could be issued: graded here, never uploaded (A9).
    Practice,
    UploadPending,
    UploadRetry,
    // The server's statuses (A4).
    Queued,
    Scoring,
    Scored,
    Rejected,
    Failed,
    Expired,
    Removed,
    /// A status this app doesn't know: not final, keeps polling.
    Waiting,
    /// The site refused the upload for good.
    UploadRefused,
    /// Signed in as someone else: the attempt belongs to the first user.
    UploadAbandoned,
    /// Telling the site the attempt was abandoned (queued if offline).
    Abandoning,
    Abandoned,
}

impl SiteState {
    pub fn from_server(status: &str) -> Self {
        match status {
            "queued" => Self::Queued,
            "scoring" => Self::Scoring,
            "scored" => Self::Scored,
            "rejected" => Self::Rejected,
            "failed" => Self::Failed,
            "expired" => Self::Expired,
            "abandoned" => Self::Abandoned,
            "removed" => Self::Removed,
            _ => Self::Waiting,
        }
    }

    /// Reported by the server.
    fn server(self) -> bool {
        use SiteState::*;
        matches!(
            self,
            Queued | Scoring | Scored | Rejected | Failed | Expired | Removed | Waiting | Abandoned
        )
    }

    /// Nothing more happens on its own.
    pub fn is_final(self) -> bool {
        use SiteState::*;
        matches!(
            self,
            Practice
                | Scored
                | Rejected
                | Failed
                | Expired
                | Removed
                | UploadRefused
                | UploadAbandoned
                | Abandoned
        )
    }

    /// Still to reach the site: what "Sign out" warns about.
    pub fn unsent(self) -> bool {
        matches!(self, Self::Issued | Self::UploadPending | Self::UploadRetry)
    }

    /// Uploaded and waiting for a final status.
    pub fn polled(self) -> bool {
        matches!(self, Self::Queued | Self::Scoring | Self::Waiting)
    }

    pub fn can_become(self, to: Self) -> bool {
        use SiteState::*;
        match (self, to) {
            (Issuing, Issued | Practice) => true,
            (Issued, UploadPending | Abandoning | Abandoned | Expired) => true,
            (
                UploadPending | UploadRetry,
                UploadPending | UploadRetry | UploadRefused | UploadAbandoned,
            ) => true,
            (UploadPending | UploadRetry | Queued | Scoring | Waiting, s) => s.server(),
            (Abandoning, Abandoning | Abandoned) => true,
            _ => false,
        }
    }
}

/// `site.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sidecar {
    pub origin: String,
    /// The account it was issued to (`/me` id); another account never
    /// uploads it.
    pub user_id: Option<String>,
    pub challenge: String,
    pub version: u32,
    pub state: SiteState,
    /// The server's attempt id.
    pub attempt_id: Option<String>,
    #[serde(rename = "ref")]
    pub marker_ref: Option<String>,
    pub definition_sha256: Option<String>,
    /// The definition flown equals the one the site issued.
    pub definition_match: Option<bool>,
    /// Local deadlines: response received + `fly_within_s` / `upload_within_s`.
    pub fly_by_ms: Option<i64>,
    pub upload_by_ms: Option<i64>,
    /// The site's own display times.
    pub upload_by: Option<String>,
    pub tries: u32,
    pub checksum_tries: u32,
    /// When the uploader next acts on it.
    pub next_at_ms: Option<i64>,
    pub uploaded_at_ms: Option<i64>,
    /// SHA-256 of the bytes uploaded.
    pub sha256: Option<String>,
    /// The plain copy shown now (A6) and the site's own message.
    pub text: String,
    pub detail: Option<String>,
    pub last: Option<AttemptStatus>,
}

impl Sidecar {
    pub fn new(origin: &str, user_id: Option<String>, challenge: &str, version: u32) -> Self {
        Self {
            origin: origin.into(),
            user_id,
            challenge: challenge.into(),
            version,
            state: SiteState::Issuing,
            attempt_id: None,
            marker_ref: None,
            definition_sha256: None,
            definition_match: None,
            fly_by_ms: None,
            upload_by_ms: None,
            upload_by: None,
            tries: 0,
            checksum_tries: 0,
            next_at_ms: None,
            uploaded_at_ms: None,
            sha256: None,
            text: "Asking the site for an attempt…".into(),
            detail: None,
            last: None,
        }
    }

    pub fn path(dir: &Path) -> PathBuf {
        dir.join(FILE)
    }

    pub fn read(dir: &Path) -> Option<Self> {
        serde_json::from_slice(&fs::read(Self::path(dir)).ok()?).ok()
    }

    /// Written aside and renamed, so a crash never leaves half a file.
    pub fn write(&self, dir: &Path) -> Result<(), String> {
        let part = dir.join(format!("{FILE}.part"));
        let bytes = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        fs::write(&part, bytes).map_err(|e| e.to_string())?;
        fs::rename(part, Self::path(dir)).map_err(|e| e.to_string())
    }

    /// Moves to `to` (refused when the lifecycle doesn't allow it), writes
    /// the file and logs `event` with `fields`.
    pub fn go(
        &mut self,
        dir: &Path,
        to: SiteState,
        event: &str,
        fields: Value,
    ) -> Result<(), String> {
        if self.state != to && !self.state.can_become(to) {
            return Err(format!("site state {:?} can't become {to:?}", self.state));
        }
        self.state = to;
        self.write(dir)?;
        log(dir, event, fields);
        Ok(())
    }
}

/// One line of `site.log.jsonl`.
pub fn log(dir: &Path, event: &str, fields: Value) {
    let mut line = json!({ "event": event });
    if let (Value::Object(l), Value::Object(f)) = (&mut line, fields) {
        l.extend(f);
    }
    if let Err(e) = jsonl::append(&dir.join(LOG), line) {
        eprintln!("site log: {e}");
    }
}

pub fn read_log(dir: &Path) -> Vec<Value> {
    jsonl::read(&dir.join(LOG))
}

#[cfg(test)]
mod tests {
    use super::*;
    use SiteState::*;

    #[test]
    fn the_site_lifecycle_allows_only_its_moves() {
        assert!(Issuing.can_become(Issued) && Issuing.can_become(Practice));
        assert!(Issued.can_become(UploadPending) && Issued.can_become(Abandoning));
        assert!(UploadPending.can_become(UploadRetry) && UploadRetry.can_become(Queued));
        assert!(Queued.can_become(Scored) && Waiting.can_become(Scoring));
        assert!(
            !Practice.can_become(UploadPending),
            "practice is never uploaded"
        );
        assert!(
            !Issuing.can_become(UploadPending),
            "nothing uploads without an issued attempt"
        );
        assert!(!Scored.can_become(Queued) && !UploadRefused.can_become(UploadRetry));
        assert!(!Issued.can_become(Queued), "an upload comes first");
        assert_eq!(SiteState::from_server("moderating"), Waiting);
        assert!(!Waiting.is_final() && Scored.is_final() && Practice.is_final());
    }

    #[test]
    fn a_sidecar_moves_writes_and_logs() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let mut s = Sidecar::new("https://example.org", Some("u1".into()), "ltfj-06", 1);
        s.write(dir).unwrap();
        s.go(dir, Issued, "issued", json!({ "attempt_id": "a1" }))
            .unwrap();
        assert!(s.go(dir, Queued, "x", json!({})).is_err());
        assert_eq!(Sidecar::read(dir).unwrap().state, Issued);
        let log = read_log(dir);
        assert_eq!(
            (log[0]["event"].as_str(), log[0]["attempt_id"].as_str()),
            (Some("issued"), Some("a1"))
        );
        assert!(
            !fs::read_to_string(Sidecar::path(dir))
                .unwrap()
                .contains("token")
        );
    }
}
