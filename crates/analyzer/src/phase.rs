use std::ops::Range;
use std::str::FromStr;

use fdm_core::SimTime;

use crate::{Leg, TRACK_HZ, Track, runs};

/// Phases of flight per the CICTT (ICAO/CAST) taxonomy; the boundaries are
/// measurable proxies for the crew actions CICTT names (`docs/phases.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Standing,
    Taxi,
    /// Takeoff roll to 35 ft above the runway.
    Takeoff,
    /// 35 ft to `climb_end_ft` above the aerodrome (or the top of a lower
    /// climb).
    InitialClimb,
    EnRoute,
    /// Descending through `approach_ft` above the aerodrome (or from the
    /// top of a lower climb) to the flare.
    Approach,
    /// Flare to leaving the runway, or to the lowest ground speed of a
    /// touch-and-go.
    Landing,
    GoAround,
}

impl Phase {
    pub const ALL: [Self; 8] = [
        Self::Standing,
        Self::Taxi,
        Self::Takeoff,
        Self::InitialClimb,
        Self::EnRoute,
        Self::Approach,
        Self::Landing,
        Self::GoAround,
    ];

    /// The variant name, as printed and parsed.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Standing => "Standing",
            Self::Taxi => "Taxi",
            Self::Takeoff => "Takeoff",
            Self::InitialClimb => "InitialClimb",
            Self::EnRoute => "EnRoute",
            Self::Approach => "Approach",
            Self::Landing => "Landing",
            Self::GoAround => "GoAround",
        }
    }
}

impl FromStr for Phase {
    type Err = String;

    /// Parses the variant name, e.g. `InitialClimb`.
    fn from_str(s: &str) -> Result<Self, String> {
        Self::ALL
            .into_iter()
            .find(|p| p.name() == s)
            .ok_or_else(|| {
                let names: Vec<_> = Self::ALL.iter().map(|p| p.name()).collect();
                format!("unknown phase `{s}`; expected one of {}", names.join(", "))
            })
    }
}

/// The phase boundaries that depend on the aircraft: class defaults, which
/// a type profile may override.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PhaseConfig {
    /// Ground speed from which a ground run is a takeoff or landing roll:
    /// above any taxi speed, below every liftoff speed, kt.
    pub roll_kt: f64,
    /// The initial climb ends this high above the aerodrome, ft.
    pub climb_end_ft: f64,
    /// The approach starts descending through this height above the
    /// aerodrome, ft; at least `climb_end_ft`.
    pub approach_ft: f64,
    /// The landing starts (the flare) below this height above the runway, ft.
    pub flare_ft: f64,
    /// A climb this high from an approach's lowest point is a go-around, ft.
    pub go_around_rise_ft: f64,
    /// A go-around climb ends at its highest point once the height has
    /// fallen this far below it; smaller dips are noise or a levelling-off, ft.
    pub go_around_drop_ft: f64,
}

impl PhaseConfig {
    /// CICTT puts both the end of the initial climb and the start of the
    /// approach at 1000 ft; the flare from 50 ft, also the threshold
    /// crossing height.
    pub const TRANSPORT: Self = Self {
        roll_kt: 30.0,
        climb_end_ft: 1000.0,
        approach_ft: 1000.0,
        flare_ft: 50.0,
        go_around_rise_ft: 200.0,
        go_around_drop_ft: 50.0,
    };
    /// A light aircraft flares from about 10–20 ft.
    pub const LIGHT: Self = Self {
        flare_ft: 20.0,
        ..Self::TRANSPORT
    };
}

/// What opened a phase span, as measured where it starts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cause {
    pub trigger: Trigger,
    /// The `basis` parameter at the start of the span; `NaN` if unknown.
    pub value: f64,
    /// The boundary of `trigger` (a `PhaseConfig` value or a fixed one); kt
    /// on a ground speed basis, else ft.
    pub threshold: f64,
    pub basis: Basis,
}

/// The proxy that opened a span.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    /// On the ground under 2 kt.
    Stopped,
    /// Moving on the ground under the roll speed.
    Moving,
    /// On the ground at roll speed and continuing to liftoff.
    Roll,
    /// The lowest ground speed of a landing roll that runs into a liftoff:
    /// takeoff power applied, which ends a touch-and-go landing (CICTT).
    TouchAndGo,
    /// Climbing through 35 ft above the runway.
    Liftoff,
    /// Climbing through `climb_end_ft` above the aerodrome.
    ClimbEnd,
    /// Past the highest point of a climb that stayed under the boundary.
    TopOfClimb,
    /// Descending through `approach_ft` above the aerodrome.
    Descent,
    /// Descending through `flare_ft` above the runway.
    Flare,
    /// Ground contact with no flare seen.
    Touchdown,
    /// The lowest point of an approach, from which the aircraft climbed
    /// `go_around_rise_ft` without touching down.
    GoAround,
    /// Already airborne at the first recorded point.
    Airborne,
}

/// The parameter a `Cause` is measured on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Basis {
    GroundSpeed,
    /// Calibrated radio height, above the runway.
    Ra,
    /// Height above the aerodrome; also the height above the runway where
    /// RA is invalid.
    Aal,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Span {
    pub phase: Phase,
    pub start: SimTime,
    pub end: SimTime,
    pub cause: Cause,
}

pub(crate) const STANDING_MAX_GS_KT: f64 = 2.0;
pub(crate) const TAKEOFF_END_FT: f64 = 35.0;
/// A gap in ground speed this long (2 s at the track rate) still belongs to
/// a roll; a longer one ends it.
const ROLL_MAX_GAP: usize = 2 * TRACK_HZ as usize;

/// The phase at every track point.
pub fn phase_labels(track: &Track, cfg: &PhaseConfig) -> Vec<Phase> {
    Labels::of(track, cfg).into_iter().map(|(p, _)| p).collect()
}

/// Contiguous phase spans covering the whole track, each with its cause.
pub fn phases(track: &Track, cfg: &PhaseConfig) -> Vec<Span> {
    let mut spans: Vec<Span> = Vec::new();
    for (i, (phase, cause)) in Labels::of(track, cfg).into_iter().enumerate() {
        let t = track.t[i];
        match spans.last_mut() {
            Some(last) if last.phase == phase => last.end = t,
            last => {
                if let Some(last) = last {
                    last.end = t;
                }
                spans.push(Span {
                    phase,
                    start: t,
                    end: t,
                    cause,
                });
            }
        }
    }
    spans
}

/// Height above the runway: calibrated RA where valid, else AAL.
fn runway_height(track: &Track, i: usize) -> (f64, Basis) {
    Some(track.ra_ft[i])
        .filter(|r| r.is_finite())
        .map_or((track.aal_ft[i], Basis::Aal), |r| (r, Basis::Ra))
}

/// `trigger` with its parameter at point `i`.
fn cause(track: &Track, trigger: Trigger, i: usize, threshold: f64) -> Cause {
    use Trigger::*;
    let (value, basis) = match trigger {
        Stopped | Moving | Roll | TouchAndGo | Touchdown => (track.gs_kt[i], Basis::GroundSpeed),
        Liftoff | Flare => runway_height(track, i),
        ClimbEnd | TopOfClimb | Descent | GoAround | Airborne => (track.aal_ft[i], Basis::Aal),
    };
    Cause {
        trigger,
        value,
        threshold,
        basis,
    }
}

/// Every point's phase with the cause of its span: by ground speed first,
/// then leg by leg.
struct Labels<'a> {
    track: &'a Track,
    cfg: &'a PhaseConfig,
    label: Vec<(Phase, Cause)>,
}

impl<'a> Labels<'a> {
    fn of(track: &'a Track, cfg: &'a PhaseConfig) -> Vec<(Phase, Cause)> {
        let label = (0..track.len())
            .map(|i| match track.gs_kt[i] >= STANDING_MAX_GS_KT {
                true => (Phase::Taxi, cause(track, Trigger::Moving, i, cfg.roll_kt)),
                false => (
                    Phase::Standing,
                    cause(track, Trigger::Stopped, i, STANDING_MAX_GS_KT),
                ),
            })
            .collect();
        let mut labels = Self { track, cfg, label };
        for leg in &track.legs {
            labels.leg(leg);
        }
        labels.label
    }

    /// Labels `r` as `phase`, opened by `trigger` at its first point.
    fn set(&mut self, r: Range<usize>, phase: Phase, trigger: Trigger, threshold: f64) {
        if !r.is_empty() {
            let c = cause(self.track, trigger, r.start, threshold);
            self.label[r].fill((phase, c));
        }
    }

    fn leg(&mut self, leg: &Leg) {
        use Phase::*;
        let (t, cfg) = (self.track, self.cfg);
        let Range { start: a, end: b } = leg.airborne;
        let aal = &t.aal_ft;

        let taxiing = |p: Phase| matches!(p, Standing | Taxi);
        let before = (0..a)
            .rev()
            .take_while(|&i| t.on_ground[i] && taxiing(self.label[i].0));
        let roll = a - roll_len(before.map(|i| t.gs_kt[i]), cfg.roll_kt);
        let mut i = a;
        while i < b && runway_height(t, i).0 < TAKEOFF_END_FT {
            i += 1;
        }
        self.set(roll..i, Takeoff, Trigger::Roll, cfg.roll_kt);
        let mut j = i;
        while j < b && j <= leg.peak && aal[j] < cfg.climb_end_ft {
            j += 1;
        }
        self.set(i..j, InitialClimb, Trigger::Liftoff, TAKEOFF_END_FT);
        let cruise = match j {
            0 => Trigger::Airborne,
            _ if j < b && aal[j] >= cfg.climb_end_ft => Trigger::ClimbEnd,
            _ => Trigger::TopOfClimb,
        };
        self.set(j..b, EnRoute, cruise, cfg.climb_end_ft);

        // An approach is entered from above. A stretch under `approach_ft`
        // from the end of the initial climb is still that climb or a low
        // cruise up to the leg's top, and an approach once past it under
        // `climb_end_ft`: from the top of a climb under it, or descending
        // through it from a cruise under the pattern height (docs/phases.md).
        for low in runs(b, |k| k >= j && aal[k] < cfg.approach_ft) {
            let (start, entry) = match low.start == j {
                true => (
                    (low.start.max(leg.peak + 1)..low.end)
                        .find(|&k| aal[k] < cfg.climb_end_ft)
                        .unwrap_or(low.end),
                    match aal[leg.peak] >= cfg.climb_end_ft {
                        true => (Trigger::Descent, cfg.climb_end_ft),
                        false => (Trigger::TopOfClimb, cfg.approach_ft),
                    },
                ),
                false => (low.start, (Trigger::Descent, cfg.approach_ft)),
            };
            if start < low.end {
                self.low(start..low.end, low.end == b, entry);
            }
        }

        let after = (b..t.len()).take_while(|&i| t.on_ground[i]);
        let end = b + roll_len(after.map(|i| t.gs_kt[i]), cfg.roll_kt);
        // A roll that runs into the next liftoff is a touch-and-go: the
        // landing ends where takeoff power is applied, at the lowest speed.
        let go = match end < t.len() && !t.on_ground[end] {
            true => (b..end)
                .filter(|&i| t.gs_kt[i].is_finite())
                .min_by(|&x, &y| t.gs_kt[x].total_cmp(&t.gs_kt[y]))
                .unwrap_or(end),
            false => end,
        };
        self.set(b..go, Landing, Trigger::Touchdown, cfg.roll_kt);
        self.set(go..end, Takeoff, Trigger::TouchAndGo, cfg.roll_kt);
    }

    /// A stretch under `approach_ft` entered by `entry` (trigger and its
    /// threshold): approaches, go-arounds and, if the leg `lands` in it, the
    /// landing.
    fn low(&mut self, low: Range<usize>, lands: bool, mut entry: (Trigger, f64)) {
        let (t, cfg) = (self.track, self.cfg);
        let aal = &t.aal_ft;
        let mut start = low.start;
        while start < low.end {
            let mut bottom = start;
            let climb = (start..low.end).find(|&i| {
                if aal[i] < aal[bottom] {
                    bottom = i;
                }
                aal[i] >= aal[bottom] + cfg.go_around_rise_ft
            });
            match climb {
                Some(top) => {
                    let mut peak = top;
                    for i in top..low.end {
                        if aal[i] > aal[peak] {
                            peak = i;
                        } else if aal[i] <= aal[peak] - cfg.go_around_drop_ft {
                            break;
                        }
                    }
                    let rise = cfg.go_around_rise_ft;
                    self.set(start..bottom, Phase::Approach, entry.0, entry.1);
                    self.set(bottom..peak + 1, Phase::GoAround, Trigger::GoAround, rise);
                    start = peak + 1;
                    entry = (Trigger::TopOfClimb, cfg.approach_ft);
                }
                None if lands => {
                    let flare = (start..low.end)
                        .rev()
                        .find(|&i| runway_height(t, i).0 >= cfg.flare_ft)
                        .map_or(start, |i| i + 1);
                    self.set(start..flare, Phase::Approach, entry.0, entry.1);
                    self.set(flare..low.end, Phase::Landing, Trigger::Flare, cfg.flare_ft);
                    return;
                }
                // A shallow dip that climbs back: still en route.
                None => return,
            }
        }
    }
}

/// Points of a roll along `gs_kt`, which runs away from the runway end:
/// those at `roll_kt` or more plus any gap (`NaN`) of up to `ROLL_MAX_GAP`
/// points between them or at the runway end (rolling at liftoff and
/// touchdown), so a dropout does not split the roll but never stretches it
/// past its last known point.
fn roll_len(gs_kt: impl Iterator<Item = f64>, roll_kt: f64) -> usize {
    let (mut gap, mut len, mut known) = (0, 0, false);
    for (n, gs) in gs_kt.enumerate() {
        if gs.is_nan() {
            gap += 1;
            if gap > ROLL_MAX_GAP {
                break;
            }
            if !known {
                len = n + 1;
            }
        } else if gs >= roll_kt {
            (gap, len, known) = (0, n + 1, true);
        } else {
            break;
        }
    }
    len
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: PhaseConfig = PhaseConfig::TRANSPORT;
    /// A turbine pattern: approach from 1500 ft, above the 1000 ft climb end.
    const HIGH_PATTERN: PhaseConfig = PhaseConfig {
        approach_ft: 1500.0,
        ..PhaseConfig::LIGHT
    };

    /// A track of `aal` heights at 8 Hz with the given ground speeds; ground
    /// contact where `on_ground`. RA is missing, so AAL gives the height.
    fn track(aal: Vec<f64>, gs: Vec<f64>, on_ground: Vec<bool>, legs: Vec<Leg>) -> Track {
        Track {
            alt_ft: aal.clone(),
            gs_kt: gs,
            on_ground,
            legs,
            aal_ft: aal.clone(),
            ..Track::blank(aal.len())
        }
    }

    /// A takeoff roll at 60 kt, the leg's heights `air` at 90 kt, then a
    /// rollout at 60 kt slowing to 20 kt.
    fn circuit(air: &[f64]) -> Track {
        let (a, b) = (10, 10 + air.len());
        let n = b + 10;
        let mut aal = vec![0.0; n];
        aal[a..b].copy_from_slice(air);
        let gs = (0..n).map(|i| match i {
            _ if (a..b).contains(&i) => 90.0,
            _ if i < b + 5 => 60.0,
            _ => 20.0,
        });
        let peak = (a..b).max_by(|&x, &y| aal[x].total_cmp(&aal[y])).unwrap();
        let on_ground = (0..n).map(|i| !(a..b).contains(&i)).collect();
        track(
            aal,
            gs.collect(),
            on_ground,
            vec![Leg {
                airborne: a..b,
                peak,
            }],
        )
    }

    fn sequence(spans: &[Span]) -> Vec<Phase> {
        spans.iter().map(|s| s.phase).collect()
    }

    /// Up in 100 ft steps to `top`, and back down.
    fn up_and_down(top: u32) -> Vec<f64> {
        let up = (0..=top / 100).map(|k| f64::from(k * 100));
        up.clone().chain(up.rev().skip(1)).collect()
    }

    #[test]
    fn a_climb_through_the_approach_height_stays_en_route() {
        use Phase::*;
        let t = circuit(&up_and_down(3000));
        let spans = phases(&t, &HIGH_PATTERN);
        let seq = [Takeoff, InitialClimb, EnRoute, Approach, Landing, Taxi];
        assert_eq!(sequence(&spans), seq);
        // 1200 ft on the climb, between the climb end and the approach height.
        assert_eq!(phase_labels(&t, &HIGH_PATTERN)[10 + 12], EnRoute);
        let approach = spans[3].cause;
        assert_eq!(approach.trigger, Trigger::Descent);
        assert_eq!((approach.value, approach.threshold), (1400.0, 1500.0));
        assert_eq!(spans[2].cause.trigger, Trigger::ClimbEnd);
        assert_eq!(spans[0].cause.trigger, Trigger::Roll);
        assert_eq!(spans[0].cause.value, 60.0);
    }

    #[test]
    fn a_climb_topping_under_the_climb_end_approaches_from_its_top() {
        use Phase::*;
        let spans = phases(&circuit(&up_and_down(800)), &HIGH_PATTERN);
        let seq = [Takeoff, InitialClimb, Approach, Landing, Taxi];
        assert_eq!(sequence(&spans), seq);
        assert_eq!(spans[2].cause.trigger, Trigger::TopOfClimb);
        assert_eq!(spans[2].cause.value, 700.0);
    }

    #[test]
    fn a_low_cruise_under_the_approach_height_approaches_through_the_climb_end() {
        use Phase::*;
        // Up to 1300 ft, a cruise with 60 ft dips, down through 1000 ft.
        let mut air = up_and_down(1300)[..14].to_vec();
        air.extend([1240.0, 1300.0, 1240.0, 1290.0, 1300.0]);
        air.extend(up_and_down(1300)[14..].iter().copied());
        let spans = phases(&circuit(&air), &HIGH_PATTERN);
        let seq = [Takeoff, InitialClimb, EnRoute, Approach, Landing, Taxi];
        assert_eq!(sequence(&spans), seq);
        let approach = spans[3].cause;
        assert_eq!(approach.trigger, Trigger::Descent);
        assert_eq!((approach.value, approach.threshold), (900.0, 1000.0));
        // The climb out of a dip to above the top is still en route.
        air.splice(16..17, [1440.0, 1440.0]);
        let spans = phases(&circuit(&air), &HIGH_PATTERN);
        assert_eq!(sequence(&spans), seq);
    }

    #[test]
    fn the_flare_height_follows_the_config() {
        // 100 ft steps down to 0: the last point at or above the flare
        // height is 100 ft for a transport (50 ft) and a light (20 ft) alike,
        // so finer steps near the runway tell them apart.
        let mut air = up_and_down(500);
        air.splice(air.len() - 1.., [60.0, 40.0, 25.0, 10.0]);
        let t = circuit(&air);
        let flare = |cfg| phases(&t, cfg)[3].cause;
        assert_eq!((flare(&T).value, flare(&T).threshold), (40.0, 50.0));
        assert_eq!(flare(&PhaseConfig::LIGHT).value, 10.0);
        assert_eq!(flare(&T).basis, Basis::Aal);
    }

    #[test]
    fn a_dip_in_the_go_around_climb_does_not_end_it() {
        // Down to 100 ft, up through 300 ft (go-around), a 10 ft dip, then on
        // up: the whole climb is the go-around, not an approach again.
        let aal: Vec<f64> = [600.0, 300.0, 100.0, 250.0, 350.0, 340.0, 360.0, 380.0].into();
        let n = aal.len();
        let t = track(aal, vec![80.0; n], vec![false; n], vec![]);
        let c = cause(&t, Trigger::Descent, 0, T.approach_ft);
        let mut labels = Labels {
            track: &t,
            cfg: &T,
            label: vec![(Phase::Approach, c); n],
        };
        labels.low(0..n, true, (Trigger::Descent, T.approach_ft));
        let label: Vec<_> = labels.label.iter().map(|l| l.0).collect();
        assert_eq!(label[1], Phase::Approach);
        assert!(
            label[2..].iter().all(|&p| p == Phase::GoAround),
            "{label:?}"
        );
        assert_eq!(labels.label[2].1.trigger, Trigger::GoAround);
        assert_eq!(labels.label[2].1.value, 100.0);
    }

    /// A landing roll that runs into a second takeoff: Landing to its lowest
    /// ground speed, Takeoff after.
    #[test]
    fn a_touch_and_go_splits_the_roll_at_its_lowest_speed() {
        use Phase::*;
        let mut aal = vec![0.0; 70];
        let mut gs = vec![60.0; 70];
        for i in 10..30 {
            aal[i] = 100.0;
            aal[i + 40] = 100.0;
        }
        for (k, i) in (30..40).enumerate() {
            gs[i] = 52.0 + 2.0 * (k as f64 - 4.0).abs();
        }
        let on_ground = aal.iter().map(|&a| a == 0.0).collect();
        let leg = |a| Leg {
            airborne: a..a + 20,
            peak: a,
        };
        let t = track(aal, gs, on_ground, vec![leg(10), leg(50)]);
        let spans = phases(&t, &T);
        let seq = sequence(&spans);
        let first = [
            Takeoff,
            InitialClimb,
            Approach,
            Landing,
            Takeoff,
            InitialClimb,
        ];
        assert_eq!(seq[..6], first);
        assert_eq!(spans[3].cause.trigger, Trigger::Touchdown);
        assert_eq!(spans[4].start, t.t[34]);
        assert_eq!(spans[4].cause.trigger, Trigger::TouchAndGo);
        assert_eq!(spans[4].cause.value, 52.0);
    }

    /// Takeoff roll (ground, `roll_gs` at `a` points before liftoff) and then
    /// a climb; the leg airborne from index `a`.
    fn takeoff(gs_before: Vec<f64>) -> (Track, usize) {
        let a = gs_before.len();
        let n = a + 40;
        let aal = (0..n).map(|i| (i.saturating_sub(a) * 100) as f64).collect();
        let mut gs = gs_before;
        gs.resize(n, 90.0);
        let on_ground = (0..n).map(|i| i < a).collect();
        let leg = Leg {
            airborne: a..n,
            peak: n - 1,
        };
        (track(aal, gs, on_ground, vec![leg]), a)
    }

    #[test]
    fn a_long_gs_dropout_does_not_stretch_the_takeoff_roll() {
        let mut gs = vec![90.0; 20];
        gs.extend(vec![f64::NAN; 40]);
        let (t, a) = takeoff(gs);
        let label = phase_labels(&t, &T);
        let roll = a - 16; // 2 s of dropout at 8 Hz
        assert_eq!(label[roll - 1], Phase::Standing);
        assert_eq!(label[roll], Phase::Takeoff);
    }

    #[test]
    fn a_short_gs_dropout_does_not_split_the_takeoff_roll() {
        let mut gs = vec![90.0; 20];
        gs.extend(vec![f64::NAN; 10]);
        gs.extend(vec![90.0; 5]);
        let (t, _) = takeoff(gs);
        assert_eq!(phase_labels(&t, &T)[0], Phase::Takeoff);
    }

    #[test]
    fn a_long_gs_dropout_does_not_stretch_the_landing_rollout() {
        let n = 80;
        let mut aal = vec![0.0; n];
        aal[..10]
            .iter_mut()
            .enumerate()
            .for_each(|(i, a)| *a = 100.0 - 10.0 * i as f64);
        let mut gs = vec![60.0; 20];
        gs.extend(vec![f64::NAN; 60]);
        let leg = Leg {
            airborne: 0..10,
            peak: 0,
        };
        let t = track(aal, gs, (0..n).map(|i| i >= 10).collect(), vec![leg]);
        let label = phase_labels(&t, &T);
        // Ground from 10; finite gs to 19, the trailing dropout not rolled.
        assert_eq!(label[19], Phase::Landing);
        assert_ne!(label[20], Phase::Landing);
    }

    #[test]
    fn a_roll_ends_at_its_last_known_point() {
        let len = |gs: &[f64]| roll_len(gs.iter().copied(), 30.0);
        let nan = f64::NAN;
        assert_eq!(len(&[60.0, 60.0, nan, nan]), 2);
        assert_eq!(len(&[nan, 60.0, nan, 60.0, nan]), 4);
        assert_eq!(len(&[nan, nan]), 2, "at the runway end it was rolling");
        assert_eq!(len(&[60.0, 20.0, 60.0]), 1);
    }
}
