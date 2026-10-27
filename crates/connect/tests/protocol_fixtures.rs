//! The protocol's example fixtures (docs/challenge-protocol.md A11,
//! `fixtures/protocol/`) read with the client's own types: what a site
//! copies into its tests is what this client accepts.

use std::fs;
use std::path::{Path, PathBuf};

use connect::api::{ApiError, AttemptStatus, ChallengeEntry, IssuedAttempt, TokenResponse, User};
use connect::auth::DeviceCode;
use connect::copy::{self, Ctx};
use connect::discovery::WellKnown;
use connect::http::Response;
use connect::origin::Origin;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::value::RawValue;
use sha2::{Digest, Sha256};

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/protocol")
}

fn read<T: DeserializeOwned>(name: &str) -> T {
    let text = fs::read_to_string(dir().join(name)).unwrap();
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// The definition's bytes as sent hash to the stated `definition_sha256`,
/// and this app can fly them.
fn definition_matches(definition: &RawValue, sha256: &str) {
    assert_eq!(format!("{:x}", Sha256::digest(definition.get())), sha256);
    challenge::Challenge::parse(definition.get()).unwrap();
}

#[test]
fn discovery_and_sign_in_fixtures_read() {
    let origin = Origin::parse("https://landings.example", false).unwrap();
    let wk = read::<WellKnown>("well-known.json").check(&origin).unwrap();
    assert_eq!(wk.api(), "https://landings.example/api/foqa/v1");
    let t: TokenResponse = read("token.json");
    assert_eq!(t.user.unwrap().handle.as_deref(), Some("ege"));
    let t: TokenResponse = read("token-no-handle.json");
    assert_eq!(t.user.unwrap().handle, None, "a site may send handle null");
    #[derive(Deserialize)]
    struct Me {
        user: User,
    }
    assert!(!read::<Me>("me.json").user.id.is_empty());
    let code: DeviceCode = read("device-code.json");
    assert!(origin.owns(&code.verification_uri));
}

#[test]
fn challenge_fixtures_carry_their_definition_bytes() {
    #[derive(Deserialize)]
    struct List {
        challenges: Vec<ChallengeEntry>,
    }
    #[derive(Deserialize)]
    struct One {
        challenge: ChallengeEntry,
    }
    #[derive(Deserialize)]
    struct Issued {
        attempt: IssuedAttempt,
        definition: Box<RawValue>,
    }
    let mut entries = read::<List>("challenges.json").challenges;
    entries.push(read::<One>("challenge.json").challenge);
    for e in &entries {
        definition_matches(&e.definition, &e.definition_sha256);
    }
    let issued: Issued = read("issue-attempt.json");
    definition_matches(&issued.definition, &issued.attempt.definition_sha256);
    assert_eq!(issued.attempt.marker_ref.len(), 22);
}

#[test]
fn every_status_fixture_reads_and_only_known_ones_are_final() {
    #[derive(Deserialize)]
    struct One {
        attempt: AttemptStatus,
    }
    #[derive(Deserialize)]
    struct Page {
        attempts: Vec<AttemptStatus>,
        next: Option<String>,
    }
    let mut seen = 0;
    for entry in fs::read_dir(dir()).unwrap() {
        let name = entry.unwrap().file_name().into_string().unwrap();
        let Some(case) = name
            .strip_prefix("attempt-")
            .and_then(|n| n.strip_suffix(".json"))
        else {
            continue;
        };
        let a = read::<One>(&name).attempt;
        let known = !matches!(case, "issued" | "queued" | "scoring" | "unknown");
        assert_eq!(a.is_final(), known, "{name}");
        assert!(!copy::status(&a, "landings.example").is_empty());
        if a.status == "scored" {
            assert!(a.score().is_some(), "{name}: the server's result");
        }
        seen += 1;
    }
    assert_eq!(
        seen, 11,
        "the nine statuses, scored unverified and an unknown one"
    );
    let page: Page = read("me-attempts.json");
    assert!(page.attempts.len() == 1 && page.next.is_none());
}

#[test]
fn every_error_fixture_has_its_code_and_its_own_copy() {
    let ctx = Ctx {
        site: "landings.example",
        upload_by: None,
        max_bytes: 2 * 1024 * 1024,
    };
    let token_errors = [
        "authorization_pending",
        "slow_down",
        "access_denied",
        "expired_token",
        "invalid_grant",
    ];
    let mut seen = 0;
    for entry in fs::read_dir(dir().join("errors")).unwrap() {
        let name = entry.unwrap().file_name().into_string().unwrap();
        let (http, code) = name.strip_suffix(".json").unwrap().split_once('-').unwrap();
        let r = Response {
            status: http.parse().unwrap(),
            body: fs::read(dir().join("errors").join(&name)).unwrap(),
            request_id: None,
            retry_after: None,
            content_type: Some("application/json".into()),
        };
        let e = ApiError::from_response(&r);
        assert_eq!(e.code, code, "{name}");
        if !token_errors.contains(&code) {
            let text = copy::error(&connect::api::Error::Api(e), ctx);
            assert!(!text.contains(" refused: "), "{name}: no copy of its own");
        }
        seen += 1;
    }
    assert_eq!(seen, 24);
}
