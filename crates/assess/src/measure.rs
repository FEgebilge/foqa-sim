//! From an analysed flight to the measures of one landing: the approach
//! window's series, the touchdown and rollout values, the findings that
//! count toward it and the reference speeds (docs/scoring.md §2.5, §5).
//! Each criterion id of the method is measured here.

use analyzer::{Approach, Flight, Phase, Touchdown, Track, held, phases, wind_components};
use fdm_core::SimTime;
use rules::{Aircraft, Domain, Event, RefSpeeds, Rule, ScoringClass, Selection};
use serde::Serialize;

use crate::method::{Context, Height, Method};
use crate::score::{Finding, LandingScore, Measures, Outcome, Value};

const FT_PER_M: f64 = 1.0 / 0.3048;
/// Descent rate on a 3 deg path per knot of ground speed: tan 3 deg x
/// 6076 ft / 60 min.
const ROD3_FPM_PER_KT: f64 = 5.31;
/// Threshold crossing height of the glidepath (S8; ICAO Annex 10 ILS
/// reference datum height 15 m), at the eye or the antenna.
const TCH_FT: f64 = 50.0;
/// The glidepath to the touchdown target: 3 deg (ICAO PANS-OPS, S8).
const GLIDEPATH_DEG: f64 = 3.0;
/// A light aircraft aims at the Annex 14 aiming point of runways under
/// 800 m (Vol I Table 5-1, 150 m), the code 1 runways its field length is
/// laid out for, on any runway: the airline aiming point (up to 400 m) of a
/// long runway is no touchdown target for it.
const LIGHT_AIM_M: f64 = 150.0;
/// A longer stretch without data in the window makes the score partial.
const MAX_GAP_US: i64 = 1_500_000;
/// Gear counts as down from here, %.
const GEAR_DOWN_PCT: f64 = 99.0;
/// Light aircraft: a flap change below this height is late (est).
const LIGHT_LATE_FLAP_FT: f64 = 200.0;
/// A bounce is a large one past these (est).
const BIG_BOUNCE_US: i64 = 2_000_000;
const BIG_BOUNCE_FT: f64 = 5.0;

/// The landing to assess.
pub struct Landing<'a> {
    pub flight: &'a Flight,
    pub track: &'a Track,
    pub selection: &'a Selection<'a>,
    pub touchdowns: &'a [Touchdown],
    /// The touchdown that starts the landing.
    pub index: usize,
    pub events: &'a [Event],
    /// The rules the events come from, for their domains.
    pub rules: &'a [Rule],
    /// A ranked challenge attempt: no selected-speed fallback (Decision 1).
    pub ranked: bool,
    /// A challenge gate lower than the window's top, ft above the runway.
    pub gate_ft: Option<f64>,
    /// A challenge's touchdown target past the threshold, m; else the
    /// aiming point.
    pub target_m: Option<f64>,
}

/// Where the reference speeds came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SpeedSource {
    /// The type profile (Vref table, POH speeds or 1.3 VSO).
    Profile,
    /// The recorded selected speed, a labelled debrief-only fallback.
    Selected,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Speeds {
    pub vref_kt: f64,
    pub vapp_kt: f64,
    pub source: SpeedSource,
    pub approximate: bool,
    /// The document or the recorded value, in words.
    pub basis: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum Assessment {
    Scored {
        score: LandingScore,
        speeds: Option<Speeds>,
        /// `false` for a touch-and-go, whose rollout is not scored.
        full_stop: bool,
    },
    /// Data any score needs is missing: findings only.
    Insufficient { missing: Vec<&'static str> },
}

impl Method {
    /// The landing's assessment.
    pub fn assess(&self, l: &Landing) -> Assessment {
        match self.measures(l) {
            Ok((m, speeds, full_stop)) => Assessment::Scored {
                score: self.score(&m),
                speeds,
                full_stop,
            },
            Err(missing) => Assessment::Insufficient { missing },
        }
    }

    /// Vref and the approach speed (docs/scoring.md §5): the profile's,
    /// with the wind additive where it gives a Vref only; else, outside a
    /// ranked attempt, the selected speed (Vref 5 kt under it).
    pub fn speeds(
        &self,
        profile: Option<RefSpeeds>,
        selected_kt: Option<f64>,
        headwind_kt: Option<f64>,
        autothrottle: bool,
        ranked: bool,
    ) -> Option<Speeds> {
        let a = &self.additive;
        if let Some(p) = profile {
            let additive = if autothrottle {
                a.autothrottle_kt
            } else {
                (headwind_kt.unwrap_or(0.0) / 2.0).clamp(a.min_kt, a.max_kt)
            };
            return Some(Speeds {
                vapp_kt: p.vapp.unwrap_or(p.vref + additive),
                vref_kt: p.vref,
                source: SpeedSource::Profile,
                approximate: p.approximate,
                basis: p.source,
            });
        }
        let selected = selected_kt.filter(|_| !ranked)?;
        Some(Speeds {
            vref_kt: selected - a.min_kt,
            vapp_kt: selected,
            source: SpeedSource::Selected,
            approximate: true,
            basis: format!("selected speed {selected:.0} kt (debrief only)"),
        })
    }

    /// The top and bottom of the landing's approach window, ft above the
    /// runway: its class's, the top lowered to a challenge gate.
    pub fn window_ft(&self, l: &Landing) -> (f64, f64) {
        let window = self.windows[&scoring_class(l.selection)];
        let top = l.gate_ft.map_or(window.top_ft, |g| g.min(window.top_ft));
        (top, window.bottom_ft.unwrap_or(l.selection.phases.flare_ft))
    }

    /// The measures of the landing, its speeds and whether it was a full
    /// stop; the missing required data otherwise.
    #[allow(clippy::type_complexity)]
    pub fn measures(
        &self,
        l: &Landing,
    ) -> Result<(Measures, Option<Speeds>, bool), Vec<&'static str>> {
        let (flight, track) = (l.flight, l.track);
        let td = &l.touchdowns[l.index];
        let class = scoring_class(l.selection);
        let (top, bottom) = self.window_ft(l);
        let a = Approach::of(flight, track, td, top, bottom);
        // Slowed to taxi speed on the ground, or still rolling where the
        // recording ends; a liftoff first is a touch-and-go.
        let full_stop = a.slowed.is_some() || a.liftoff.is_none();
        let mut missing = Vec::new();
        let fix = td.runway.as_ref();
        if fix.is_none() {
            missing.push("runway");
        }
        if !td.peak_g.is_finite() {
            missing.push("peak_g");
        }
        if !td.sink_fpm.is_finite() {
            missing.push("sink");
        }
        if full_stop && fix.is_some() && a.rollout_cross_m.is_none() {
            missing.push("rollout position");
        }
        let Some(fix) = fix.filter(|_| missing.is_empty()) else {
            return Err(missing);
        };

        let w = &a.window;
        let mut partial = Vec::new();
        let at = |name: &str, t: Option<&SimTime>| t.and_then(|&t| flight.at(name, t));
        let below_500 = w.height_ft.iter().position(|&h| h < 500.0);
        let landing_flap = below_500
            .or(w.t.len().checked_sub(1))
            .and_then(|i| at("flap.te_angle", w.t.get(i)));
        let profile = l
            .selection
            .profile
            .and_then(|p| p.ref_speeds(at("weight.gross", w.t.first()), landing_flap));
        let selected = median(
            w.t.iter()
                .filter_map(|&t| flight.at("sel.speed", t))
                .filter(|&s| s > 0.0),
        );
        let headwind = mean(w.t.iter().filter_map(|&t| {
            let (speed, from) = (flight.at("wind.speed", t)?, flight.at("wind.dir", t)?);
            Some(wind_components(speed, from, fix.heading_deg).0)
        }));
        let autothrottle = at("at.engaged", w.t.first()).is_some_and(|v| v >= 0.5);
        let speeds = self.speeds(profile, selected, headwind, autothrottle, l.ranked);
        if speeds.is_none() {
            let icao = l.selection.icao.as_deref().unwrap_or("this aircraft");
            partial.push(format!(
                "No reference speed (Vref) for {icao}, so speed items were not graded"
            ));
        }
        let light = class == ScoringClass::Light;
        let reference = speeds
            .as_ref()
            .map(|s| if light { s.vapp_kt } else { s.vref_kt });
        let profile_value = |f| l.selection.profile.and_then(|p| p.value(f, || None));
        let track_ft = profile_value(rules::ProfileField::MainGearTrack)
            .unwrap_or(self.fallback_track_ft[&class]);
        let aim_m = if light {
            LIGHT_AIM_M
        } else {
            fix.aiming_point_m
        };
        let target_m = l.target_m.unwrap_or(aim_m);
        // The glidepath to the target at the threshold, at the wheels.
        let path_ft = target_m * FT_PER_M * GLIDEPATH_DEG.to_radians().tan();
        let tch_ft = path_ft.min(TCH_FT) - eye_to_wheel_ft(class);
        let context = Context {
            add_kt: speeds.as_ref().map(|s| s.vapp_kt - s.vref_kt),
            gust_kt: Some(0.0),
            rod3_fpm: mean(w.gs_kt.iter().copied().filter(|g| g.is_finite()))
                .map(|g| ROD3_FPM_PER_KT * g),
            target_ft: Some(target_m * FT_PER_M),
            tdz_ft: Some(fix.tdz_m * FT_PER_M),
            lda_ft: Some(fix.lda_m * FT_PER_M),
            width_ft: Some(fix.width_m * FT_PER_M),
            track_ft: Some(track_ft),
            hard_g: profile_value(rules::ProfileField::HardLandingG),
        };

        let mut values: Vec<(String, Value)> = Vec::new();
        // A gap in a recorded series makes its criterion n/a; the ILS
        // deviations are unknown without a signal, n/a by design.
        let mut series = |id: &str, x: Vec<f64>, recorded: bool| {
            let Some(c) = self.criterion(id) else { return };
            if x.iter().all(|v| v.is_nan()) {
                return;
            }
            if recorded && gap(&w.t, &x) {
                partial.push(format!(
                    "{} was not graded: the recording has a gap of over 1.5 s in the approach",
                    c.title
                ));
                return;
            }
            let neg: Vec<f64> = x.iter().map(|v| -v).collect();
            let low = held(&w.t, &neg, c.sustain_us)
                .into_iter()
                .map(|v| -v)
                .collect();
            let mut high = held(&w.t, &x, c.sustain_us);
            if let Some((beyond, sustain_us)) = c.beyond {
                for (h, s) in high.iter_mut().zip(held(&w.t, &x, sustain_us)) {
                    if s >= beyond && (h.is_nan() || s > *h) {
                        *h = s;
                    }
                }
            }
            values.push((id.to_owned(), Value::Series { high, low }));
        };
        if let Some(r) = reference {
            series("A1", w.ias_kt.iter().map(|v| v - r).collect(), true);
        }
        series("A2", w.sink_fpm.clone(), true);
        series("A3", w.roll_deg.iter().map(|v| v.abs()).collect(), true);
        series("A4", w.pitch_deg.clone(), true);
        series("A5G", w.gs_dev_dot.iter().map(|v| v.abs()).collect(), false);
        series(
            "A5L",
            w.loc_dev_dot.iter().map(|v| v.abs()).collect(),
            false,
        );

        let mut crossing = a.threshold.as_ref();
        if crossing.is_some_and(|c| track.ra_ft[track.index_at(c.t)].is_nan()) {
            partial.push(
                "Threshold crossing height was not graded: no radio altitude at the threshold"
                    .into(),
            );
            crossing = None;
        }
        let tail = profile_value(rules::ProfileField::TailLandingPitch);
        let scalars = [
            (
                "A6",
                configuration(&a, light, self.windows[&class].split_ft),
            ),
            ("B1", crossing.map(|c| c.height_ft - tch_ft)),
            ("B2", td.ias_kt.zip(reference).map(|(ias, r)| ias - r)),
            ("B3", a.float_s),
            ("C1", Some((fix.along_m - target_m) * FT_PER_M)),
            ("D1", Some(fix.cross_m.abs() * FT_PER_M)),
            ("D2", a.crab_deg.map(f64::abs)),
            ("D3", a.drift_kt.map(f64::abs)),
            (
                "D4",
                a.rollout_cross_m
                    .filter(|_| full_stop)
                    .map(|m| m * FT_PER_M),
            ),
            ("E1", Some(td.peak_g)),
            ("E2", Some(td.sink_fpm)),
            ("E3", Some(bounce(track, l.touchdowns, l.index))),
            ("E4", tail.zip(td.pitch_deg).map(|(tail, p)| tail - p)),
            ("E4N", td.pitch_deg),
            ("E5", td.roll_deg.map(f64::abs)),
            ("F1", a.decel_delay_s.filter(|_| full_stop)),
            ("F2", a.required_decel_g.filter(|_| full_stop)),
        ];
        values.extend(scalars.map(|(id, x)| (id.to_owned(), Value::Scalar(Height::All, x))));

        let (from, to) = landing_span(l, &a, full_stop);
        let crashed = flight.crashes().any(|t| (from..=to).contains(&t));
        let outcome = Outcome {
            undershoot: fix.short_landing,
            excursion: if crashed {
                Some("The sim reported a crash".into())
            } else {
                full_stop.then(|| excursion(&a, fix)).flatten()
            },
        };
        let findings = l
            .events
            .iter()
            .enumerate()
            .filter(|(_, e)| (from..=to).contains(&e.start))
            .filter_map(|(i, e)| {
                let rule = l.rules.iter().find(|r| r.id == e.rule)?;
                let domain = rule.domain;
                (domain != Domain::Other).then(|| Finding {
                    rule: e.rule.clone(),
                    title: rule.title.clone(),
                    domain,
                    level: e.level,
                    height_ft: Some(track.aal_ft[track.index_at(e.peak)]).filter(|h| h.is_finite()),
                    event: Some(i),
                })
            })
            .collect();
        let measures = Measures {
            class,
            context,
            heights_ft: w.height_ft.clone(),
            values,
            findings,
            outcome,
            partial,
        };
        Ok((measures, speeds, full_stop))
    }
}

/// The profile's scoring class, else the default of the rule class
/// (transport where unknown, as the rules grade it).
pub fn scoring_class(s: &Selection) -> ScoringClass {
    s.profile.map_or_else(
        || ScoringClass::of(s.class.unwrap_or(Aircraft::Transport)),
        |p| p.scoring_class,
    )
}

/// Eye to main-wheel height on the glidepath, from the FAA Order 8260.3
/// height groups: HG1 up to 10 ft (general aviation, commuters), HG2 10-15
/// ft (737, A320); the class value is an estimate within them. The radio
/// height B1 reads is the wheels' (`Track::ra_ft`).
fn eye_to_wheel_ft(class: ScoringClass) -> f64 {
    match class {
        ScoringClass::Light => 6.0,
        ScoringClass::Turboprop => 10.0,
        ScoringClass::Jet => 15.0,
    }
}

fn mean(xs: impl Iterator<Item = f64>) -> Option<f64> {
    let (sum, n) = xs.fold((0.0, 0.0), |(s, n), x| (s + x, n + 1.0));
    (n > 0.0).then(|| sum / n)
}

fn median(xs: impl Iterator<Item = f64>) -> Option<f64> {
    let mut v: Vec<f64> = xs.collect();
    v.sort_by(f64::total_cmp);
    v.get(v.len() / 2).copied()
}

/// Whether `x` has a stretch without data longer than `MAX_GAP_US`.
fn gap(t: &[SimTime], x: &[f64]) -> bool {
    analyzer::runs(x.len(), |i| x[i].is_nan()).any(|r| {
        let (a, b) = (r.start.saturating_sub(1), r.end.min(t.len() - 1));
        t[b].us_since(t[a]) > MAX_GAP_US
    })
}

/// A6 as a level: a jet's flap or gear change at or above the split L2,
/// below it L3, gear not down at the bottom of the window L3 (Data4Safety
/// late flap and gear, SL2A / SL3); a light
/// aircraft's flap change under 200 ft L1, gear not down below its split L3
/// (est; fixed gear reads 100 %). `None` without flap or gear data.
fn configuration(a: &Approach, light: bool, split: f64) -> Option<f64> {
    let w = &a.window;
    let known = |v: &[f64]| v.iter().any(|x| x.is_finite());
    if !known(&w.flap_deg) && !known(&w.gear_pct) {
        return None;
    }
    let changes = |what| a.config_changes.iter().filter(move |c| c.what == what);
    let level = if light {
        let gear_up = w
            .gear_pct
            .iter()
            .zip(&w.height_ft)
            .any(|(g, h)| *g < GEAR_DOWN_PCT && *h < split);
        let late = changes(analyzer::Config::Flaps)
            .filter(|c| c.height_ft < LIGHT_LATE_FLAP_FT)
            .map(|_| 1.0);
        late.chain(gear_up.then_some(3.0)).fold(0.0, f64::max)
    } else {
        let not_down = w
            .gear_pct
            .iter()
            .rfind(|g| g.is_finite())
            .is_some_and(|&g| g < GEAR_DOWN_PCT);
        let late = changes(analyzer::Config::Flaps)
            .chain(changes(analyzer::Config::Gear))
            .map(|c| if c.height_ft >= split { 2.0 } else { 3.0 });
        late.chain(not_down.then_some(3.0)).fold(0.0, f64::max)
    };
    Some(level)
}

/// E3 as a level: 1 for a bounce after this touchdown, 2 when it was
/// airborne over 2 s or rose over 5 ft (est); 0 without one.
fn bounce(track: &Track, tds: &[Touchdown], index: usize) -> f64 {
    let Some(next) = tds.get(index + 1).filter(|t| t.bounce) else {
        return 0.0;
    };
    let hop = track.index_at(tds[index].t)..track.index_at(next.t);
    let airborne = hop.clone().filter(|&i| !track.on_ground[i]).count() as i64 * Track::STEP_US;
    let rose = hop
        .map(|i| track.ra_ft[i])
        .filter(|h| h.is_finite())
        .fold(0.0, f64::max);
    if airborne > BIG_BOUNCE_US || rose > BIG_BOUNCE_FT {
        2.0
    } else {
        1.0
    }
}

/// Leaving the runway on the rollout, in words.
fn excursion(a: &Approach, fix: &analyzer::RunwayFix) -> Option<String> {
    if a.rollout_cross_m.is_some_and(|c| c > fix.width_m / 2.0) {
        return Some("Left the side of the runway on the landing roll".into());
    }
    a.slowed
        .as_ref()
        .filter(|s| s.along_m > fix.lda_m)
        .map(|_| "Overran the runway: still not at taxi speed at its far end".into())
}

/// The span whose events count toward the landing: from the start of its
/// approach phase to the end of the rollout (taxi speed), or to the next
/// takeoff of a touch-and-go.
fn landing_span(l: &Landing, a: &Approach, full_stop: bool) -> (SimTime, SimTime) {
    let td = l.touchdowns[l.index].t;
    let spans = phases(l.track, &l.selection.phases);
    let from = spans
        .iter()
        .rfind(|s| s.phase == Phase::Approach && s.start <= td)
        .map(|s| s.start)
        .or(a.window.t.first().copied())
        .unwrap_or(td);
    let takeoff = spans
        .iter()
        .find(|s| s.phase == Phase::Takeoff && s.start > td);
    let end = l.track.t.last().copied().unwrap_or(td);
    let to = match (&a.slowed, takeoff) {
        (Some(s), _) if full_stop => s.t,
        (_, Some(t)) => t.start,
        _ => end,
    };
    (from, to)
}
