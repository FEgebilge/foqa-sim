//! Flying skills (docs/skills.md): how a flight was flown, as measures
//! rather than a grade. Computed once from an `Analysis`; the debrief, the
//! flight summary and `fdm debrief` all take them from here.

use std::collections::BTreeMap;
use std::ops::Range;

use analyzer::{Flight, Phase, Span, TRACK_HZ, Touchdown, Track, runs};
use fdm_core::SimTime;
use rules::Aircraft as Class;
use serde::{Deserialize, Serialize};

use crate::Analysis;
use crate::trends::{r1, r2};

const G_MS2: f64 = 9.806_65;
/// Airborne time below which ride, fuel and control measures are left out.
const MIN_AIRBORNE_S: f64 = 60.0;
const KG_PER_LB: f64 = 0.453_592_37;
/// The comfort proxy removes the slow manoeuvring load with this moving mean.
const MOVING_MEAN_S: f64 = 2.0;
/// ISO 2631-1 Annex C: upper bounds (exclusive) of the lowest band that
/// contains the value; the bands themselves overlap.
const RIDE_BANDS: [(f64, &str); 5] = [
    (0.315, "not uncomfortable"),
    (0.63, "a little uncomfortable"),
    (1.0, "fairly uncomfortable"),
    (1.6, "uncomfortable"),
    (2.5, "very uncomfortable"),
];
const RIDE_BAND_TOP: &str = "extremely uncomfortable";
const SIDESLIP_MAX_AAL_FT: f64 = 10_000.0;
/// Below this the sideslip vane means nothing (a hop, a slew, a stall).
const SIDESLIP_MIN_IAS_KT: f64 = 40.0;
/// Fuel that rises by more than this between take-off and landing was added.
const REFUEL_KG: f64 = 1.0;
/// The top of descent is the last point this close to the flight's highest.
const TOP_TOLERANCE_FT: f64 = 100.0;
const LEVEL_FPM: f64 = 300.0;
const LEVEL_MIN_S: f64 = 10.0;
/// The descent counts down to this height above the landing runway.
const DESCENT_END_FT: f64 = 1000.0;
/// 0.5 % of full travel: `ctl.pitch` and `ctl.roll` run -100 to 100 %.
const REVERSAL_PCT: f64 = 1.0;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Skills {
    pub automation: Automation,
    pub ride: Ride,
    pub fuel: Fuel,
    pub control: Control,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Automation {
    /// Time in the air, lift-off to touchdown, summed over the legs.
    pub airborne_s: f64,
    pub ap_s: Option<f64>,
    pub at_s: Option<f64>,
    pub by_phase: Vec<PhaseAutomation>,
    /// Radio height of the last autopilot disconnect before the final
    /// touchdown; `None` for an autoland, which has no disconnect.
    pub ap_off_ft: Option<f64>,
    /// Hand-flown time from that disconnect to touchdown.
    pub manual_final_s: Option<f64>,
    /// The autopilot was still engaged at contact.
    pub autoland: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PhaseAutomation {
    pub phase: &'static str,
    pub s: f64,
    pub ap_s: Option<f64>,
    pub at_s: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Ride {
    /// m/s².
    pub vertical_rms: Option<f64>,
    pub ride_band: Option<&'static str>,
    pub lateral_rms: Option<f64>,
    pub sideslip_rms_deg: Option<f64>,
    pub max_bank_deg: BTreeMap<&'static str, f64>,
    pub rotation_rate_dps: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Fuel {
    pub fuel_used_kg: Option<f64>,
    pub fuel_kg_h: Option<f64>,
    pub fuel_kg_nm: Option<f64>,
    pub taxi_fuel_kg: Option<f64>,
    pub descent_level_s: Option<f64>,
    pub descent_levels: Option<usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Control {
    pub manual_s: Option<f64>,
    pub reversals_per_min: Reversals,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Reversals {
    pub pitch: Option<f64>,
    pub roll: Option<f64>,
}

/// The figures trends need (`FlightSummary.skills`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SkillsSummary {
    pub ap_share: Option<f64>,
    pub at_share: Option<f64>,
    pub ap_off_ft: Option<f64>,
    #[serde(default)]
    pub autoland: bool,
    pub vertical_rms: Option<f64>,
    pub ride_band: Option<String>,
    pub fuel_kg_h: Option<f64>,
    pub fuel_kg_nm: Option<f64>,
    pub descent_level_s: Option<f64>,
}

impl From<&Skills> for SkillsSummary {
    fn from(s: &Skills) -> Self {
        let a = &s.automation;
        let share = |on: Option<f64>| {
            on.filter(|_| a.airborne_s > 0.0)
                .map(|on| r2(on / a.airborne_s))
        };
        Self {
            ap_share: share(a.ap_s),
            at_share: share(a.at_s),
            ap_off_ft: a.ap_off_ft,
            autoland: a.autoland,
            vertical_rms: s.ride.vertical_rms,
            ride_band: s.ride.ride_band.map(str::to_owned),
            fuel_kg_h: s.fuel.fuel_kg_h,
            fuel_kg_nm: s.fuel.fuel_kg_nm,
            descent_level_s: s.fuel.descent_level_s,
        }
    }
}

/// The flying skills of an analysed flight.
pub fn skills(a: &Analysis) -> Skills {
    let transport = a.selection.class == Some(Class::Transport);
    Ctx::new(&a.flight, &a.track, &a.phases, &a.touchdowns, transport).skills()
}

/// A flight with the per-point series every measure reads.
struct Ctx<'a> {
    flight: &'a Flight,
    track: &'a Track,
    spans: &'a [Span],
    touchdowns: &'a [Touchdown],
    transport: bool,
    /// The airborne track points: each leg from lift-off to touchdown.
    air: Vec<usize>,
    /// Autopilot and autothrottle engaged at each point; `None` without a valid value.
    ap: Vec<Option<bool>>,
    at: Vec<Option<bool>>,
    phase: Vec<Option<Phase>>,
}

impl<'a> Ctx<'a> {
    fn new(
        flight: &'a Flight,
        track: &'a Track,
        spans: &'a [Span],
        touchdowns: &'a [Touchdown],
        transport: bool,
    ) -> Self {
        let flag = |name| {
            let on = flight.resample(name, &track.t).into_iter();
            on.map(|v| v.is_finite().then_some(v >= 0.5)).collect()
        };
        let mut phase = vec![None; track.len()];
        for s in spans {
            phase[points(track, s.start, s.end)].fill(Some(s.phase));
        }
        Self {
            flight,
            track,
            spans,
            touchdowns,
            transport,
            air: track.airborne().collect(),
            ap: flag("ap.engaged"),
            at: flag("at.engaged"),
            phase,
        }
    }

    fn skills(&self) -> Skills {
        // A hop or a slew is too short for an RMS, a rate or a burn to mean
        // anything: its measures stay out rather than read as smooth.
        if Self::s(self.air.len()) < MIN_AIRBORNE_S {
            return Skills {
                automation: self.automation(),
                ride: Ride::default(),
                fuel: Fuel::default(),
                control: Control::default(),
            };
        }
        Skills {
            automation: self.automation(),
            ride: self.ride(),
            fuel: self.fuel(),
            control: self.control(),
        }
    }

    /// Seconds in `n` track points.
    fn s(n: usize) -> f64 {
        n as f64 / TRACK_HZ
    }

    fn param(&self, name: &str) -> Vec<f64> {
        self.flight.resample(name, &self.track.t)
    }

    /// The first span of `phase`, or the last.
    fn span(&self, phase: Phase, last: bool) -> Option<&Span> {
        let mut spans = self.spans.iter().filter(|s| s.phase == phase);
        if last {
            spans.next_back()
        } else {
            spans.next()
        }
    }

    /// The final landing's first touchdown and its track point.
    fn final_touchdown(&self) -> Option<(&Touchdown, usize)> {
        let td = self.touchdowns.iter().rfind(|td| !td.bounce)?;
        Some((td, self.track.index_at(td.t)))
    }

    /// The airborne points of each phase that has any, in phase order.
    fn air_by_phase(&self) -> Vec<(Phase, Vec<usize>)> {
        let of = |p| {
            self.air
                .iter()
                .copied()
                .filter(move |&i| self.phase[i] == Some(p))
        };
        let by = Phase::ALL.map(|p| (p, of(p).collect::<Vec<_>>()));
        by.into_iter().filter(|(_, idx)| !idx.is_empty()).collect()
    }

    /// Time `flags` is set over `idx`; `None` if it has no valid value there.
    fn engaged_s(flags: &[Option<bool>], idx: &[usize]) -> Option<f64> {
        let known = idx.iter().any(|&i| flags[i].is_some());
        let on = idx.iter().filter(|&&i| flags[i] == Some(true)).count();
        known.then(|| r1(Self::s(on)))
    }

    fn automation(&self) -> Automation {
        let (ap_off_ft, manual_final_s, autoland) = self.disconnect();
        Automation {
            airborne_s: r1(Self::s(self.air.len())),
            ap_s: Self::engaged_s(&self.ap, &self.air),
            at_s: Self::engaged_s(&self.at, &self.air),
            by_phase: (self.air_by_phase().into_iter())
                .map(|(p, idx)| PhaseAutomation {
                    phase: p.name(),
                    s: r1(Self::s(idx.len())),
                    ap_s: Self::engaged_s(&self.ap, &idx),
                    at_s: Self::engaged_s(&self.at, &idx),
                })
                .collect(),
            ap_off_ft,
            manual_final_s,
            autoland,
        }
    }

    /// The last autopilot disconnect of the final approach: its radio height
    /// and the hand-flown time to touchdown, and whether it was an autoland
    /// (no disconnect, no hand-flown time); `None` if the autopilot was not
    /// on in the approach.
    fn disconnect(&self) -> (Option<f64>, Option<f64>, bool) {
        let track = self.track;
        let Some((td, end)) = self.final_touchdown() else {
            return (None, None, false);
        };
        let approach = (self.spans.iter()).rfind(|s| s.phase == Phase::Approach && s.start <= td.t);
        let Some(from) = approach.map(|s| track.index_at(s.start)) else {
            return (None, None, false);
        };
        let Some(on) = (from..=end).rfind(|&i| self.ap[i] == Some(true)) else {
            return (None, None, false);
        };
        if on == end {
            return (None, Some(0.0), true);
        }
        let off = on + 1;
        let ft = [track.ra_ft[off], track.aal_ft[off]];
        let ft = ft.into_iter().find(|h| h.is_finite());
        (
            ft.map(|h| r1(h.max(0.0))),
            Some(r1(Self::s(end - off))),
            false,
        )
    }

    fn ride(&self) -> Ride {
        let (nz, window) = (&self.track.nz_g, (MOVING_MEAN_S * TRACK_HZ) as usize);
        let load: Vec<f64> = (self.track.legs.iter())
            .flat_map(|l| detrended(&nz[l.airborne.clone()], window))
            .collect();
        let (lat, beta) = (self.param("accel.lat"), self.param("sideslip"));
        let aal = &self.track.aal_ft;
        let ias = &self.track.ias_kt;
        let low = (self.air.iter())
            .filter(|&&i| aal[i] < SIDESLIP_MAX_AAL_FT && ias[i] >= SIDESLIP_MIN_IAS_KT);
        let vertical_rms = rms(load.into_iter()).map(|g| g * G_MS2);
        Ride {
            vertical_rms: vertical_rms.map(r2),
            ride_band: vertical_rms.map(ride_band),
            lateral_rms: rms((self.air.iter())
                .map(|&i| side_force(lat[i], self.track.roll_deg[i], self.track.pitch_deg[i])))
            .map(|g| r2(g * G_MS2)),
            sideslip_rms_deg: rms(low.map(|&i| beta[i])).map(r2),
            max_bank_deg: (self.air_by_phase().into_iter())
                .filter_map(|(p, idx)| {
                    let bank = idx.iter().map(|&i| self.track.roll_deg[i].abs());
                    let peak = bank.filter(|b| b.is_finite()).max_by(f64::total_cmp);
                    Some((p.name(), r1(peak?)))
                })
                .collect(),
            rotation_rate_dps: self.rotation_rate(),
        }
    }

    /// Peak pitch rate from rotation to 35 ft, transport class only.
    fn rotation_rate(&self) -> Option<f64> {
        let roll = self
            .span(Phase::Takeoff, false)
            .filter(|_| self.transport)?;
        analyzer::rotation_rate(self.track, points(self.track, roll.start, roll.end)).map(r1)
    }

    fn fuel(&self) -> Fuel {
        let roll = self.span(Phase::Takeoff, false);
        let stop = self.span(Phase::Landing, true);
        let used = roll.zip(stop).and_then(|(a, b)| self.burn(a.start, b.end));
        let gs = (self.air.iter()).map(|&i| self.track.gs_kt[i]);
        let gs: Vec<f64> = gs.filter(|g| g.is_finite()).collect();
        let nm = gs.iter().sum::<f64>() / TRACK_HZ / 3600.0;
        let hours = Self::s(self.air.len()) / 3600.0;
        // The rates divide airborne time and distance, so they take the
        // airborne burn: each leg lift-off to touchdown, not the rolls or a
        // taxi back between circuits.
        let t = &self.track.t;
        let airborne = (self.track.legs.iter())
            .filter(|l| !l.airborne.is_empty())
            .map(|l| self.burn(t[l.airborne.start], t[l.airborne.end - 1]))
            .sum::<Option<f64>>();
        let levels = self.descent_levels();
        let taxi = self.spans.iter().filter(|s| s.phase == Phase::Taxi);
        let taxi = taxi.map(|s| self.burn(s.start, s.end)).sum::<Option<f64>>();
        Fuel {
            fuel_used_kg: used.map(r1),
            fuel_kg_h: airborne.filter(|_| hours > 0.0).map(|kg| r1(kg / hours)),
            fuel_kg_nm: airborne.filter(|_| nm > 0.0).map(|kg| r2(kg / nm)),
            taxi_fuel_kg: taxi
                .filter(|_| self.span(Phase::Taxi, false).is_some())
                .map(r1),
            descent_level_s: levels.map(|(s, _)| r1(s)),
            descent_levels: levels.map(|(_, n)| n),
        }
    }

    /// `fuel.total` in kg at `t`; the sampling (4 s) may stop just short of
    /// the flight's end, so a time past it reads the last sample.
    fn kg(&self, t: SimTime) -> Option<f64> {
        let series = self.flight.series("fuel.total");
        let t = t.clamp(series.first()?.t, series.last()?.t);
        self.flight.at("fuel.total", t).map(|lb| lb * KG_PER_LB)
    }

    /// Fuel burnt from `from` to `to`; `None` if it is unknown or rose in
    /// between (fuel added in flight, or a slew).
    fn burn(&self, from: SimTime, to: SimTime) -> Option<f64> {
        let mut low = f64::INFINITY;
        let series = self.flight.series("fuel.total").iter();
        let rose = series
            .filter(|s| s.is_valid() && (from..=to).contains(&s.t))
            .any(|s| {
                let kg = s.value * KG_PER_LB;
                low = low.min(kg);
                kg > low + REFUEL_KG
            });
        let kg = self.kg(from)? - self.kg(to)?;
        (!rose).then_some(kg.max(0.0))
    }

    /// Time and number of the level segments between the top of descent and
    /// 1,000 ft above the landing runway. The top is the last point near
    /// the final leg's highest; the end is where the last stretch below
    /// 1,000 ft before touchdown begins.
    fn descent_levels(&self) -> Option<(f64, usize)> {
        let track = self.track;
        let leg = track.legs.last()?;
        let (_, td) = self.final_touchdown()?;
        let end = 1 + (leg.airborne.start..=td).rfind(|&i| track.aal_ft[i] > DESCENT_END_FT)?;
        let highest = track.alt_ft[leg.peak] - TOP_TOLERANCE_FT;
        let top = (leg.airborne.start..end).rfind(|&i| track.alt_ft[i] >= highest)?;
        let levels = level_segments(&track.vs_fpm[top..end]);
        Some((Self::s(levels.iter().sum()), levels.len()))
    }

    fn control(&self) -> Control {
        let mut manual = vec![false; self.track.len()];
        for &i in &self.air {
            manual[i] = self.ap[i] == Some(false);
        }
        let count = manual.iter().filter(|&&m| m).count();
        let known = self.air.iter().any(|&i| self.ap[i].is_some());
        let per_min = |name| {
            let v = self.param(name);
            let held = runs(v.len(), |i| manual[i] && v[i].is_finite());
            let (n, pts) = held.fold((0, 0), |(n, pts), r| {
                (n + reversals(&v[r.clone()], REVERSAL_PCT), pts + r.len())
            });
            // Over the time this channel was recorded, not all hand-flown time.
            (pts > 0).then(|| r1(n as f64 * 60.0 / Self::s(pts)))
        };
        Control {
            manual_s: known.then(|| r1(Self::s(count))),
            reversals_per_min: Reversals {
                pitch: per_min("ctl.pitch"),
                roll: per_min("ctl.roll"),
            },
        }
    }
}

/// The track points from `from` up to `to`, the last point included when
/// `to` is the end of the track.
fn points(track: &Track, from: SimTime, to: SimTime) -> Range<usize> {
    let before = |t: SimTime| track.t.partition_point(|&x| x < t);
    let end = if track.t.last().is_some_and(|&l| to >= l) {
        track.len()
    } else {
        before(to)
    };
    before(from)..end
}

/// The band `vertical_rms` (m/s²) falls in: the lowest that contains it.
fn ride_band(rms: f64) -> &'static str {
    let band = RIDE_BANDS.iter().find(|(top, _)| rms < *top);
    band.map_or(RIDE_BAND_TOP, |b| b.1)
}

/// Root mean square of the finite values.
fn rms(v: impl Iterator<Item = f64>) -> Option<f64> {
    let v: Vec<f64> = v.filter(|x| x.is_finite()).collect();
    (!v.is_empty()).then(|| (v.iter().map(|x| x * x).sum::<f64>() / v.len() as f64).sqrt())
}

/// The finite values of `v` minus their moving mean over `window` points.
fn detrended(v: &[f64], window: usize) -> Vec<f64> {
    let v: Vec<f64> = v.iter().copied().filter(|x| x.is_finite()).collect();
    let mut sum = vec![0.0];
    sum.extend(v.iter().scan(0.0, |acc, x| {
        *acc += x;
        Some(*acc)
    }));
    (0..v.len())
        .map(|i| {
            let (lo, hi) = (i.saturating_sub(window / 2), (i + window / 2).min(v.len()));
            v[i] - (sum[hi] - sum[lo]) / (hi - lo) as f64
        })
        .collect()
}

/// Lengths (points) of the stretches of `vs` within ±300 fpm for at least 10 s.
fn level_segments(vs: &[f64]) -> Vec<usize> {
    let min = (LEVEL_MIN_S * TRACK_HZ) as usize;
    let level = runs(vs.len(), |i| vs[i].abs() <= LEVEL_FPM);
    level.map(|r| r.len()).filter(|&n| n >= min).collect()
}

/// Direction changes of `v` larger than `threshold`: a reversal counts once
/// the signal has come back that far from its last extreme.
fn reversals(v: &[f64], threshold: f64) -> usize {
    let mut it = v.iter().copied();
    let Some(mut extreme) = it.next() else {
        return 0;
    };
    let (mut dir, mut n) = (0.0_f64, 0);
    for x in it {
        let d = x - extreme;
        if dir == 0.0 {
            if d.abs() > threshold {
                (dir, extreme) = (d.signum(), x);
            }
        } else if dir * d > 0.0 {
            extreme = x;
        } else if -dir * d > threshold {
            (dir, extreme, n) = (-dir, x, n + 1);
        }
    }
    n
}

/// The lateral load the ball shows, in g. `accel.lat` is the aircraft's
/// acceleration (`ACCELERATION BODY X`), which in a coordinated turn carries
/// gravity's share along the wing, sin(bank)·cos(pitch); without it a 25°
/// turn reads as 0.4 g of side load (checked on recorded flights).
fn side_force(lat_g: f64, roll_deg: f64, pitch_deg: f64) -> f64 {
    lat_g - roll_deg.to_radians().sin() * pitch_deg.to_radians().cos()
}

#[cfg(test)]
mod tests {
    use analyzer::{PhaseConfig, phases, touchdowns};
    use testkit::{AIRLINER_LB, Pt, fly, landing, lerp};

    use super::*;

    const LIFTOFF_S: f64 = 30.0;
    const TOUCHDOWN_S: f64 = 300.0;
    /// Pounds of fuel burnt per second of the sortie.
    const LB_S: f64 = 0.8;

    /// A transport sortie: roll to liftoff at 30 s, climb to 3000 ft, a 30 s
    /// level-off at 2000 ft in the descent, touchdown at 300 s, stop at 346 s.
    fn sortie(t: f64) -> Pt {
        let alt = [
            (0.0, 0.0),
            (30.0, 0.0),
            (90.0, 3000.0),
            (150.0, 3000.0),
            (170.0, 2000.0),
            (200.0, 2000.0),
            (300.0, 0.0),
        ];
        let speed = [
            (0.0, 0.0),
            (30.0, 140.0),
            (300.0, 140.0),
            (340.0, 20.0),
            (346.0, 0.0),
        ];
        Pt {
            alt: lerp(&alt, t),
            speed: lerp(&speed, t),
            pitch: lerp(&[(0.0, 0.0), (25.0, 0.0), (28.0, 9.0), (60.0, 3.0)], t),
            ground: !(LIFTOFF_S..TOUCHDOWN_S).contains(&t),
            ..landing(t, TOUCHDOWN_S, 1.0)
        }
    }

    /// The skills of a `sortie` that `tweak` changes at each instant.
    fn fly_skills(tweak: impl Fn(f64, &mut Pt)) -> Skills {
        let (flight, track) = fly(AIRLINER_LB, 346.0, |t| {
            let mut p = sortie(t);
            tweak(t, &mut p);
            p
        });
        let spans = phases(&track, &PhaseConfig::TRANSPORT);
        let tds = touchdowns(&flight, &track);
        Ctx::new(&flight, &track, &spans, &tds, true).skills()
    }

    fn near(x: Option<f64>, want: f64, tol: f64) {
        let x = x.expect("a value");
        assert!((x - want).abs() <= tol, "{x} is not {want} within {tol}");
    }

    /// The autothrottle engaged from `from` to `to` s.
    fn at_on(from: f64, to: f64, t: f64, p: &mut Pt) {
        let on = f64::from(u8::from((from..to).contains(&t)));
        p.set = vec![("AUTOPILOT MANAGED THROTTLE ACTIVE", on)];
    }

    #[test]
    fn autopilot_share_and_phases() {
        let s = fly_skills(|t, p| {
            p.ap = (60.0..280.0).contains(&t);
            at_on(40.0, 100.0, t, p);
        });
        let a = &s.automation;
        near(Some(a.airborne_s), 270.0, 1.0);
        near(a.ap_s, 220.0, 1.0);
        near(a.at_s, 60.0, 1.0);
        let total: f64 = a.by_phase.iter().map(|p| p.s).sum();
        assert!((total - a.airborne_s).abs() < 0.5);
        let phase = |name| a.by_phase.iter().find(|p| p.phase == name).unwrap();
        // The approach starts at 1000 ft, 250 s; the autopilot goes off at 280 s.
        near(phase("Approach").ap_s, 30.0, 1.5);
        near(phase("EnRoute").ap_s, 190.0, 1.5);
        let summary = SkillsSummary::from(&s);
        near(summary.ap_share, 0.81, 0.01);
        near(summary.at_share, 0.22, 0.01);
    }

    #[test]
    fn autopilot_disconnect_height() {
        let off = |from: f64, to: f64| {
            let a = fly_skills(|t, p| p.ap = (from..to).contains(&t)).automation;
            (a.ap_off_ft, a.manual_final_s, a.autoland)
        };
        // 20 s before touchdown at 20 ft/s.
        let (ft, manual, _) = off(60.0, 280.0);
        near(ft, 400.0, 30.0);
        near(manual, 20.0, 1.0);
        // Still engaged at contact: an autoland.
        let (ft, manual, autoland) = off(60.0, 320.0);
        assert_eq!((ft, manual, autoland), (None, Some(0.0), true));
        // Off before the approach began (250 s): never on in it.
        assert_eq!(off(60.0, 200.0), (None, None, false));
        let a = fly_skills(|t, p| p.ap = (60.0..200.0).contains(&t)).automation;
        near(a.ap_s, 140.0, 1.0);
        assert_eq!(off(0.0, 0.0).0, None);
    }

    #[test]
    fn missing_parameters_are_null_not_zero() {
        let s = fly_skills(|_, p| p.lost = &["AUTOPILOT MASTER"]);
        assert_eq!((s.automation.ap_s, s.automation.at_s), (None, None));
        assert_eq!(s.automation.ap_off_ft, None);
        assert_eq!(s.control.manual_s, None);
        assert_eq!(s.control.reversals_per_min.pitch, None);
        let f = &s.fuel;
        assert_eq!(
            (f.fuel_used_kg, f.fuel_kg_h, f.fuel_kg_nm, f.taxi_fuel_kg),
            (None, None, None, None)
        );
        assert_eq!(s.ride.lateral_rms, None);
        assert_eq!(SkillsSummary::from(&s).ap_share, None);
        assert!(s.automation.airborne_s > 0.0);
    }

    #[test]
    fn vertical_rms_removes_the_moving_mean() {
        // 1 g plus a 0.1 g sine of 1 s: the mean is the 1 g, the RMS 0.1 g / sqrt 2.
        let s = fly_skills(|t, p| p.g = 1.0 + 0.1 * (std::f64::consts::TAU * t).sin());
        near(s.ride.vertical_rms, 0.1 / 2.0_f64.sqrt() * G_MS2, 0.03);
        assert_eq!(s.ride.ride_band, Some("fairly uncomfortable"));
        // A constant load has no ride roughness.
        near(fly_skills(|_, p| p.g = 1.3).ride.vertical_rms, 0.0, 0.01);
    }

    #[test]
    fn detrending_and_rms_ignore_missing_values() {
        let v = [1.0, f64::NAN, 3.0];
        assert_eq!(detrended(&v, 16), [-1.0, 1.0]);
        assert_eq!(rms([3.0, f64::NAN, 4.0].into_iter()), Some(12.5_f64.sqrt()));
        assert_eq!(rms(std::iter::empty()), None);
    }

    #[test]
    fn ride_bands_are_the_lowest_that_contain_the_value() {
        assert_eq!(ride_band(0.0), "not uncomfortable");
        assert_eq!(ride_band(0.314), "not uncomfortable");
        assert_eq!(ride_band(0.315), "a little uncomfortable");
        assert_eq!(ride_band(0.629), "a little uncomfortable");
        assert_eq!(ride_band(0.63), "fairly uncomfortable");
        assert_eq!(ride_band(1.0), "uncomfortable");
        assert_eq!(ride_band(1.6), "very uncomfortable");
        assert_eq!(ride_band(2.49), "very uncomfortable");
        assert_eq!(ride_band(2.5), "extremely uncomfortable");
    }

    #[test]
    fn side_force_takes_gravity_out_of_a_turn() {
        // Coordinated: the body acceleration is all gravity's share.
        let turn = 25f64.to_radians().sin() * 3f64.to_radians().cos();
        assert!(side_force(turn, 25.0, 3.0).abs() < 1e-12);
        assert!((side_force(0.1, 0.0, 0.0) - 0.1).abs() < 1e-12);
    }

    #[test]
    fn lateral_sideslip_bank_and_rotation() {
        let s = fly_skills(|t, p| {
            // 0.05 g of side load and 2 deg all flight; a coordinated 20 deg
            // turn (the sim flips the bank's sign) from 100 to 120 s, where
            // the body acceleration also carries gravity's share.
            let bank: f64 = if (100.0..120.0).contains(&t) {
                -20.0
            } else {
                0.0
            };
            let gravity = (-bank).to_radians().sin();
            p.set = vec![
                ("ACCELERATION BODY X", (0.05 + gravity) * 32.174),
                ("INCIDENCE BETA", 2.0),
                ("PLANE BANK DEGREES", bank),
            ];
        });
        near(s.ride.lateral_rms, 0.05 * G_MS2, 0.01);
        near(s.ride.sideslip_rms_deg, 2.0, 0.01);
        near(s.ride.max_bank_deg.get("EnRoute").copied(), 20.0, 0.5);
        // Pitch rises 3 deg/s from 25 s.
        near(s.ride.rotation_rate_dps, 3.0, 0.3);
    }

    #[test]
    fn rotation_rate_is_for_transport_class_only() {
        let (flight, track) = fly(AIRLINER_LB, 346.0, sortie);
        let spans = phases(&track, &PhaseConfig::TRANSPORT);
        let tds = touchdowns(&flight, &track);
        let light = Ctx::new(&flight, &track, &spans, &tds, false).skills();
        assert_eq!(light.ride.rotation_rate_dps, None);
    }

    fn burn_lb(t: f64) -> f64 {
        2000.0 - LB_S * t
    }

    /// The fuel measures of a sortie whose tank reads `tank` pounds, and the
    /// times the take-off roll starts and the landing roll ends.
    fn fuel_flown(tank: impl Fn(f64) -> f64) -> (Fuel, f64, f64) {
        let (flight, track) = fly(AIRLINER_LB, 346.0, |t| Pt {
            set: vec![("FUEL TOTAL QUANTITY WEIGHT", tank(t))],
            ..sortie(t)
        });
        let spans = phases(&track, &PhaseConfig::TRANSPORT);
        let tds = touchdowns(&flight, &track);
        let roll = spans.iter().find(|s| s.phase == Phase::Takeoff).unwrap();
        let stop = spans.iter().rfind(|s| s.phase == Phase::Landing).unwrap();
        let s = Ctx::new(&flight, &track, &spans, &tds, true).skills();
        (s.fuel, roll.start.as_secs_f64(), stop.end.as_secs_f64())
    }

    #[test]
    fn fuel_used_per_hour_and_per_mile() {
        let (f, from, to) = fuel_flown(burn_lb);
        let used = LB_S * (to - from) * KG_PER_LB;
        near(f.fuel_used_kg, used, 0.5);
        // The rates are the airborne burn alone: the steady flow, at 140 kt
        // of ground speed, without the take-off and landing rolls.
        let kg_h = LB_S * 3600.0 * KG_PER_LB;
        near(f.fuel_kg_h, kg_h, 15.0);
        near(f.fuel_kg_nm, kg_h / 140.0, 0.1);
        // Taxi out and in are a few seconds each.
        let taxi = f.taxi_fuel_kg.unwrap();
        assert!(taxi > 0.0 && taxi < 10.0, "{taxi}");
    }

    #[test]
    fn fuel_that_rose_in_flight_is_unknown() {
        let (f, ..) = fuel_flown(|t| burn_lb(t) + if t >= 150.0 { 100.0 } else { 0.0 });
        assert_eq!(
            (f.fuel_used_kg, f.fuel_kg_h, f.fuel_kg_nm),
            (None, None, None)
        );
        // Taxi is measured on the ground, away from the jump.
        assert!(f.taxi_fuel_kg.is_some());
        // Quantity noise under the threshold is not a refuel.
        let (f, ..) = fuel_flown(|t| {
            burn_lb(t)
                + if (t as u64).is_multiple_of(2) {
                    0.5
                } else {
                    0.0
                }
        });
        assert!(f.fuel_used_kg.is_some());
    }

    #[test]
    fn level_segments_need_ten_seconds() {
        let hz = TRACK_HZ as usize;
        assert_eq!(level_segments(&vec![0.0; 10 * hz]), [80]);
        assert_eq!(level_segments(&vec![250.0; 8 * hz]), Vec::<usize>::new());
        // +-300 fpm is level, beyond it is not; segments are counted apart.
        let vs = [
            vec![-300.0; 10 * hz],
            vec![-301.0; 3],
            vec![300.0; 12 * hz],
            vec![f64::NAN; 90],
        ];
        assert_eq!(level_segments(&vs.concat()), [80, 96]);
    }

    #[test]
    fn the_descent_has_one_level_segment() {
        let s = fly_skills(|_, _| {});
        assert_eq!(s.fuel.descent_levels, Some(1));
        // 30 s level at 2000 ft; the derived rate trims some at the edges.
        let level = s.fuel.descent_level_s.unwrap();
        assert!((10.0..=31.0).contains(&level), "{level}");
    }

    #[test]
    fn reversals_need_a_swing_over_the_threshold() {
        assert_eq!(reversals(&[0.0, 2.0, 0.0, 2.0, 0.0], 1.0), 3);
        // Wiggles under the threshold, drift without turning back, a flat signal.
        assert_eq!(reversals(&[0.0, 0.6, 0.0, 0.6, 0.0], 1.0), 0);
        assert_eq!(reversals(&[0.0, 2.0, 4.0, 8.0], 1.0), 0);
        assert_eq!(reversals(&[0.0; 5], 1.0), 0);
        // The turn-back is measured from the extreme: 4 down to 2.9 is over 1, 3.5 is not.
        assert_eq!(reversals(&[0.0, 4.0, 3.5, 4.0, 2.9, 4.0, 2.0], 1.0), 3);
        assert_eq!(reversals(&[], 1.0), 0);
    }

    #[test]
    fn reversals_count_only_while_hand_flying() {
        // The pitch control flips between 0 and 10 each second from 100 to 160 s;
        // the autopilot is on for the first 30 s of that.
        let s = fly_skills(|t, p| {
            p.ap = (100.0..130.0).contains(&t);
            let flip = (100.0..160.0).contains(&t) && (t as u64).is_multiple_of(2);
            p.set = vec![("YOKE Y POSITION", if flip { 10.0 } else { 0.0 })];
        });
        // 240 s hand-flown, about 30 reversals in the last 30 s of the flips.
        near(s.control.manual_s, 240.0, 1.0);
        near(s.control.reversals_per_min.pitch, 7.5, 1.0);
        assert_eq!(s.control.reversals_per_min.roll, None);
    }
}
