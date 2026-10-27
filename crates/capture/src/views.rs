//! The challenge part of the in-sim views (docs/challenge-protocol.md B9):
//! the attempt strip, the EFB's challenge list and briefing, and the
//! views' attempt commands, which the app carries out. The app renders
//! every word; the views stay dumb. Each message fits the Comm Bus cap,
//! fields cut to compact sizes first.

use std::sync::mpsc::Sender;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::history::{JSON_MAX, REQUEST_MAX_BYTES, Reply};
use crate::panel::{Ack, Msg, VERSION};

/// App to view: the current attempt (`AttemptStrip`).
pub const ATTEMPT: &str = "FOQA_SIM_ATTEMPT";
/// App to view: the reply to `list_challenges`.
pub const CHALLENGES: &str = "FOQA_SIM_CHALLENGES";
/// App to view: the reply to `briefing`.
pub const BRIEFING: &str = "FOQA_SIM_BRIEFING";
/// Challenges in one list.
pub const CARDS_MAX: usize = 12;
/// A library key, as the app makes them (`<id>-v<version>`).
const KEY_MAX_BYTES: usize = 80;
/// Characters of a strip's text, a title, a line, a pre-flight text or fix.
const TEXT_MAX: usize = 160;
const TITLE_MAX: usize = 60;
const LINE_MAX: usize = 120;
const LINES_MAX: usize = 12;
/// Pre-flight lines (B8 has four checks).
const PREFLIGHT_MAX: usize = 6;
/// Characters of an airport or runway.
const CODE_MAX: usize = 16;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttemptStrip {
    pub v: u32,
    /// The local attempt id.
    pub id: String,
    /// The challenge's library key, for Fly again.
    pub key: String,
    /// Attempt number on this challenge on this PC.
    pub n: u32,
    /// `placing`, `flying`, `gate`, `rollout`, `finished`, `graded`,
    /// `abandoned` or `failed_start`.
    pub stage: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub landed: Option<Landed>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local: Option<Local>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site: Option<SiteStrip>,
    /// "Attempt 2 · Landed 142 fpm · Graded 87.4 · Scored 87.4, #3"
    pub text: String,
    pub can_fly_again: bool,
    /// The attempt is placed and not yet landed or abandoned: Finish works.
    pub can_finish: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Landed {
    pub sink_fpm: f64,
    pub peak_g: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Local {
    pub valid: bool,
    pub score: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SiteStrip {
    /// `practice`, `uploading`, `waiting`, `scoring`, `scored`, `rejected`,
    /// `unverified`, `expired`, `signed_out` or `unknown`.
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rank: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChallengeCard {
    pub key: String,
    pub title: String,
    pub airport: String,
    pub runway: String,
    /// "File" or "<name> (<ascii host>)".
    pub source: String,
    /// `open`, `upcoming`, `closed` or `needs_update`.
    pub state: String,
    /// The last attempt in words.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rank: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreflightLine {
    /// `sim`, `aircraft`, `weather` or `ranking`.
    pub name: String,
    /// `ok`, `warn`, `fail` or `unknown`.
    pub state: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BriefingMsg {
    pub v: u32,
    pub req: String,
    pub key: String,
    pub title: String,
    pub lines: Vec<String>,
    pub preflight: Vec<PreflightLine>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// A view's attempt command, for the app.
#[derive(Debug, Clone, PartialEq)]
pub enum AppCommand {
    ListChallenges {
        req: String,
    },
    Briefing {
        req: String,
        key: String,
    },
    /// Create, issue (unless practice) and place, as the app's Place.
    Place {
        key: String,
        practice: bool,
    },
    /// Abandon the current or last attempt and start its challenge again.
    /// Each names the attempt its strip shows (`id`); the app refuses
    /// another, so a double press never acts on the next attempt.
    FlyAgain {
        id: String,
    },
    Finish {
        id: String,
    },
    Abandon {
        id: String,
    },
}

impl AppCommand {
    pub fn name(&self) -> &'static str {
        match self {
            Self::ListChallenges { .. } => "list_challenges",
            Self::Briefing { .. } => "briefing",
            Self::Place { .. } => "place",
            Self::FlyAgain { .. } => "fly_again",
            Self::Finish { .. } => "finish",
            Self::Abandon { .. } => "abandon",
        }
    }

    /// Answered on its own event, not in `Status.ack`.
    pub fn is_query(&self) -> bool {
        matches!(self, Self::ListChallenges { .. } | Self::Briefing { .. })
    }

    /// From a view's message `v`; `None` when `cmd` is not an attempt
    /// command. A bad one is refused with its reason.
    pub(crate) fn parse(v: &Value) -> Option<Result<Self, &'static str>> {
        let text = |k: &str| v[k].as_str().unwrap_or_default().to_owned();
        let cmd = match v["cmd"].as_str()? {
            "list_challenges" => Self::ListChallenges { req: text("req") },
            "briefing" => Self::Briefing {
                req: text("req"),
                key: text("key"),
            },
            "place" => Self::Place {
                key: text("key"),
                practice: v["practice"].as_bool().unwrap_or(false),
            },
            "fly_again" => Self::FlyAgain { id: text("id") },
            "finish" => Self::Finish { id: text("id") },
            "abandon" => Self::Abandon { id: text("id") },
            _ => return None,
        };
        Some(cmd.check(v))
    }

    fn check(self, v: &Value) -> Result<Self, &'static str> {
        if v["v"] != VERSION {
            return Err("unsupported version");
        }
        let (req, key) = match &self {
            Self::ListChallenges { req } => (req.as_str(), None),
            Self::Briefing { req, key } => (req.as_str(), Some(key.as_str())),
            Self::Place { key, .. } => ("", Some(key.as_str())),
            Self::FlyAgain { id } | Self::Finish { id } | Self::Abandon { id } => {
                let ok = (1..=KEY_MAX_BYTES).contains(&id.len())
                    && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
                return if ok { Ok(self) } else { Err("no such attempt") };
            }
        };
        if req.len() + key.map_or(0, str::len) > REQUEST_MAX_BYTES {
            return Err("request too long");
        }
        let bad_key = key.is_some_and(|k| {
            k.is_empty()
                || k.len() > KEY_MAX_BYTES
                || !k
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        });
        if bad_key {
            return Err("no such challenge");
        }
        Ok(self)
    }
}

/// `s` cut to `max` characters, "…" marking the cut.
fn clip(s: &str, max: usize) -> String {
    match s.char_indices().nth(max.saturating_sub(1)) {
        Some((i, _)) if s.chars().count() > max => format!("{}…", &s[..i]),
        _ => s.to_owned(),
    }
}

/// The `FOQA_SIM_ATTEMPT` message, its text cut to fit.
pub fn attempt_message(strip: &AttemptStrip) -> String {
    let mut s = strip.clone();
    s.v = VERSION;
    s.text = clip(&s.text, TEXT_MAX);
    if let Some(site) = &mut s.site {
        site.reason = site.reason.as_deref().map(|r| clip(r, TEXT_MAX));
    }
    json!(s).to_string()
}

/// The `FOQA_SIM_CHALLENGES` reply: at most `CARDS_MAX` cards, fewer to fit.
pub fn challenges_message(req: &str, cards: &[ChallengeCard]) -> String {
    let mut cards: Vec<ChallengeCard> = cards.iter().take(CARDS_MAX).map(compact_card).collect();
    loop {
        let msg = json!({ "v": VERSION, "req": req, "challenges": cards }).to_string();
        if msg.len() <= JSON_MAX || cards.pop().is_none() {
            return msg;
        }
    }
}

fn compact_card(c: &ChallengeCard) -> ChallengeCard {
    ChallengeCard {
        title: clip(&c.title, TITLE_MAX),
        airport: clip(&c.airport, CODE_MAX),
        runway: clip(&c.runway, CODE_MAX),
        source: clip(&c.source, TITLE_MAX),
        last: c.last.as_deref().map(|l| clip(l, TEXT_MAX)),
        ..c.clone()
    }
}

/// The `FOQA_SIM_BRIEFING` reply, briefing lines then pre-flight lines
/// dropped from the end to fit; when even that does not, an `error` reply
/// naming the briefing only.
pub fn briefing_message(b: &BriefingMsg) -> String {
    let mut b = BriefingMsg {
        v: VERSION,
        title: clip(&b.title, TITLE_MAX),
        lines: b
            .lines
            .iter()
            .take(LINES_MAX)
            .map(|l| clip(l, LINE_MAX))
            .collect(),
        preflight: b
            .preflight
            .iter()
            .take(PREFLIGHT_MAX)
            .map(|p| PreflightLine {
                text: clip(&p.text, TEXT_MAX),
                fix: p.fix.as_deref().map(|f| clip(f, TEXT_MAX)),
                ..p.clone()
            })
            .collect(),
        ..b.clone()
    };
    loop {
        let msg = json!(b).to_string();
        if msg.len() <= JSON_MAX {
            return msg;
        }
        if b.lines.pop().is_none() && b.preflight.pop().is_none() {
            break;
        }
    }
    // `req` and `key` come from a checked request; clipped all the same.
    json!(BriefingMsg {
        req: clip(&b.req, REQUEST_MAX_BYTES),
        key: clip(&b.key, KEY_MAX_BYTES),
        title: String::new(),
        error: Some(BRIEFING_TOO_LONG.into()),
        ..b
    })
    .to_string()
}

const BRIEFING_TOO_LONG: &str = "This briefing is too long for the sim; open it in the app.";

/// The app's way to the views: what it publishes and its answers to the
/// views' attempt commands. Cloned freely; sends are dropped once the
/// feed is gone.
#[derive(Debug, Clone)]
pub struct Views(pub(crate) Sender<Msg>);

impl Views {
    /// The current attempt, sent each second (at once on change); `done`
    /// when nothing more will happen to it: it goes on for ten minutes.
    pub fn attempt(&self, strip: &AttemptStrip, done: bool) {
        let json = attempt_message(strip);
        let _ = self.0.send(Msg::Attempt { json, done });
    }

    pub fn challenges(&self, req: &str, cards: &[ChallengeCard]) {
        self.reply(CHALLENGES, challenges_message(req, cards));
    }

    pub fn briefing(&self, b: &BriefingMsg) {
        self.reply(BRIEFING, briefing_message(b));
    }

    /// The answer to a command, in the next `Status.ack`.
    pub fn ack(&self, cmd: &AppCommand, result: Result<(), String>) {
        let message = result.map_err(|e| clip(&e, TEXT_MAX));
        let _ = self.0.send(Msg::Ack(Ack::new(cmd.name(), message)));
    }

    fn reply(&self, event: &'static str, msg: String) {
        let _ = self.0.send(Msg::Reply(Reply {
            event,
            msgs: vec![msg],
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip() -> AttemptStrip {
        AttemptStrip {
            v: 2,
            id: "3f0e7f8e-6c2b-4d55-9a57-1b1f2a8d0c11".into(),
            key: "ltfj-06-v1".into(),
            n: 2,
            stage: "graded".into(),
            landed: Some(Landed {
                sink_fpm: 142.0,
                peak_g: 1.18,
            }),
            local: Some(Local {
                valid: true,
                score: Some(87.4),
            }),
            site: Some(SiteStrip {
                state: "scored".into(),
                score: Some(87.4),
                rank: Some(3),
                reason: None,
            }),
            text: "Attempt 2 · Landed 142 fpm · Graded 87.4 · Scored 87.4, #3".into(),
            can_fly_again: true,
            can_finish: false,
        }
    }

    fn card(n: usize) -> ChallengeCard {
        ChallengeCard {
            key: format!("challenge-{n}-v1"),
            title: "T".repeat(200),
            airport: "LTFJ".into(),
            runway: "06".into(),
            source: "File".into(),
            state: "open".into(),
            last: Some("Attempt 3 · Graded 81.0".into()),
            rank: None,
        }
    }

    #[test]
    fn the_strip_round_trips_with_its_text_cut() {
        let s = strip();
        let back: AttemptStrip = serde_json::from_str(&attempt_message(&s)).unwrap();
        assert_eq!(back, s);
        let v: Value = serde_json::from_str(&attempt_message(&s)).unwrap();
        assert!(v.get("landed").is_some() && v["site"].get("reason").is_none());
        let mut long = s.clone();
        long.text = "é".repeat(1000);
        long.site.as_mut().unwrap().reason = Some("x".repeat(9000));
        let msg = attempt_message(&long);
        assert!(msg.len() <= JSON_MAX, "{}", msg.len());
        let back: AttemptStrip = serde_json::from_str(&msg).unwrap();
        assert_eq!(back.text.chars().count(), TEXT_MAX);
        assert!(back.text.ends_with('…'));
        // A local attempt has no site part.
        let mut local = s;
        local.site = None;
        assert!(!attempt_message(&local).contains("\"site\""));
    }

    #[test]
    fn the_list_holds_twelve_cards_at_most_and_fits() {
        let cards: Vec<_> = (0..20).map(card).collect();
        let msg = challenges_message("c1", &cards);
        assert!(msg.len() <= JSON_MAX);
        let v: Value = serde_json::from_str(&msg).unwrap();
        assert_eq!((v["v"].as_u64(), v["req"].as_str()), (Some(2), Some("c1")));
        let back: Vec<ChallengeCard> = serde_json::from_value(v["challenges"].clone()).unwrap();
        assert_eq!(back.len(), CARDS_MAX);
        assert_eq!(back[0].key, "challenge-0-v1", "newest first, as given");
        assert_eq!(back[0].title.chars().count(), TITLE_MAX);
        // Cards too big for one message are dropped from the end.
        let mut fat = card(0);
        fat.last = Some("\"".repeat(TEXT_MAX));
        fat.key = "k".repeat(KEY_MAX_BYTES);
        fat.airport = "A".repeat(2000);
        fat.source = "s".repeat(TITLE_MAX);
        let msg = challenges_message("c2", &vec![fat; 30]);
        assert!(msg.len() <= JSON_MAX);
        let v: Value = serde_json::from_str(&msg).unwrap();
        let n = v["challenges"].as_array().unwrap().len();
        assert!((1..CARDS_MAX).contains(&n), "{n}: fewer, to fit");
    }

    #[test]
    fn the_briefing_drops_lines_to_fit() {
        let b = BriefingMsg {
            v: 2,
            req: "b1".into(),
            key: "ltfj-06-v1".into(),
            title: "LTFJ 06 gusty".into(),
            lines: vec!["Start 8 nm, 2500 ft, 160 kt".into(); 30],
            preflight: vec![PreflightLine {
                name: "sim".into(),
                state: "fail".into(),
                text: "MSFS isn't connected.".into(),
                fix: Some("Start MSFS 2024, then load a flight.".into()),
            }],
            error: None,
        };
        let back: BriefingMsg = serde_json::from_str(&briefing_message(&b)).unwrap();
        assert_eq!(back.lines.len(), LINES_MAX);
        assert_eq!(back.preflight, b.preflight);
        let mut fat = b.clone();
        fat.preflight = vec![b.preflight[0].clone(); 40];
        fat.preflight
            .iter_mut()
            .for_each(|p| p.text = "w".repeat(TEXT_MAX));
        let back: BriefingMsg = serde_json::from_str(&briefing_message(&fat)).unwrap();
        assert_eq!(back.preflight.len(), PREFLIGHT_MAX);
        fat.title = "t".repeat(5000);
        fat.lines = vec!["l".repeat(LINE_MAX); LINES_MAX];
        let msg = briefing_message(&fat);
        assert!(msg.len() <= JSON_MAX, "{}", msg.len());
        // Wide characters and escapes: the pre-flight lines go too, and at
        // worst the reply says so instead of passing the cap.
        let wide = "\u{1}".repeat(TEXT_MAX);
        let line = PreflightLine {
            text: wide.clone(),
            fix: Some(wide.clone()),
            ..b.preflight[0].clone()
        };
        let mut huge = b.clone();
        huge.title = wide.clone();
        huge.lines = vec!["\u{1}".repeat(LINE_MAX); LINES_MAX];
        huge.preflight = vec![line; PREFLIGHT_MAX];
        let msg = briefing_message(&huge);
        assert!(msg.len() <= JSON_MAX, "{}", msg.len());
        let back: BriefingMsg = serde_json::from_str(&msg).unwrap();
        assert!(back.lines.is_empty() && back.preflight.len() < PREFLIGHT_MAX);
        let mut wider = huge.clone();
        wider.title = "\u{1}".repeat(JSON_MAX);
        wider.key = "k".repeat(JSON_MAX);
        let msg = briefing_message(&wider);
        assert!(msg.len() <= JSON_MAX, "{}", msg.len());
        let back: BriefingMsg = serde_json::from_str(&msg).unwrap();
        assert_eq!(back.error.as_deref(), Some(BRIEFING_TOO_LONG));
        assert!(back.title.is_empty() && back.preflight.is_empty());
    }

    #[test]
    fn attempt_commands_parse_and_bad_ones_are_refused() {
        let parse = |s: &str| AppCommand::parse(&serde_json::from_str(s).unwrap());
        use AppCommand::*;
        assert_eq!(
            parse(r#"{"v":2,"cmd":"list_challenges","req":"c1"}"#),
            Some(Ok(ListChallenges { req: "c1".into() }))
        );
        assert_eq!(
            parse(r#"{"v":2,"cmd":"briefing","req":"b1","key":"ltfj-06-v1"}"#),
            Some(Ok(Briefing {
                req: "b1".into(),
                key: "ltfj-06-v1".into()
            }))
        );
        assert_eq!(
            parse(r#"{"v":2,"cmd":"place","key":"ltfj-06-v1","practice":true}"#),
            Some(Ok(Place {
                key: "ltfj-06-v1".into(),
                practice: true
            }))
        );
        assert_eq!(
            parse(r#"{"v":2,"cmd":"place","key":"ltfj-06-v1"}"#),
            Some(Ok(Place {
                key: "ltfj-06-v1".into(),
                practice: false
            }))
        );
        let id = || "3f0e7f8e-6c2b-4d55-9a57-1b1f2a8d0c11".to_owned();
        assert_eq!(
            parse(r#"{"v":2,"cmd":"fly_again","id":"3f0e7f8e-6c2b-4d55-9a57-1b1f2a8d0c11"}"#),
            Some(Ok(FlyAgain { id: id() }))
        );
        assert_eq!(
            parse(r#"{"v":2,"cmd":"finish","id":"3f0e7f8e-6c2b-4d55-9a57-1b1f2a8d0c11"}"#),
            Some(Ok(Finish { id: id() }))
        );
        assert_eq!(
            parse(r#"{"v":2,"cmd":"abandon","id":"3f0e7f8e-6c2b-4d55-9a57-1b1f2a8d0c11"}"#),
            Some(Ok(Abandon { id: id() }))
        );
        for bad in [
            r#"{"v":2,"cmd":"fly_again"}"#,
            r#"{"v":2,"cmd":"finish","id":""}"#,
            r#"{"v":2,"cmd":"abandon","id":"../x"}"#,
        ] {
            assert_eq!(parse(bad), Some(Err("no such attempt")), "{bad}");
        }
        assert_eq!(parse(r#"{"v":2,"cmd":"end_flight"}"#), None, "not ours");
        assert_eq!(
            parse(r#"{"v":1,"cmd":"finish"}"#),
            Some(Err("unsupported version"))
        );
        for bad in [
            r#"{"v":2,"cmd":"place"}"#,
            r#"{"v":2,"cmd":"place","key":"../../x"}"#,
            r#"{"v":2,"cmd":"briefing","req":"b","key":"LTFJ"}"#,
        ] {
            assert_eq!(parse(bad), Some(Err("no such challenge")), "{bad}");
        }
        let long = format!(
            r#"{{"v":2,"cmd":"list_challenges","req":"{}"}}"#,
            "r".repeat(300)
        );
        assert_eq!(parse(&long), Some(Err("request too long")));
        assert!(
            Place {
                key: String::new(),
                practice: false
            }
            .name()
                == "place"
        );
        assert!(
            ListChallenges { req: String::new() }.is_query() && !FlyAgain { id: id() }.is_query()
        );
    }

    #[test]
    fn clip_cuts_on_characters() {
        assert_eq!(clip("abc", 3), "abc");
        assert_eq!(clip("abcd", 3), "ab…");
        assert_eq!(clip("ééé", 2), "é…");
        assert_eq!(clip("", 3), "");
    }
}
