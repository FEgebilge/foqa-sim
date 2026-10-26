use std::ops::Range;
use std::str::FromStr;

use fdm_core::SimTime;

use crate::{Leg, TRACK_HZ, Track, runs};

/// Phases of flight per the CICTT (ICAO/CAST) taxonomy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Standing,
    Taxi,
    /// Takeoff roll to 35 ft above the runway.
    Takeoff,
    /// 35 ft to 1000 ft above the aerodrome (or the pattern altitude).
    InitialClimb,
    EnRoute,
    /// 1000 ft above the aerodrome (or pattern entry) to the flare.
    Approach,
    /// Flare to leaving the runway.
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub phase: Phase,
    pub start: SimTime,
    pub end: SimTime,
}

pub(crate) const STANDING_MAX_GS_KT: f64 = 2.0;
/// Below this ground speed the aircraft is taxiing, not rolling for takeoff
/// or landing.
pub(crate) const ROLL_MIN_GS_KT: f64 = 30.0;
pub(crate) const TAKEOFF_END_FT: f64 = 35.0;
// Deliberately equal: CICTT puts both the end of initial climb and the start
// of approach at 1000 ft AAL.
pub(crate) const INITIAL_CLIMB_END_FT: f64 = 1000.0;
pub(crate) const APPROACH_START_FT: f64 = 1000.0;
pub(crate) const FLARE_FT: f64 = 50.0;
/// Climb from a low point that turns an approach into a go-around.
pub(crate) const GO_AROUND_RISE_FT: f64 = 200.0;
/// A go-around climb ends at its highest point once the height has fallen
/// this far below it; smaller dips are noise or a levelling-off.
pub(crate) const GO_AROUND_DROP_FT: f64 = 50.0;
/// A gap in ground speed this long (2 s at the track rate) still belongs to
/// a roll; a longer one ends it.
const ROLL_MAX_GAP: usize = 2 * TRACK_HZ as usize;

/// The phase at every track point.
pub fn phase_labels(track: &Track) -> Vec<Phase> {
    let mut label: Vec<Phase> = track
        .gs_kt
        .iter()
        .map(|&gs| {
            if gs >= STANDING_MAX_GS_KT {
                Phase::Taxi
            } else {
                Phase::Standing
            }
        })
        .collect();
    for leg in &track.legs {
        label_leg(track, leg, &mut label);
    }
    label
}

/// Contiguous phase spans covering the whole track.
pub fn phases(track: &Track) -> Vec<Span> {
    let mut spans: Vec<Span> = Vec::new();
    for (i, phase) in phase_labels(track).into_iter().enumerate() {
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
                });
            }
        }
    }
    spans
}

/// Height above the runway: calibrated RA where valid, else AAL.
fn runway_height(track: &Track, i: usize) -> f64 {
    Some(track.ra_ft[i])
        .filter(|r| r.is_finite())
        .unwrap_or(track.aal_ft[i])
}

fn label_leg(track: &Track, leg: &Leg, label: &mut [Phase]) {
    let Range { start: a, end: b } = leg.airborne;
    let (gs, aal) = (&track.gs_kt, &track.aal_ft);
    let taxiing = |p: Phase| matches!(p, Phase::Standing | Phase::Taxi);

    let before = (0..a)
        .rev()
        .take_while(|&i| track.on_ground[i] && taxiing(label[i]));
    let roll = a - roll_len(before.map(|i| gs[i]));
    let mut i = a;
    while i < b && runway_height(track, i) < TAKEOFF_END_FT {
        i += 1;
    }
    label[roll..i].fill(Phase::Takeoff);
    let mut j = i;
    while j < b && j <= leg.peak && aal[j] < INITIAL_CLIMB_END_FT {
        j += 1;
    }
    label[i..j].fill(Phase::InitialClimb);
    label[j..b].fill(Phase::EnRoute);

    for low in runs(b, |k| k >= j && aal[k] < APPROACH_START_FT) {
        let lands = low.end == b;
        label_low(track, low, lands, label);
    }

    let after = (b..label.len()).take_while(|&i| track.on_ground[i]);
    let rollout = roll_len(after.map(|i| gs[i]));
    label[b..b + rollout].fill(Phase::Landing);
}

/// A stretch below 1000 ft AAL after the initial climb: approaches,
/// go-arounds and, if the leg ends in it, the landing.
fn label_low(track: &Track, low: Range<usize>, lands: bool, label: &mut [Phase]) {
    let aal = &track.aal_ft;
    let mut start = low.start;
    while start < low.end {
        let mut bottom = start;
        let climb = (start..low.end).find(|&i| {
            if aal[i] < aal[bottom] {
                bottom = i;
            }
            aal[i] >= aal[bottom] + GO_AROUND_RISE_FT
        });
        match climb {
            Some(top) => {
                let mut peak = top;
                for i in top..low.end {
                    if aal[i] > aal[peak] {
                        peak = i;
                    } else if aal[i] <= aal[peak] - GO_AROUND_DROP_FT {
                        break;
                    }
                }
                label[start..bottom].fill(Phase::Approach);
                label[bottom..=peak].fill(Phase::GoAround);
                start = peak + 1;
            }
            None if lands => {
                let flare = (start..low.end)
                    .rev()
                    .find(|&i| runway_height(track, i) >= FLARE_FT)
                    .map_or(start, |i| i + 1);
                label[start..flare].fill(Phase::Approach);
                label[flare..low.end].fill(Phase::Landing);
                return;
            }
            // A shallow dip below 1000 ft that climbs back: still en route.
            None => return,
        }
    }
}

/// Points of a roll along `gs_kt`, which runs away from the runway end:
/// those at roll speed plus any gap (`NaN`) of up to `ROLL_MAX_GAP` points
/// between them, so a dropout does not split the roll but never stretches it.
fn roll_len(gs_kt: impl Iterator<Item = f64>) -> usize {
    let mut gap = 0;
    let mut len = 0;
    for (n, gs) in gs_kt.enumerate() {
        if gs.is_nan() {
            gap += 1;
            if gap > ROLL_MAX_GAP {
                break;
            }
        } else if gs >= ROLL_MIN_GS_KT {
            gap = 0;
        } else {
            break;
        }
        len = n + 1;
    }
    len
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn a_dip_in_the_go_around_climb_does_not_end_it() {
        // Down to 100 ft, up through 300 ft (go-around), a 10 ft dip, then on
        // up: the whole climb is the go-around, not an approach again.
        let aal: Vec<f64> = [600.0, 300.0, 100.0, 250.0, 350.0, 340.0, 360.0, 380.0].into();
        let n = aal.len();
        let t = track(aal, vec![80.0; n], vec![false; n], vec![]);
        let mut label = vec![Phase::Approach; n];
        label_low(&t, 0..n, true, &mut label);
        assert_eq!(label[1], Phase::Approach);
        assert!(
            label[2..].iter().all(|&p| p == Phase::GoAround),
            "{label:?}"
        );
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
        let label = phase_labels(&t);
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
        assert_eq!(phase_labels(&t)[0], Phase::Takeoff);
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
        let label = phase_labels(&t);
        // Ground from 10; finite gs to 19, then 16 NaN points allowed.
        assert_eq!(label[10 + 10 + 15], Phase::Landing);
        assert_ne!(label[10 + 10 + 16], Phase::Landing);
    }
}
