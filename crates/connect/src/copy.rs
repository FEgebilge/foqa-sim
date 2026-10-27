//! What the pilot reads (A6): the app's own plain copy for every error,
//! status, reject code and unverified reason; the site's `message` is
//! only shown under "Details". Also what each error means for a retry.

use crate::api::{ApiError, AttemptStatus, Error};

/// What a message is about: the site as shown (its ASCII host), the
/// local upload deadline and the site's upload limit.
#[derive(Debug, Clone, Copy)]
pub struct Ctx<'a> {
    pub site: &'a str,
    pub upload_by: Option<&'a str>,
    pub max_bytes: u64,
}

/// What to do after an error (A6 "Retry" column, B6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Retry {
    No,
    /// Read the slice again and resend, at most 3 times.
    Checksum,
    /// Wait for a sign-in; the upload stays queued.
    SignIn,
    /// After `Retry-After` seconds, else the backoff.
    After(Option<u64>),
    /// The next backoff step.
    Backoff,
}

pub fn retry(e: &Error) -> Retry {
    let api = match e {
        Error::Network(_) => return Retry::Backoff,
        Error::SignedOut => return Retry::SignIn,
        Error::Protocol(_) => return Retry::No,
        Error::Api(api) => api,
    };
    match (api.http, api.code.as_str()) {
        (400, "checksum_mismatch") => Retry::Checksum,
        (401, _) => Retry::SignIn,
        (429, "too_many_open_attempts") => Retry::No,
        (429, _) => Retry::After(api.retry_after),
        (500.., _) => Retry::After(api.retry_after),
        _ => Retry::No,
    }
}

pub fn error(e: &Error, ctx: Ctx) -> String {
    let site = ctx.site;
    let api = match e {
        Error::Network(_) => return unreachable(ctx),
        Error::SignedOut => return format!("Signed out of {site}. Sign in again to upload."),
        Error::Protocol(_) => {
            return format!(
                "{site} sent an answer FOQA-Sim couldn't read. Update FOQA-Sim, or report it to the site."
            );
        }
        Error::Api(api) => api,
    };
    api_error(api, ctx)
}

fn unreachable(ctx: Ctx) -> String {
    match ctx.upload_by {
        Some(by) => format!("Can't reach {}. Will retry until {by}.", ctx.site),
        None => format!("Can't reach {}. Will retry.", ctx.site),
    }
}

fn api_error(api: &ApiError, ctx: Ctx) -> String {
    let site = ctx.site;
    match (api.http, api.code.as_str()) {
        (400, "invalid_request") => {
            "The site couldn't read this request. Update FOQA-Sim, or report it to the site.".into()
        }
        (400, "bad_recording") => "The site couldn't read this recording.".into(),
        (400, "checksum_mismatch") => "The upload was damaged on the way. Retrying…".into(),
        (401, _) => format!("Signed out of {site}. Sign in again to upload."),
        (403, "handle_required") => {
            format!("Pick a display name on {site} first, then try again.")
        }
        (403, "forbidden") => format!("{site} doesn't allow this for your account."),
        (403, "bad_origin") => "Your network changed the request. Try another network.".into(),
        (404, _) => format!("This challenge or attempt is no longer on {site}."),
        (409, "conflict") => "This attempt already has a different recording.".into(),
        (409, "duplicate_recording") => {
            "This recording was already uploaded for another attempt.".into()
        }
        (409, "challenge_closed") => "This challenge is closed.".into(),
        (409, "version_superseded") => {
            "This challenge was updated. Open it again to fly the new version.".into()
        }
        (410, _) => match ctx.upload_by {
            Some(by) => {
                format!("Too late to upload: the window closed at {by}. Your local grade is kept.")
            }
            None => "Too late to upload: the window has closed. Your local grade is kept.".into(),
        },
        (413, _) => format!(
            "This recording is larger than {site} accepts ({}).",
            size(ctx.max_bytes)
        ),
        (426, _) => format!("{site} needs a newer FOQA-Sim. Update to continue."),
        (429, "too_many_open_attempts") => {
            "You have unfinished attempts. Finish or abandon one, then fly again.".into()
        }
        (429, _) => match api.retry_after {
            Some(n) => format!("{site} is busy. Retrying in {n} s."),
            None => format!("{site} is busy. Retrying soon."),
        },
        (500.., _) => unreachable(ctx),
        _ => format!("{site} refused: {}", api.message),
    }
}

/// Sign-in errors from `/token` (A6 last row).
pub fn sign_in_failed() -> &'static str {
    "Sign-in was cancelled or expired. Try again."
}

fn size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{} KB", bytes.div_ceil(1024))
    }
}

/// A server status in words; `scored` with its total and rank, or the
/// reasons it isn't ranked.
pub fn status(s: &AttemptStatus, site: &str) -> String {
    match s.status.as_str() {
        "issued" => "Waiting for your flight.".into(),
        "queued" | "scoring" => "Uploaded. Scoring…".into(),
        "scored" => {
            let total = s
                .score()
                .map_or("Scored".to_owned(), |t| format!("Scored {t:.1}"));
            match (s.verified, s.rank) {
                (true, Some(rank)) => format!("{total}, #{rank}"),
                (true, None) => total,
                (false, _) => {
                    let why: Vec<String> =
                        s.unverified.iter().map(|r| unverified(r, site)).collect();
                    if why.is_empty() {
                        format!("{total}, not ranked.")
                    } else {
                        format!("{total}, not ranked: {}", why.join(" "))
                    }
                }
            }
        }
        "rejected" => s
            .reject
            .as_ref()
            .map_or("Rejected.".into(), |r| reject(&r.code, site)),
        "failed" => {
            "The site couldn't score this attempt. It's their side; try again later.".into()
        }
        "expired" => "Not uploaded in time.".into(),
        "abandoned" => "Abandoned.".into(),
        "removed" => "Removed by the site's moderators.".into(),
        _ => "Waiting for the site".into(),
    }
}

pub fn reject(code: &str, site: &str) -> String {
    match code {
        "bad_recording" => "The recording couldn't be read.".into(),
        "not_scorable" => "No landing was found to grade after the start.".into(),
        _ => format!("Rejected ({code}). See details on {site}."),
    }
}

pub fn unverified(code: &str, site: &str) -> String {
    match code {
        "not_valid" => "A challenge rule wasn't met (see the checks).".into(),
        "marker_mismatch" => "The recording belongs to a different attempt.".into(),
        "too_late" => "Uploaded after the deadline.".into(),
        "outside_window" => "Flown outside the challenge's dates.".into(),
        "duration" => "The flight took longer in the sim than in real time since the start (sim rate or pause).".into(),
        "duplicate" => "This recording was already used.".into(),
        _ => format!("{code}: see details on {site}."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn api(http: u16, code: &str) -> Error {
        Error::Api(ApiError {
            http,
            code: code.into(),
            message: "nope".into(),
            retry_after: Some(30),
            request_id: None,
        })
    }

    const CTX: Ctx = Ctx {
        site: "example.org",
        upload_by: Some("28 Oct 18:02"),
        max_bytes: 2 * 1024 * 1024,
    };

    #[test]
    fn every_a6_error_has_its_own_copy_and_retry() {
        let cases: &[(u16, &str, Retry, &str)] = &[
            (
                400,
                "invalid_request",
                Retry::No,
                "couldn't read this request",
            ),
            (
                400,
                "bad_recording",
                Retry::No,
                "couldn't read this recording",
            ),
            (
                400,
                "checksum_mismatch",
                Retry::Checksum,
                "damaged on the way",
            ),
            (
                401,
                "invalid_token",
                Retry::SignIn,
                "Signed out of example.org",
            ),
            (
                403,
                "handle_required",
                Retry::No,
                "Pick a display name on example.org",
            ),
            (403, "forbidden", Retry::No, "doesn't allow this"),
            (403, "bad_origin", Retry::No, "network changed the request"),
            (404, "not_found", Retry::No, "no longer on example.org"),
            (409, "conflict", Retry::No, "different recording"),
            (409, "duplicate_recording", Retry::No, "another attempt"),
            (409, "challenge_closed", Retry::No, "is closed"),
            (409, "version_superseded", Retry::No, "was updated"),
            (
                410,
                "attempt_expired",
                Retry::No,
                "window closed at 28 Oct 18:02",
            ),
            (413, "too_large", Retry::No, "accepts (2.0 MB)"),
            (
                426,
                "unsupported_version",
                Retry::No,
                "needs a newer FOQA-Sim",
            ),
            (
                429,
                "rate_limited",
                Retry::After(Some(30)),
                "busy. Retrying in 30 s",
            ),
            (429, "busy", Retry::After(Some(30)), "busy"),
            (
                429,
                "too_many_open_attempts",
                Retry::No,
                "unfinished attempts",
            ),
            (
                503,
                "busy",
                Retry::After(Some(30)),
                "Can't reach example.org. Will retry until",
            ),
            (500, "", Retry::After(Some(30)), "Can't reach"),
            (418, "teapot", Retry::No, "example.org refused: nope"),
        ];
        for (http, code, want_retry, text) in cases {
            let e = api(*http, code);
            assert_eq!(retry(&e), *want_retry, "{http} {code}");
            assert!(
                error(&e, CTX).contains(text),
                "{http} {code}: {}",
                error(&e, CTX)
            );
        }
        assert_eq!(retry(&Error::Network("down".into())), Retry::Backoff);
        assert!(error(&Error::Network("down".into()), CTX).starts_with("Can't reach"));
        assert_eq!(retry(&Error::SignedOut), Retry::SignIn);
    }

    #[test]
    fn statuses_read_in_plain_words_and_unknown_ones_wait() {
        let mut s: AttemptStatus = serde_json::from_value(serde_json::json!({
            "id": "a", "status": "scored", "verified": true, "rank": 3,
            "result": { "score": { "total": 87.43 } }
        }))
        .unwrap();
        assert_eq!(status(&s, "x.org"), "Scored 87.4, #3");
        s.verified = false;
        s.unverified = vec!["duration".into(), "new_reason".into()];
        let text = status(&s, "x.org");
        assert!(
            text.starts_with("Scored 87.4, not ranked: The flight took longer"),
            "{text}"
        );
        assert!(text.ends_with("new_reason: see details on x.org."));
        s.status = "rejected".into();
        s.reject = Some(crate::api::Reject {
            code: "not_scorable".into(),
            detail: None,
        });
        assert_eq!(
            status(&s, "x.org"),
            "No landing was found to grade after the start."
        );
        s.status = "moderating".into();
        assert!(!s.is_final());
        assert_eq!(status(&s, "x.org"), "Waiting for the site");
        for (code, text) in [
            ("queued", "Uploaded. Scoring…"),
            ("expired", "Not uploaded in time."),
            ("removed", "Removed by the site's moderators."),
        ] {
            s.status = code.into();
            assert_eq!(status(&s, "x.org"), text);
        }
    }
}
