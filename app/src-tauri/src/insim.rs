//! The in-sim views' challenge part (docs/challenge-protocol.md B9): the
//! attempt strip under the toolbar panel's board and in the EFB, the EFB's
//! Challenges tab (list, briefing with the pre-flight card), and the views'
//! Place / Fly again / Finish / Abandon, carried out as the app's own
//! buttons. Every word is written here; the views only render.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::thread;
use std::time::Duration;

use capture::{AppCommand, AttemptStrip, BriefingMsg, ChallengeCard, Landed, Local, SiteStrip};
use challenge::{Challenge, Mode};
use chrono::{DateTime, Utc};

use crate::attempts::{Record, State, View};
use crate::commands::Shared;

/// The strip is rebuilt (and an open attempt's gate looked for) this often,
/// and at once on an attempt's change; the board repeats it each second.
const EVERY: Duration = Duration::from_secs(5);
/// A finished attempt's strip is not shown again this long after.
const SHOWN_FOR_S: i64 = 10 * 60;
const SOURCE_FILE: &str = "File";

impl Shared {
    /// Runs the views' commands and keeps the strip up to date: woken by
    /// `changed` on every attempt change. Gives each a thread of its own.
    pub fn in_sim(self: &Arc<Self>, changed: Receiver<()>, commands: Option<Receiver<AppCommand>>) {
        if let Some(commands) = commands {
            let shared = Arc::clone(self);
            thread::spawn(move || {
                for cmd in commands {
                    let shared = Arc::clone(&shared);
                    thread::spawn(move || shared.view_command(&cmd));
                }
            });
        }
        let shared = Arc::clone(self);
        thread::spawn(move || {
            loop {
                shared.publish_strip();
                match changed.recv_timeout(EVERY) {
                    Ok(()) => while changed.try_recv().is_ok() {},
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
        });
    }

    /// The current attempt's strip to the views.
    fn publish_strip(&self) {
        let Some(view) = &self.attempts.current() else {
            return;
        };
        self.watch_gate(&view.record);
        let id = &view.record.id;
        let site = self.hooks.as_ref().and_then(|h| h.site_strip(id));
        let from_site = self.source(&view.record.key) != SOURCE_FILE;
        let (strip, done) = strip(view, site, from_site);
        let age_s = DateTime::parse_from_rfc3339(&view.record.updated_at)
            .map_or(0, |t| (Utc::now() - t.with_timezone(&Utc)).num_seconds());
        if !done || age_s < SHOWN_FOR_S {
            self.views.attempt(&strip, done);
        }
    }

    fn source(&self, key: &str) -> String {
        let site = self.hooks.as_ref().and_then(|h| h.source(key));
        site.unwrap_or_else(|| SOURCE_FILE.to_owned())
    }

    fn view_command(self: &Arc<Self>, cmd: &AppCommand) {
        let named = |id: &str, open: bool| {
            let views = self.attempts.list();
            named(&views, id, open)
                .map(|r| r.id.clone())
                .map_err(str::to_owned)
        };
        let result = match cmd {
            AppCommand::ListChallenges { req } => return self.views.challenges(req, &self.cards()),
            AppCommand::Briefing { req, key } => {
                return self.views.briefing(&self.briefing(req, key));
            }
            AppCommand::Place { key, practice } => self
                .library
                .get(key)
                .and_then(|(_, json)| self.start_attempt(key, &json, *practice))
                .map(drop),
            AppCommand::FlyAgain { id } => named(id, false)
                .and_then(|id| self.fly_again(&id))
                .map(drop),
            AppCommand::Finish { id } => named(id, true)
                .and_then(|id| self.finish(&id, "finish"))
                .map(drop),
            AppCommand::Abandon { id } => named(id, true)
                .and_then(|id| self.abandon(&id, "cancelled"))
                .map(drop),
        };
        self.views.ack(cmd, result);
    }

    /// The library as cards, the challenge flown last first.
    fn cards(&self) -> Vec<ChallengeCard> {
        let views = self.attempts.list();
        let mut cards: Vec<(Option<String>, ChallengeCard)> = self
            .library
            .list()
            .into_iter()
            .map(|e| {
                let last = views.iter().find(|v| v.record.key == e.key);
                let site = last.and_then(|v| self.hooks.as_ref()?.site_strip(&v.record.id));
                let from_site = self.source(&e.key) != SOURCE_FILE;
                let text = last.map(|v| strip(v, site.clone(), from_site).0.text);
                let card = ChallengeCard {
                    source: self.source(&e.key),
                    state: "open".into(),
                    last: text,
                    rank: site.and_then(|(s, _)| s.rank),
                    key: e.key,
                    title: e.title,
                    airport: e.airport,
                    runway: e.runway,
                };
                (last.map(|v| v.record.created_at.clone()), card)
            })
            .collect();
        cards.sort_by(|a, b| b.0.cmp(&a.0));
        cards.into_iter().map(|(_, c)| c).collect()
    }

    fn briefing(&self, req: &str, key: &str) -> BriefingMsg {
        let mut msg = BriefingMsg {
            v: 2,
            req: req.to_owned(),
            key: key.to_owned(),
            title: String::new(),
            lines: Vec::new(),
            preflight: Vec::new(),
            error: None,
        };
        match self.library.get(key) {
            Ok((c, _)) => {
                msg.title = c.title.clone();
                msg.lines = briefing_lines(&c, &self.source(key));
                msg.preflight = self.preflight_lines(key).unwrap_or_default();
            }
            Err(e) => msg.error = Some(e),
        }
        msg
    }
}

/// The open attempt, else the newest; `views` come newest first.
fn current(views: &[View]) -> Option<&View> {
    views.iter().find(|v| v.record.open()).or(views.first())
}

/// The attempt a view's command names by `id`: it must be the one the strip
/// shows, and still open for Finish and Abandon (`open`). Any other is
/// refused, so a press on a strip that is behind, or on both the toolbar
/// and the EFB, never acts on the attempt that followed.
fn named<'a>(views: &'a [View], id: &str, open: bool) -> Result<&'a Record, &'static str> {
    let r = &current(views)
        .ok_or("No attempt yet: Place one first.")?
        .record;
    if r.id != id {
        return Err("That attempt is no longer the current one.");
    }
    if open && !r.open() {
        return Err("No attempt is open.");
    }
    Ok(r)
}

/// The strip of attempt `view` with its site part, and whether nothing
/// more will happen to it. `from_site`: its challenge came from a site,
/// so without a site part it is practice.
pub fn strip(
    view: &View,
    site: Option<(SiteStrip, bool)>,
    from_site: bool,
) -> (AttemptStrip, bool) {
    let r = &view.record;
    let stage = match r.state {
        State::Created | State::Placing => "placing",
        State::Flying => "flying",
        State::Approach => "gate",
        State::Rollout => "rollout",
        State::Finished => "finished",
        State::GradedLocal | State::Sliced | State::Done => "graded",
        State::Abandoned => "abandoned",
        State::FailedStart => "failed_start",
    };
    let touchdown = view.log.iter().rev().find(|l| l["event"] == "touchdown");
    let landed = touchdown.and_then(|l| {
        Some(Landed {
            sink_fpm: l["sink_fpm"].as_f64()?,
            peak_g: l["peak_g"].as_f64().unwrap_or(f64::NAN),
        })
    });
    let local = view.result.as_ref().map(|a| Local {
        valid: a["valid"].as_bool().unwrap_or(false),
        score: a["score"]["total"].as_f64(),
    });
    let mut words = vec![format!("Attempt {}", r.n)];
    let landed_text = landed.as_ref().map_or("Landed".to_owned(), |l| {
        format!("Landed {:.0} fpm", l.sink_fpm)
    });
    match stage {
        "placing" => words.push("Placing…".into()),
        "flying" => words.push("Placed".into()),
        "gate" => words.push("Gate".into()),
        "rollout" | "finished" => words.push(landed_text),
        "abandoned" => words.push("Abandoned".into()),
        "failed_start" => words.push("Couldn't place".into()),
        _ => {
            if landed.is_some() {
                words.push(landed_text);
            }
            words.push(match &local {
                Some(Local {
                    score: Some(s),
                    valid: true,
                }) => format!("Graded {s:.1}"),
                Some(Local { score: Some(s), .. }) => format!("Graded {s:.1}, a rule not met"),
                _ => "No landing to grade".into(),
            });
        }
    }
    let (site, site_done) = match site {
        Some((s, done)) => (Some(s), done),
        None if from_site && !matches!(stage, "placing" | "failed_start") => {
            let practice = SiteStrip {
                state: "practice".into(),
                score: None,
                rank: None,
                reason: None,
            };
            (Some(practice), true)
        }
        None => (None, true),
    };
    if let Some(s) = &site
        && let Some(text) = site_text(s)
    {
        words.push(text);
    }
    let core_done = matches!(r.state, State::Done | State::Abandoned | State::FailedStart);
    let strip = AttemptStrip {
        v: 2,
        id: r.id.clone(),
        key: r.key.clone(),
        n: r.n,
        stage: stage.into(),
        landed: landed.filter(|l| l.peak_g.is_finite()),
        local,
        site,
        text: words.join(" · "),
        can_fly_again: !matches!(r.state, State::Created | State::Placing),
        can_finish: matches!(r.state, State::Flying | State::Approach | State::Rollout),
    };
    (strip, core_done && site_done)
}

/// The site part in a few words; nothing while the site waits for the flight.
fn site_text(s: &SiteStrip) -> Option<String> {
    let score = |s: Option<f64>| s.map_or("Scored".to_owned(), |t| format!("Scored {t:.1}"));
    Some(match s.state.as_str() {
        "practice" => "Practice".into(),
        "waiting" => return None,
        "uploading" => "Uploading…".into(),
        "scoring" => "Scoring…".into(),
        "scored" => match s.rank {
            Some(rank) => format!("{}, #{rank}", score(s.score)),
            None => score(s.score),
        },
        "unverified" => format!("{}, not ranked", score(s.score)),
        "rejected" => "Rejected".into(),
        "expired" => "Not uploaded in time".into(),
        "signed_out" => "Signed out: sign in to upload".into(),
        _ => s
            .reason
            .clone()
            .unwrap_or_else(|| "Waiting for the site".into()),
    })
}

/// The briefing's facts in one line each.
pub fn briefing_lines(c: &Challenge, source: &str) -> Vec<String> {
    let s = &c.start;
    let placed = match s.mode {
        Mode::Teleport => "the app places you there",
        Mode::SelfFlown => "fly there yourself",
    };
    let gear = if s.gear_down { "down" } else { "up" };
    let mut lines = vec![
        format!(
            "Start: {} NM out, {:.0} ft, {:.0} kt, gear {gear}, flaps {:.0} %; {placed}.",
            s.distance_nm,
            s.height_ft,
            s.ias_kt,
            s.flaps * 100.0
        ),
        format!("Gate: {} NM; grading starts here.", c.gate.distance_nm),
    ];
    let rule = &c.aircraft;
    lines.push(if !rule.types.is_empty() {
        format!("Aircraft: {}.", rule.types.join(" or "))
    } else if !rule.classes.is_empty() {
        let classes: Vec<_> = rule.classes.iter().map(|k| k.to_string()).collect();
        format!("Aircraft: {}.", classes.join(" or "))
    } else {
        "Aircraft: any.".into()
    });
    let w = &c.conditions;
    if let Some(wind) = &w.wind {
        lines.push(format!(
            "Wind: {:03.0}° {:.0} kt (±{:.0}°, ±{:.0} kt), gate to touchdown.",
            wind.from_deg, wind.speed_kt, wind.tolerance_deg, wind.tolerance_kt
        ));
    }
    let band = |name: &str, b: &challenge::Band| {
        let (lo, hi) = (
            b.min.map(|v| format!("{v:.0}")),
            b.max.map(|v| format!("{v:.0}")),
        );
        match (lo, hi) {
            (Some(lo), Some(hi)) => format!("{name}: {lo} to {hi} kt at touchdown."),
            (Some(lo), None) => format!("{name}: at least {lo} kt at touchdown."),
            (None, Some(hi)) => format!("{name}: at most {hi} kt at touchdown."),
            (None, None) => format!("{name}: any."),
        }
    };
    if let Some(b) = &w.crosswind_kt {
        lines.push(band("Crosswind", b));
    }
    if let Some(b) = &w.tailwind_kt {
        lines.push(band("Tailwind", b));
    }
    let rules = &c.rules;
    let mut extra = Vec::new();
    if let Some(h) = rules.ap_off_by_height_ft {
        extra.push(format!("autopilot off below {h:.0} ft"));
    }
    if rules.stop_on_runway {
        extra.push("slow to taxi speed on the runway".to_owned());
    }
    lines.push(if extra.is_empty() {
        "Rules: integrity only (sim rate 1, no pause, no slew).".into()
    } else {
        format!(
            "Rules: {}; sim rate 1, no pause, no slew.",
            extra.join(", ")
        )
    });
    if let Some(window) = &c.window {
        lines.push(format!("Open: {} to {}.", window.from, window.to));
    }
    // Every challenge is graded with the current method, whatever it names.
    let named = &c.scoring.version;
    let scoring = if named == challenge::CURRENT {
        challenge::CURRENT.to_owned()
    } else {
        format!("{} (the challenge names {named})", challenge::CURRENT)
    };
    lines.push(format!("Scoring: {scoring}. Source: {source}."));
    lines
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::attempts::Record;

    fn view(state: State, log: Vec<serde_json::Value>, result: Option<serde_json::Value>) -> View {
        View {
            record: Record {
                id: "3f0e7f8e-6c2b-4d55-9a57-1b1f2a8d0c11".into(),
                key: "test-20-v1".into(),
                challenge: "test-20".into(),
                version: 1,
                title: "Runway 20".into(),
                definition_sha256: String::new(),
                n: 2,
                state,
                created_at: "2026-10-28T10:00:00.000Z".into(),
                updated_at: "2026-10-28T10:00:00.000Z".into(),
                flight_id: Some("flight-1".into()),
                marker_ref: None,
                begin_s: Some(100.0),
                touchdown_s: None,
                reason: None,
                slice_failed: None,
            },
            result,
            log,
        }
    }

    #[test]
    fn a_view_command_acts_only_on_the_attempt_its_strip_shows() {
        let flying = view(State::Flying, vec![], None);
        let id = flying.record.id.clone();
        let mut newer = view(State::Flying, vec![], None);
        newer.record.id = "a2".into();
        assert_eq!(
            named(&[], &id, false).unwrap_err(),
            "No attempt yet: Place one first."
        );
        assert_eq!(
            named(std::slice::from_ref(&flying), &id, true).unwrap().id,
            id
        );
        // Fly again was pressed twice (toolbar and EFB): the second names the
        // attempt the first replaced, and does nothing.
        let after = [newer, view(State::Abandoned, vec![], None)];
        assert_eq!(
            named(&after, &id, false).unwrap_err(),
            "That attempt is no longer the current one."
        );
        let done = [view(State::Done, vec![], None)];
        assert!(
            named(&done, &id, false).is_ok(),
            "Fly again after the grade"
        );
        assert_eq!(named(&done, &id, true).unwrap_err(), "No attempt is open.");
    }

    fn site(state: &str, score: Option<f64>, rank: Option<u32>) -> SiteStrip {
        SiteStrip {
            state: state.into(),
            score,
            rank,
            reason: None,
        }
    }

    fn touchdown() -> serde_json::Value {
        json!({ "event": "touchdown", "sink_fpm": 142.4, "peak_g": 1.18 })
    }

    #[test]
    fn the_strip_follows_the_attempt_from_placed_to_scored() {
        let text = |v: &View, s: Option<(SiteStrip, bool)>| strip(v, s, true).0.text;
        assert_eq!(
            text(&view(State::Placing, vec![], None), None),
            "Attempt 2 · Placing…"
        );
        let waiting = Some((site("waiting", None, None), false));
        let placed = strip(&view(State::Flying, vec![], None), waiting.clone(), true);
        assert_eq!(placed.0.text, "Attempt 2 · Placed");
        assert!(placed.0.can_finish && placed.0.can_fly_again && !placed.1);
        assert_eq!(
            text(&view(State::Approach, vec![], None), waiting.clone()),
            "Attempt 2 · Gate"
        );
        let rolling = view(State::Rollout, vec![touchdown()], None);
        let (s, done) = strip(&rolling, waiting, true);
        assert_eq!(
            (s.text.as_str(), s.stage.as_str()),
            ("Attempt 2 · Landed 142 fpm", "rollout")
        );
        assert_eq!(s.landed.map(|l| l.peak_g), Some(1.18));
        assert!(!done);
        let graded = view(
            State::Done,
            vec![touchdown()],
            Some(json!({ "valid": true, "score": { "total": 87.44 } })),
        );
        let uploading = Some((site("uploading", None, None), false));
        assert_eq!(
            text(&graded, uploading),
            "Attempt 2 · Landed 142 fpm · Graded 87.4 · Uploading…"
        );
        let scored = Some((site("scored", Some(87.4), Some(3)), true));
        let (s, done) = strip(&graded, scored, true);
        assert_eq!(
            s.text,
            "Attempt 2 · Landed 142 fpm · Graded 87.4 · Scored 87.4, #3"
        );
        assert!(done && !s.can_finish);
        let unverified = Some((site("unverified", Some(81.0), None), true));
        assert!(text(&graded, unverified).ends_with("Scored 81.0, not ranked"));
    }

    #[test]
    fn a_local_attempt_has_no_site_part_and_is_done_with_the_core() {
        let graded = view(
            State::Done,
            vec![],
            Some(json!({ "valid": false, "score": { "total": 40.0 } })),
        );
        let (s, done) = strip(&graded, None, false);
        assert_eq!(s.text, "Attempt 2 · Graded 40.0, a rule not met");
        assert!(s.site.is_none() && done);
        let (s, done) = strip(&view(State::GradedLocal, vec![], None), None, false);
        assert_eq!(s.text, "Attempt 2 · No landing to grade");
        assert!(!done, "still to slice");
        let (s, done) = strip(&view(State::Abandoned, vec![], None), None, true);
        assert_eq!(s.text, "Attempt 2 · Abandoned · Practice");
        assert!(done && s.can_fly_again);
        let (s, _) = strip(&view(State::Created, vec![], None), None, false);
        assert!(!s.can_fly_again, "placing: wait for it");
    }

    #[test]
    fn the_current_attempt_is_the_open_one_else_the_newest() {
        let newest = view(State::Done, vec![], None);
        let mut open = view(State::Flying, vec![], None);
        open.record.id = "open".into();
        let views = [newest, open];
        assert_eq!(current(&views).unwrap().record.id, "open");
        assert_eq!(current(&views[..1]).unwrap().record.state, State::Done);
        assert!(current(&[]).is_none());
    }

    #[test]
    fn briefing_lines_name_every_fact() {
        let c = Challenge::parse(
            r#"{ "id": "test-20", "version": 1, "title": "Runway 20", "airport": "TEST", "runway": "20",
              "aircraft": { "types": ["C172"] },
              "start": { "distance_nm": 8, "height_ft": 2500, "ias_kt": 160, "mode": "teleport" },
              "gate": { "distance_nm": 3 },
              "conditions": { "wind": { "from_deg": 60, "speed_kt": 15, "tolerance_deg": 20, "tolerance_kt": 5 },
                              "crosswind_kt": { "min": 12, "max": 20 } },
              "rules": { "ap_off_by_height_ft": 500, "stop_on_runway": true },
              "scoring": { "version": "landing-1" } }"#,
        )
        .unwrap();
        let lines = briefing_lines(&c, "File");
        assert_eq!(
            lines[0],
            "Start: 8 NM out, 2500 ft, 160 kt, gear up, flaps 0 %; the app places you there."
        );
        assert!(lines.contains(&"Aircraft: C172.".to_owned()));
        assert!(lines.contains(&"Wind: 060° 15 kt (±20°, ±5 kt), gate to touchdown.".to_owned()));
        assert!(lines.contains(&"Crosswind: 12 to 20 kt at touchdown.".to_owned()));
        assert!(
            lines
                .iter()
                .any(|l| l.contains("autopilot off below 500 ft"))
        );
        assert_eq!(
            lines.last().unwrap(),
            "Scoring: landing-3 (the challenge names landing-1). Source: File."
        );
    }
}
