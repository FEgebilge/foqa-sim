//! Grading one recorded attempt: is it valid (the checks), and its score.
//! Everything comes from the flight, so the server can grade the uploaded
//! recording itself and never trust a client's number.

use analyzer::{locate, wind_components};
use debrief::Analysis;
use fdm_core::SimTime;
use fdm_core::geo::distance_m;
use rules::Aircraft;
use serde::Serialize;

use crate::score::{Landing, Score, landing_1};
use crate::{Challenge, M_PER_NM};

/// The rollout ends below this groundspeed.
const TAXI_KT: f64 = 30.0;
const KT_MS: f64 = 1852.0 / 3600.0;
/// Longest gap between two samples of a continuous recording, s.
const MAX_GAP_S: f64 = 1.5;
/// Slack on the distance flown between two samples (position noise), m.
const JUMP_SLACK_M: f64 = 50.0;
/// Ground speed assumed where it is missing: above any challenge aircraft
/// near a runway, so a dropout cannot hide a jump.
const MAX_GS_KT: f64 = 600.0;

#[derive(Debug, Clone, Serialize)]
pub struct Attempt {
    pub challenge: String,
    pub challenge_version: u32,
    /// Every check passed: the attempt counts.
    pub valid: bool,
    pub checks: Vec<Check>,
    /// Sim time of the gate and of touchdown, s.
    pub gate_s: Option<f64>,
    pub touchdown_s: Option<f64>,
    /// `None` without a landing on the challenge's runway past its gate.
    pub score: Option<Score>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Check {
    pub name: &'static str,
    pub ok: bool,
    pub detail: String,
}

fn check(name: &'static str, ok: bool, detail: String) -> Check {
    Check { name, ok, detail }
}

/// The landing that was graded.
struct Graded {
    gate: SimTime,
    touchdown: SimTime,
    score: Score,
}

/// Grades the analysed flight as an attempt at `challenge`.
pub fn evaluate(challenge: &Challenge, a: &Analysis) -> Attempt {
    let mut checks = vec![aircraft_check(challenge, a)];
    let graded = landing(challenge, a, &mut checks);
    Attempt {
        challenge: challenge.id.clone(),
        challenge_version: challenge.version,
        valid: graded.is_some() && checks.iter().all(|k| k.ok),
        checks,
        gate_s: graded.as_ref().map(|g| g.gate.as_secs_f64()),
        touchdown_s: graded.as_ref().map(|g| g.touchdown.as_secs_f64()),
        score: graded.map(|g| g.score),
    }
}

/// Whether the flight's aircraft is one the challenge allows; also the
/// app's pre-flight check on the flight being recorded.
pub fn aircraft_check(c: &Challenge, a: &Analysis) -> Check {
    let (class, icao) = (a.selection.class, a.selection.icao.as_deref());
    let rule = &c.aircraft;
    let class_ok = rule.classes.is_empty() || class.is_some_and(|k| rule.classes.contains(&k));
    let type_ok = rule.types.is_empty()
        || icao.is_some_and(|t| rule.types.iter().any(|x| x.eq_ignore_ascii_case(t)));
    let class = class.map_or("unknown class".to_owned(), |k| k.to_string());
    let detail = format!("{} ({class})", icao.unwrap_or("unknown type"));
    check("aircraft", class_ok && type_ok, detail)
}

/// Finds the landing, adds the checks on it and scores it.
fn landing(c: &Challenge, a: &Analysis, checks: &mut Vec<Check>) -> Option<Graded> {
    let (flight, track) = (&a.flight, &a.track);
    let on_runway = |icao: &str, runway: &str| {
        icao.eq_ignore_ascii_case(&c.airport) && runway.eq_ignore_ascii_case(&c.runway)
    };
    let found = a.touchdowns.iter().enumerate().find(|(_, td)| {
        !td.bounce
            && td
                .runway
                .as_ref()
                .is_some_and(|f| on_runway(&f.icao, &f.runway))
    });
    let named = format!("{} {}", c.airport, c.runway);
    checks.push(match found {
        Some(_) => check("runway", true, format!("landed on {named}")),
        None => check("runway", false, format!("no landing on {named}")),
    });
    let (k, td) = found?;
    let fix = td.runway.as_ref()?;
    let airports = flight.airports();
    let (threshold, _) = airports.iter().find_map(|ap| c.threshold(ap))?;

    let at = |name: &str, i: usize| flight.at(name, track.t[i]);
    let pos = |i: usize| Some((at("pos.lat", i)?, at("pos.lon", i)?));
    let td_i = track.index_at(td.t);
    let end_i = (td_i..track.len())
        .find(|&i| track.gs_kt[i] < TAXI_KT)
        .unwrap_or(track.len() - 1);

    // The gate: the last time before touchdown at its distance or further.
    let gate_m = c.gate.distance_nm * M_PER_NM;
    let threshold_pos = (threshold.lat_deg, threshold.lon_deg);
    let gate_i = (0..td_i)
        .rev()
        .find(|&i| pos(i).is_some_and(|p| distance_m(p, threshold_pos) >= gate_m));
    let airborne = gate_i.is_some_and(|g| !track.on_ground[g..td_i].contains(&true));
    checks.push(match gate_i {
        None => check(
            "gate",
            false,
            format!("never {} NM out before touchdown", c.gate.distance_nm),
        ),
        Some(_) if !airborne => check("gate", false, "on the ground after the gate".into()),
        Some(g) => check(
            "gate",
            true,
            format!("{} NM out at {:.0} ft", c.gate.distance_nm, track.aal_ft[g]),
        ),
    });
    let gate_i = gate_i.filter(|_| airborne)?;
    let (from, to) = (track.t[gate_i], track.t[end_i]);

    // Integrity, from the gate to the end of the rollout.
    let values = |name: &str| -> Vec<f64> {
        let inside = flight.series(name).iter();
        let inside = inside.filter(|s| (from..=to).contains(&s.t) && s.is_valid());
        flight
            .at(name, from)
            .into_iter()
            .chain(inside.map(|s| s.value))
            .collect()
    };
    let rate = values("sim.rate");
    checks.push(check(
        "sim_rate",
        !rate.is_empty() && rate.iter().all(|r| (r - 1.0).abs() <= 0.01),
        match rate.iter().copied().reduce(f64::max) {
            Some(max) => format!("up to {max}x"),
            None => "not recorded".into(),
        },
    ));
    for (param, name) in [("sim.slew", "slew"), ("sim.paused", "pause")] {
        let on = values(param).iter().any(|&v| v >= 0.5);
        check_flag(checks, name, on);
    }
    checks.push(continuity(a, &pos, gate_i, end_i));

    // Conditions.
    if let Some(w) = &c.conditions.wind {
        let (mut east, mut north, mut n) = (0.0, 0.0, 0.0);
        for i in gate_i..=td_i {
            if let (Some(speed), Some(dir)) = (at("wind.speed", i), at("wind.dir", i)) {
                let (sin, cos) = dir.to_radians().sin_cos();
                (east, north, n) = (east + speed * sin, north + speed * cos, n + 1.0);
            }
        }
        let (speed, dir) = (
            east.hypot(north) / n,
            east.atan2(north).to_degrees().rem_euclid(360.0),
        );
        let ok = n > 0.0
            && (speed - w.speed_kt).abs() <= w.tolerance_kt
            && (w.speed_kt <= w.tolerance_kt || angle_between(dir, w.from_deg) <= w.tolerance_deg);
        checks.push(check(
            "wind",
            ok,
            format!(
                "mean {dir:03.0}° {speed:.0} kt, wanted {:03.0}° ±{}°, {} ±{} kt",
                w.from_deg, w.tolerance_deg, w.speed_kt, w.tolerance_kt
            ),
        ));
    }
    let wind = flight
        .at("wind.speed", td.t)
        .zip(flight.at("wind.dir", td.t))
        .map(|(s, d)| wind_components(s, d, fix.heading_deg));
    let components = [
        (
            "crosswind",
            c.conditions.crosswind_kt,
            wind.map(|(_, x)| x.abs()),
        ),
        ("tailwind", c.conditions.tailwind_kt, wind.map(|(h, _)| -h)),
    ];
    for (name, band, value) in components {
        if let Some(band) = band {
            let detail = value.map_or("no wind recorded".into(), |v| {
                format!("{v:.0} kt at touchdown")
            });
            checks.push(check(name, value.is_some_and(|v| band.contains(v)), detail));
        }
    }

    // Rules.
    if let Some(height) = c.rules.ap_off_by_height_ft {
        let on = (gate_i..td_i)
            .find(|&i| track.aal_ft[i] <= height && at("ap.engaged", i).is_some_and(|v| v >= 0.5));
        checks.push(match on {
            None => check("autopilot", true, format!("off below {height} ft")),
            Some(i) => check(
                "autopilot",
                false,
                format!("engaged at {:.0} ft", track.aal_ft[i]),
            ),
        });
    }
    // Centreline offset at each located point of the rollout; `None` once
    // one is off the runway. A point without a position is skipped: a
    // dropout is not leaving the runway.
    let cross = |(lat, lon): (f64, f64)| {
        locate(&airports, lat, lon, fix.heading_deg)
            .filter(|f| on_runway(&f.icao, &f.runway))
            .map(|f| f.cross_m.abs())
    };
    let rollout: Option<Vec<f64>> = (td_i..=end_i).filter_map(pos).map(cross).collect();
    if c.rules.stop_on_runway {
        let stopped = track.gs_kt[end_i] < TAXI_KT;
        checks.push(check(
            "stop_on_runway",
            stopped && rollout.is_some(),
            match (stopped, rollout.is_some()) {
                (false, _) => "did not slow to taxi speed".into(),
                (true, false) => "left the runway".into(),
                (true, true) => format!("below {TAXI_KT} kt on the runway"),
            },
        ));
    }

    let score = landing_1(&Landing {
        class: a.selection.class.unwrap_or(Aircraft::Transport),
        td,
        fix,
        bounced: a.touchdowns.get(k + 1).is_some_and(|t| t.bounce),
        worst: a
            .events
            .iter()
            .filter(|e| (from..td.t).contains(&e.start))
            .map(|e| e.level)
            .max(),
        rollout_cross_m: rollout.map(|xs| xs.into_iter().fold(0.0, f64::max)),
    });
    Some(Graded {
        gate: from,
        touchdown: td.t,
        score,
    })
}

fn check_flag(checks: &mut Vec<Check>, name: &'static str, on: bool) {
    let detail = if on {
        format!("{name} used")
    } else {
        format!("no {name}")
    };
    checks.push(check(name, !on, detail));
}

/// No gap in the samples and no move further than the speed allows: no
/// teleport, reload or replay inside the attempt.
fn continuity(
    a: &Analysis,
    pos: &impl Fn(usize) -> Option<(f64, f64)>,
    from: usize,
    to: usize,
) -> Check {
    let t = &a.track.t;
    for i in from..to {
        let dt = t[i + 1].as_secs_f64() - t[i].as_secs_f64();
        if dt > MAX_GAP_S {
            return check("continuity", false, format!("{dt:.1} s without data"));
        }
        // `max` takes the known one; with neither, `MAX_GS_KT`.
        let gs = a.track.gs_kt[i].max(a.track.gs_kt[i + 1]);
        let fastest = if gs.is_finite() { gs } else { MAX_GS_KT } * KT_MS;
        if let (Some(p), Some(q)) = (pos(i), pos(i + 1)) {
            let moved = distance_m(p, q);
            if moved > fastest * dt * 1.5 + JUMP_SLACK_M {
                return check(
                    "continuity",
                    false,
                    format!("jumped {moved:.0} m in {dt:.1} s"),
                );
            }
        }
    }
    check("continuity", true, "continuous".into())
}

/// The smaller angle between two directions, deg.
fn angle_between(a: f64, b: f64) -> f64 {
    ((a - b + 540.0) % 360.0 - 180.0).abs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn angles_wrap_around_north() {
        assert_eq!(angle_between(350.0, 10.0), 20.0);
        assert_eq!(angle_between(10.0, 350.0), 20.0);
        assert_eq!(angle_between(90.0, 270.0), 180.0);
    }
}
