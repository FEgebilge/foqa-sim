use std::ops::Range;

use analyzer::{
    Flight, Phase, RunwayFix, Touchdown, Track, phase_labels, runs, touchdowns, wind_components,
};
use fdm_core::{SimTime, Unit};

use crate::expr::{Expr, Values};
use crate::profile::{Profile, ProfileField, Selection};
use crate::rule::{Aircraft, Level, Limits, Rule, Side, TdField, Trigger, Var};

const FEET_PER_METRE: f64 = 3.280_839_895;
/// A gate re-arms when more than this passes between two points of its
/// phases (about 5 s out of phase at 8 Hz); a shorter gap is a phase flicker,
/// not a new approach.
const REARM_AFTER_US: i64 = 5_000_000;
/// Two points at a level this close in time are one run, whatever lies
/// between: a value hovering at a threshold, recorded at 1-2 Hz and
/// interpolated, dips under it for a fraction of a second at a time and is
/// one exceedance, not several short ones or none (the exit hysteresis of
/// FDM event detection).
const MAX_DIP_US: i64 = 1_000_000;

/// One rule exceedance, with everything needed to explain it.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub rule: String,
    pub version: u32,
    pub title: String,
    pub level: Level,
    pub start: SimTime,
    pub end: SimTime,
    /// Where `value` was measured; `start` for a condition measure.
    pub peak: SimTime,
    /// Worst value of the measure: highest over an `above` limit, lowest
    /// under a `below` one. `None` for a condition measure.
    pub value: Option<f64>,
    /// The threshold of `level` that `value` crossed.
    pub threshold: Option<f64>,
    pub unit: Option<Unit>,
    /// The measure as written in the rule.
    pub measure: String,
    /// The event in words, from the rule's `detail`.
    pub detail: Option<String>,
}

/// Light or transport by the highest recorded gross weight; `None` without
/// weight data, in which case the flight is graded with the transport
/// rules, as with the transport phase boundaries.
pub fn aircraft(flight: &Flight) -> Option<Aircraft> {
    flight
        .series("weight.gross")
        .iter()
        .filter(|s| s.is_valid())
        .map(|s| s.value)
        .reduce(f64::max)
        .map(Aircraft::by_weight)
}

/// Runs the rules over an analysed flight graded as `selection` (class and
/// type profile); events in time order, the most severe first at equal times.
pub fn evaluate(
    rules: &[Rule],
    selection: &Selection,
    flight: &Flight,
    track: &Track,
) -> Vec<Event> {
    let phases = phase_labels(track, &selection.phases);
    let tds = touchdowns(flight, track);
    let class = selection.class.unwrap_or(Aircraft::Transport);
    let mut events: Vec<Event> = rules
        .iter()
        .filter(|r| r.aircraft.is_none_or(|a| a == class))
        .flat_map(|rule| {
            Evaluation {
                rule,
                profile: selection.profile,
                flight,
                track,
                phases: &phases,
            }
            .run(&tds)
        })
        .collect();
    events.sort_by_key(|e| (e.start, std::cmp::Reverse(e.level)));
    events
}

/// The values one expression evaluation sees.
struct Point<'a> {
    profile: Option<&'a Profile>,
    flight: &'a Flight,
    track: &'a Track,
    i: usize,
    td: Option<&'a Touchdown>,
}

impl Point<'_> {
    fn t(&self) -> SimTime {
        self.td.map_or(self.track.t[self.i], |td| td.t)
    }

    /// The wind now on the runway, as (headwind, crosswind from the right).
    fn wind(&self, fix: &RunwayFix) -> Option<(f64, f64)> {
        let t = self.t();
        let speed = self.flight.at("wind.speed", t)?;
        let from = self.flight.at("wind.dir", t)?;
        Some(wind_components(speed, from, fix.heading_deg))
            .filter(|(h, c)| h.is_finite() && c.is_finite())
    }
}

impl Values<Var> for Point<'_> {
    fn get(&self, var: Var) -> Option<f64> {
        let finite = |x: f64| x.is_finite().then_some(x);
        let t = self.t();
        match var {
            Var::Param(id) => self.flight.value(id, t).and_then(finite),
            Var::Vs => finite(self.track.vs_fpm[self.i]),
            Var::Height => finite(self.track.aal_ft[self.i]),
            Var::Ra => finite(self.track.ra_ft[self.i]),
            Var::Profile(field @ (ProfileField::Vref | ProfileField::Vapp)) => {
                let flight = self.flight;
                let speeds = self
                    .profile?
                    .ref_speeds(flight.at("weight.gross", t), flight.at("flap.te_angle", t))?;
                match field {
                    ProfileField::Vref => Some(speeds.vref),
                    _ => speeds.vapp,
                }
            }
            Var::Profile(field) => self
                .profile?
                .value(field, || self.flight.at("flap.te_angle", t)),
            Var::Td(field) => {
                let td = self.td?;
                let fix = td.runway.as_ref();
                let feet = |m: fn(&RunwayFix) -> f64| fix.map(|f| m(f) * FEET_PER_METRE);
                let flag = |c: fn(&RunwayFix) -> bool| fix.map(|f| f64::from(u8::from(c(f))));
                match field {
                    TdField::Sink => finite(td.sink_fpm),
                    TdField::PeakG => finite(td.peak_g),
                    TdField::Pitch => td.pitch_deg,
                    TdField::Roll => td.roll_deg,
                    TdField::Ias => td.ias_kt,
                    TdField::Bounce => Some(f64::from(u8::from(td.bounce))),
                    TdField::Distance => feet(|f| f.along_m),
                    TdField::Cross => feet(|f| f.cross_m),
                    TdField::Crosswind => self.wind(fix?).map(|(_, cross)| cross.abs()),
                    TdField::Tailwind => self.wind(fix?).map(|(head, _)| -head),
                    TdField::Lda => feet(|f| f.lda_m),
                    TdField::Tdz => feet(|f| f.tdz_m),
                    TdField::InTdz => flag(|f| f.in_tdz),
                    TdField::LongLanding => flag(|f| f.long_landing),
                    TdField::ShortLanding => flag(|f| f.short_landing),
                }
            }
        }
    }
}

/// Outcome of a single evaluation (gate or touchdown).
enum Settled {
    /// The precondition or the measure is unknown: try again later.
    Unknown,
    /// Decided, nothing exceeded.
    Quiet,
    Event(Event),
}

impl Settled {
    fn event(self) -> Option<Event> {
        match self {
            Self::Event(event) => Some(event),
            Self::Unknown | Self::Quiet => None,
        }
    }
}

struct Evaluation<'a> {
    rule: &'a Rule,
    profile: Option<&'a Profile>,
    flight: &'a Flight,
    track: &'a Track,
    phases: &'a [Phase],
}

impl<'a> Evaluation<'a> {
    fn run(&self, tds: &'a [Touchdown]) -> Vec<Event> {
        match &self.rule.trigger {
            Trigger::Touchdown => tds
                .iter()
                .filter_map(|td| {
                    self.settle(&self.point(self.track.index_at(td.t), Some(td)))
                        .event()
                })
                .collect(),
            Trigger::Gate(gate) => self.gated(gate),
            Trigger::Continuous { sustain_us } => self.continuous(*sustain_us),
        }
    }

    fn point(&self, i: usize, td: Option<&'a Touchdown>) -> Point<'a> {
        Point {
            profile: self.profile,
            flight: self.flight,
            track: self.track,
            i,
            td,
        }
    }

    fn applies(&self, i: usize) -> bool {
        self.rule.phases.contains(&self.phases[i])
    }

    /// Whether the precondition holds; `None` while it is unknown.
    fn admitted(&self, point: &Point) -> Option<bool> {
        self.rule
            .when
            .as_ref()
            .map_or(Some(true), |w| w.truth(point))
    }

    /// Events of a gate rule: one per crossing. The gate fires once its
    /// condition is true in the phases, at the first point where the rule
    /// can be decided, whether or not that raises an event. An unknown gate
    /// value changes nothing. It re-arms when the condition is known false or
    /// the phases did not apply for `REARM_AFTER_US`.
    fn gated(&self, gate: &Expr<Var>) -> Vec<Event> {
        let mut events = Vec::new();
        let mut fired = false;
        let mut last_applied = None;
        for i in 0..self.track.len() {
            let (t, point) = (self.track.t[i], self.point(i, None));
            let (open, applies) = (gate.truth(&point), self.applies(i));
            let away = last_applied.is_some_and(|a| t.us_since(a) > REARM_AFTER_US);
            if open == Some(false) || (applies && away) {
                fired = false;
            }
            if !applies {
                continue;
            }
            last_applied = Some(t);
            if fired || open != Some(true) {
                continue;
            }
            let settled = self.settle(&point);
            if !matches!(settled, Settled::Unknown) {
                fired = true;
                events.extend(settled.event());
            }
        }
        events
    }

    /// A single evaluation (gate or touchdown).
    fn settle(&self, point: &Point) -> Settled {
        match self.admitted(point) {
            None => return Settled::Unknown,
            Some(false) => return Settled::Quiet,
            Some(true) => {}
        }
        let Some(m) = self.rule.measure.eval(point) else {
            return Settled::Unknown;
        };
        let event = self.rule.limits.crossed(m).map(|(level, thr)| {
            let t = point.t();
            self.event(level, t, t, point, thr.map(|thr| (m, thr)))
        });
        event.map_or(Settled::Quiet, Settled::Event)
    }

    /// Exceedance episodes over the applicable points. An episode's level is
    /// the highest one held for at least `sustain_us` without a break, a run
    /// lasting from its first to its last point. Its value is the worst one
    /// among the points at exactly that level in the longest such run, so
    /// that value and threshold agree with the level.
    fn continuous(&self, sustain_us: i64) -> Vec<Event> {
        let measure: Vec<Option<f64>> = (0..self.track.len())
            .map(|i| {
                let point = self.point(i, None);
                if self.applies(i) && self.admitted(&point) == Some(true) {
                    self.rule.measure.eval(&point)
                } else {
                    None
                }
            })
            .collect();
        let limits = &self.rule.limits;
        if let Limits::Fixed(level) = limits {
            let held: Vec<_> = measure
                .iter()
                .map(|m| m.filter(|&x| x >= 0.5).map(|_| *level))
                .collect();
            return episodes(&self.track.t, &held, sustain_us)
                .map(|(level, span, _)| self.spanning(level, span, None))
                .collect();
        }
        [Side::Above, Side::Below]
            .into_iter()
            .flat_map(|side| {
                // Per point: the level crossed, the value and its threshold.
                let crossed: Vec<Option<(Level, f64, f64)>> = measure
                    .iter()
                    .map(|m| {
                        let m = (*m)?;
                        limits.level_at(side, m).map(|(level, thr)| (level, m, thr))
                    })
                    .collect();
                let levels: Vec<_> = crossed.iter().map(|c| c.map(|c| c.0)).collect();
                episodes(&self.track.t, &levels, sustain_us)
                    .map(|(level, span, run)| {
                        let worst = run
                            .filter_map(|k| crossed[k].map(|c| (k, c)))
                            .filter(|(_, c)| c.0 == level)
                            .max_by(|a, b| (side.sign() * a.1.1).total_cmp(&(side.sign() * b.1.1)));
                        self.spanning(
                            level,
                            span,
                            worst.map(|(k, (_, value, thr))| (k, value, thr)),
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// An event from the first to the last point of `span`; `worst` is
    /// the point where the value was measured, the value and the threshold
    /// it crossed.
    fn spanning(
        &self,
        level: Level,
        span: Range<usize>,
        worst: Option<(usize, f64, f64)>,
    ) -> Event {
        let (t0, t1) = (self.track.t[span.start], self.track.t[span.end - 1]);
        let peak = self.point(worst.map_or(span.start, |w| w.0), None);
        self.event(level, t0, t1, &peak, worst.map(|w| (w.1, w.2)))
    }

    /// `worst` is the value measured at `peak` and the threshold it crossed.
    fn event(
        &self,
        level: Level,
        start: SimTime,
        end: SimTime,
        peak: &Point,
        worst: Option<(f64, f64)>,
    ) -> Event {
        let unit = self.rule.limits.unit();
        let detail = self.rule.detail.as_ref();
        Event {
            rule: self.rule.id.clone(),
            version: self.rule.version,
            title: self.rule.title.clone(),
            level,
            start,
            end,
            peak: peak.t(),
            value: worst.map(|w| w.0),
            threshold: worst.map(|w| w.1),
            unit,
            measure: self.rule.measure_src.clone(),
            detail: detail.and_then(|d| d.render(unit, worst, |e| e.eval(peak))),
        }
    }
}

/// Exceedance episodes of `levels` (one per stretch of known levels, dips
/// of up to `MAX_DIP_US` bridged) as (level, whole episode, the longest run
/// at that level that satisfies `sustain_us`); episodes that sustain no
/// level are dropped.
fn episodes<'a>(
    t: &'a [SimTime],
    levels: &'a [Option<Level>],
    sustain_us: i64,
) -> impl Iterator<Item = (Level, Range<usize>, Range<usize>)> + 'a {
    bridged(t, 0..levels.len(), |i| levels[i].is_some()).filter_map(move |span| {
        Level::ALL.into_iter().rev().find_map(|level| {
            let run = longest_run(t, levels, &span, level, sustain_us)?;
            Some((level, span.clone(), run))
        })
    })
}

/// The longest run within `span` at or above `level` that lasts at least
/// `sustain_us` (any single point if it is 0); the first on a tie.
fn longest_run(
    t: &[SimTime],
    levels: &[Option<Level>],
    span: &Range<usize>,
    level: Level,
    sustain_us: i64,
) -> Option<Range<usize>> {
    bridged(t, span.clone(), |i| levels[i].is_some_and(|l| l >= level))
        .filter(|r| t[r.end - 1].us_since(t[r.start]) >= sustain_us)
        .reduce(|best, r| if r.len() > best.len() { r } else { best })
}

/// Runs of the points of `span` where `at` holds, those whose points are at
/// most `MAX_DIP_US` apart joined into one.
fn bridged(
    t: &[SimTime],
    span: Range<usize>,
    at: impl Fn(usize) -> bool,
) -> impl Iterator<Item = Range<usize>> {
    let mut joined: Vec<Range<usize>> = Vec::new();
    for r in runs(span.len(), |k| at(span.start + k)) {
        let r = span.start + r.start..span.start + r.end;
        match joined.last_mut() {
            Some(prev) if t[r.start].us_since(t[prev.end - 1]) <= MAX_DIP_US => prev.end = r.end,
            _ => joined.push(r),
        }
    }
    joined.into_iter()
}

#[cfg(test)]
mod tests {
    use fdm_core::{Lfl, Sample};
    use recorder::JournalWriter;

    use super::*;
    use crate::rule::load;

    const NAN: f64 = f64::NAN;

    /// A flight read back from a journal holding `samples`.
    fn flight_from_journal(samples: &[Sample]) -> Flight {
        let mut journal = JournalWriter::new(Vec::new(), &Lfl::V1).unwrap();
        samples.iter().for_each(|s| journal.sample(s).unwrap());
        Flight::read(&journal.into_inner()[..]).unwrap()
    }

    /// Events of one rule over a track whose points are all in `phases`
    /// (or in the given per-point phases).
    fn run(yaml: &str, track: &Track, phases: &[Phase]) -> Vec<Event> {
        let rules = load(yaml).unwrap();
        Evaluation {
            rule: &rules[0],
            profile: None,
            flight: &flight_from_journal(&[]),
            track,
            phases,
        }
        .run(&[])
    }

    fn continuous(sustain: &str, vs: &[f64]) -> Vec<Event> {
        let yaml = format!(
            "rule: x\nversion: 1\ntitle: x\ndomain: other\nphase: Approach\nmeasure: vs\n{sustain}\n\
             above: {{ low: 100 fpm, high: 1000 fpm }}"
        );
        let track = Track {
            vs_fpm: vs.to_vec(),
            ..Track::blank(vs.len())
        };
        run(&yaml, &track, &vec![Phase::Approach; vs.len()])
    }

    #[test]
    fn a_run_lasts_from_its_first_to_its_last_point() {
        // 16 points span 1.875 s, 17 span 2 s.
        assert_eq!(continuous("sustain: 2 s", &[500.0; 16]), []);
        assert_eq!(continuous("sustain: 2 s", &[500.0; 17]).len(), 1);
    }

    #[test]
    fn without_sustain_one_point_is_enough() {
        for sustain in ["", "sustain: 0", "sustain: 0 s"] {
            let events = continuous(sustain, &[0.0, 500.0, 0.0]);
            assert_eq!(events.len(), 1, "{sustain:?}");
            assert_eq!(events[0].start, events[0].end);
        }
    }

    #[test]
    fn value_and_threshold_agree_with_the_sustained_level() {
        // Low for 3 s with a 1 s spike over the High threshold.
        let mut vs = vec![300.0; 25];
        vs[8..16].fill(1600.0);
        vs[20] = 500.0;
        let [e] = &continuous("sustain: 2 s", &vs)[..] else {
            panic!("expected one event")
        };
        assert_eq!(
            (e.level, e.value, e.threshold),
            (Level::Low, Some(500.0), Some(100.0))
        );
    }

    #[test]
    fn a_value_hovering_at_the_threshold_is_one_event() {
        // 6 s over the low level with a 1/8 s dip under it every second.
        let hovering: Vec<f64> = (0..49).map(|i| if i % 8 == 4 { 50.0 } else { 150.0 }).collect();
        assert_eq!(continuous("sustain: 5 s", &hovering).len(), 1);
        // A dip longer than 1 s ends the run: two 3 s runs, neither 5 s.
        let mut split = vec![150.0; 24];
        split.extend([50.0; 9]);
        split.extend([150.0; 24]);
        assert_eq!(continuous("sustain: 5 s", &split), []);
        assert_eq!(continuous("sustain: 2 s", &split).len(), 2);
    }

    fn gate(phases: &[Phase], aal: &[f64], ra: &[f64], vs: &[f64]) -> Vec<Event> {
        let yaml = "rule: x\nversion: 1\ntitle: x\ndomain: other\nphase: Approach\ngate: height <= 500 ft\n\
                    when: ra > 100 ft\nmeasure: vs\nabove: { low: 100 fpm }";
        let track = Track {
            aal_ft: aal.to_vec(),
            ra_ft: ra.to_vec(),
            vs_fpm: vs.to_vec(),
            ..Track::blank(phases.len())
        };
        run(yaml, &track, phases)
    }

    const A: Phase = Phase::Approach;
    const G: Phase = Phase::GoAround;

    #[test]
    fn an_unknown_gate_value_keeps_the_gate_fired() {
        let events = gate(
            &[A; 5],
            &[600.0, 400.0, NAN, 400.0, 400.0],
            &[200.0; 5],
            &[500.0; 5],
        );
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn the_gate_re_arms_when_known_false() {
        let events = gate(
            &[A; 5],
            &[400.0, 600.0, 400.0, 400.0, 400.0],
            &[200.0; 5],
            &[500.0; 5],
        );
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn the_gate_re_arms_only_after_five_seconds_out_of_phase() {
        let around = |gap: usize| {
            let phases = [&[A, A][..], &vec![G; gap], &[A, A]].concat();
            let n = phases.len();
            gate(&phases, &vec![400.0; n], &vec![200.0; n], &vec![500.0; n]).len()
        };
        assert_eq!(around(1), 1);
        assert_eq!(around(39), 1);
        assert_eq!(around(40), 2);
    }

    #[test]
    fn the_gate_waits_for_a_known_when_and_measure() {
        let phases = [A; 4];
        let aal = [400.0; 4];
        let events = gate(&phases, &aal, &[NAN, NAN, 200.0, 200.0], &[500.0; 4]);
        assert_eq!(
            events.iter().map(|e| e.start).collect::<Vec<_>>(),
            [SimTime::from_micros(2 * Track::STEP_US)]
        );
        let events = gate(&phases, &aal, &[200.0; 4], &[NAN, NAN, 500.0, 500.0]);
        assert_eq!(events.len(), 1);
        // The first known evaluation is the firing, exceeded or not.
        let events = gate(&phases, &aal, &[200.0; 4], &[NAN, 50.0, 500.0, 500.0]);
        assert_eq!(events, []);
    }

    #[test]
    fn a_non_finite_parameter_is_unknown() {
        let id = Lfl::V1.by_name("roll").unwrap().id;
        let flight = flight_from_journal(&[Sample::new(SimTime::from_micros(0), id, NAN)]);
        let track = Track::blank(1);
        let point = Point {
            profile: None,
            flight: &flight,
            track: &track,
            i: 0,
            td: None,
        };
        assert_eq!(point.get(Var::Param(id)), None);
    }
}
