//! What the in-sim panel shows live while a flight is recorded
//! (`FOQA_SIM_LIVE`, `docs/ingame-panel.md`): the recorded frames in
//! engineering units, the analyzer's live track and phase, and the
//! stabilised-approach gates checked against the rule set's thresholds and
//! the type profile, so the panel and the debrief judge alike.

use std::sync::Arc;

use analyzer::{LiveInput, LivePoint, LiveTrack, Phase};
use debrief::Station;
use fdm_core::{AircraftId, Lfl, SimTime, Ssm};
use recorder::{Acquisition, Conditioned, SimEvent, Telemetry};
use rules::expr::Values;
use rules::{
    Aircraft, FLAP_TOLERANCE_DEG, Profile, ProfileField, Rule, Selection, Side, Strike,
    StrikeLimits, StrikeWatch, Var,
};
use serde::Serialize;

use crate::panel::VERSION;

/// Stabilised-approach gates, ft above the aerodrome: FSF ALAR's 1000 ft
/// (IMC) and 500 ft (VMC), both with the same criteria.
const GATES_FT: [u32; 2] = [1000, 500];
/// The rules whose thresholds the gate checks apply.
const SINK_RULE: &str = "unstable-sink-500";
const BANK_RULE: &str = "unstable-bank-500";
const SPEED_RULE: &str = "flap-overspeed";
/// How long after contact the impact's peak load is looked for: the
/// gear's first compression and any rebound.
const IMPACT_WINDOW_S: f64 = 1.0;
/// Stopped after a landing: ground speed below this, held `STOP_HOLD_S`,
/// so a taxiing aircraft or a pause in a slow rollout does not count.
const STOP_KT: f64 = 2.0;
const STOP_HOLD_S: f64 = 3.0;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Live {
    v: u32,
    t_s: f64,
    phase: &'static str,
    on_ground: bool,
    aal_ft: Option<f64>,
    ias_kt: Option<f64>,
    vs_fpm: Option<f64>,
    flap_deg: Option<f64>,
    vfe_kt: Option<f64>,
    bank_deg: Option<f64>,
    gates: Vec<Gate>,
}

impl Live {
    pub fn phase(&self) -> &'static str {
        self.phase
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct Gate {
    at_ft: u32,
    state: GateState,
    checks: Vec<Check>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
enum GateState {
    Ahead,
    Stable,
    Unstable,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct Check {
    name: &'static str,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    limit: Option<f64>,
    unit: &'static str,
}

/// A contact with the ground as the live frames measure it, reported
/// `IMPACT_WINDOW_S` after it: enough to tell a crash at once, where the
/// landing's analysis waits for the full-rate window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Impact {
    /// Sim seconds since the flight's first frame, as the debrief counts.
    pub t_s: f64,
    /// Descent rate on the last airborne frame (the sim's), fpm.
    pub sink_fpm: f64,
    /// Highest normal load from contact through the window, g.
    pub peak_g: f64,
}

/// One flight followed frame by frame.
pub struct Monitor {
    station: Arc<Station>,
    acquisition: Acquisition,
    track: LiveTrack,
    frame: Option<Conditioned>,
    t0: Option<f64>,
    aircraft: Option<AircraftId>,
    /// Heaviest gross weight so far: the class when the type has no profile.
    weight_lb: f64,
    /// Each gate's checks as they stood when it was passed, this approach.
    passed: [Option<Vec<Check>>; 2],
    /// Air/ground and descent rate on the previous frame.
    last: Option<(bool, f64)>,
    /// The contact being measured, then the one measured, until taken.
    impact: Option<(Impact, bool)>,
    /// Wing, propeller or nose strikes on the ground (`rules::strike`).
    strikes: StrikeWatch,
    /// The latest strike, until taken.
    strike: Option<Strike>,
    /// Since a touchdown, on the ground: when the speed fell below
    /// `STOP_KT`, and whether the stop was told; `None` in the air.
    rollout: Option<(Option<f64>, bool)>,
    /// When the aircraft came to a stop after a landing, until taken.
    stopped: Option<f64>,
}

impl Monitor {
    pub fn new(station: Arc<Station>) -> Self {
        Self {
            station,
            acquisition: Acquisition::new(&Lfl::V1),
            track: LiveTrack::default(),
            frame: None,
            t0: None,
            aircraft: None,
            weight_lb: f64::NAN,
            passed: [None, None],
            last: None,
            impact: None,
            strikes: StrikeWatch::default(),
            strike: None,
            rollout: None,
            stopped: None,
        }
    }

    /// When the aircraft came to a stop after its latest landing, once:
    /// what ends a challenge attempt without the pilot pressing Finish.
    /// Times as `Impact::t_s`.
    pub fn take_stopped(&mut self) -> Option<f64> {
        self.stopped.take()
    }

    /// The rollout after a touchdown, until the aircraft stops.
    fn rolling(&mut self, t_s: f64, on_ground: Option<bool>, gs_kt: f64) {
        match on_ground {
            Some(false) => self.rollout = None,
            Some(true) => {
                if let Some((slow_since, told)) = self.rollout.as_mut().filter(|(_, told)| !told) {
                    if gs_kt < STOP_KT {
                        let since = *slow_since.get_or_insert(t_s);
                        if t_s - since >= STOP_HOLD_S {
                            *told = true;
                            self.stopped = Some(t_s);
                        }
                    } else {
                        *slow_since = None;
                    }
                }
            }
            None => {}
        }
    }

    /// The latest contact once measured, once.
    pub fn take_impact(&mut self) -> Option<Impact> {
        match self.impact {
            Some((impact, true)) => {
                self.impact = None;
                Some(impact)
            }
            _ => None,
        }
    }

    /// The latest ground strike, once: a wing, the propeller or the nose
    /// on the runway after a touchdown, which MSFS crashes on but tells
    /// SimConnect nothing of. Times as `Impact::t_s`.
    pub fn take_strike(&mut self) -> Option<Strike> {
        self.strike.take()
    }

    /// Air-to-ground transitions and the load that follows them.
    fn contact(&mut self, t_s: f64, on_ground: Option<bool>, vs_fpm: f64, nz_g: f64) {
        if let Some((impact, done)) = self.impact.as_mut().filter(|(_, done)| !done) {
            impact.peak_g = impact.peak_g.max(nz_g);
            *done = t_s - impact.t_s >= IMPACT_WINDOW_S;
        }
        let Some(on_ground) = on_ground else { return };
        if let Some((false, last_vs)) = self.last.filter(|_| on_ground) {
            let impact = Impact {
                t_s,
                sink_fpm: -last_vs,
                peak_g: nz_g,
            };
            self.impact = Some((impact, false));
            self.rollout = Some((None, false));
        }
        self.last = Some((on_ground, vs_fpm));
    }

    /// Follows one recorded item.
    pub fn record(&mut self, item: &Telemetry) {
        match item {
            Telemetry::Event(SimEvent::Aircraft(id)) => self.aircraft = Some(id.clone()),
            Telemetry::Event(SimEvent::Airport(airport)) => self.track.airport(airport),
            Telemetry::Frame(raw) => {
                if let Some(frame) = self.acquisition.condition(raw) {
                    self.step(frame);
                }
            }
            _ => {}
        }
    }

    fn step(&mut self, frame: Conditioned) {
        let t0 = *self.t0.get_or_insert(frame.sim_time_s);
        let p = |name| param(&frame, name).unwrap_or(f64::NAN);
        self.weight_lb = self.weight_lb.max(p("weight.gross"));
        let on_ground = param(&frame, "air_ground").map(|v| v >= 0.5);
        let t_s = frame.sim_time_s - t0;
        self.contact(t_s, on_ground, p("sim.vs"), p("accel.normal"));
        self.rolling(t_s, on_ground, p("groundspeed"));
        let limits = StrikeLimits::of(self.selection().class);
        let struck = (self.strikes).step(limits, t_s, on_ground, p("roll"), p("pitch"));
        self.strike = struck.or(self.strike);
        // The boundaries of the aircraft as known so far.
        let phases = self.selection().phases;
        let point = self.track.step(
            &LiveInput {
                t: SimTime::from_secs_f64(t_s),
                on_ground,
                alt_ft: p("alt.pressure"),
                gs_kt: p("groundspeed"),
                ra_ft: p("alt.radio"),
                nz_g: p("accel.normal"),
                pitch_deg: p("pitch"),
                roll_deg: p("roll"),
                pos: param(&frame, "pos.lat").zip(param(&frame, "pos.lon")),
            },
            &phases,
        );
        self.frame = Some(frame);
        if !approach(&point) {
            self.passed = [None, None];
            return;
        }
        let due: Vec<usize> = (0..GATES_FT.len())
            .filter(|&i| self.passed[i].is_none())
            .filter(|&i| !point.on_ground && point.aal_ft <= f64::from(GATES_FT[i]))
            .collect();
        if !due.is_empty() {
            let checks = self.checks();
            due.into_iter()
                .for_each(|i| self.passed[i] = Some(checks.clone()));
        }
    }

    /// The panel's live values at the latest frame.
    pub fn live(&self) -> Option<Live> {
        let (frame, point) = (self.frame.as_ref()?, self.track.last()?);
        let at = self.at(frame, point);
        let gates = if approach(&point) {
            let gate = |(&at_ft, passed): (&u32, &Option<Vec<Check>>)| match passed {
                Some(checks) => Gate {
                    at_ft,
                    state: if checks.iter().all(|c| c.ok) {
                        GateState::Stable
                    } else {
                        GateState::Unstable
                    },
                    checks: checks.clone(),
                },
                None => Gate {
                    at_ft,
                    state: GateState::Ahead,
                    checks: self.checks(),
                },
            };
            GATES_FT.iter().zip(&self.passed).map(gate).collect()
        } else {
            Vec::new()
        };
        Some(Live {
            v: VERSION,
            t_s: frame.sim_time_s - self.t0?,
            phase: point.phase.name(),
            on_ground: point.on_ground,
            aal_ft: finite(point.aal_ft),
            ias_kt: param(frame, "ias"),
            vs_fpm: finite(point.vs_fpm),
            flap_deg: param(frame, "flap.te_angle"),
            vfe_kt: at.get(Var::Profile(ProfileField::Vfe)),
            bank_deg: param(frame, "roll"),
            gates,
        })
    }

    /// The aircraft as the debrief will grade it: its type profile, else
    /// the class by the sim's facts or the heaviest weight so far.
    fn selection(&self) -> Selection<'_> {
        let weight = finite(self.weight_lb).map(Aircraft::by_weight);
        self.station
            .profiles()
            .identify(self.aircraft.as_ref(), || weight)
    }

    fn at<'a>(&'a self, frame: &'a Conditioned, point: LivePoint) -> At<'a> {
        let selection = self.selection();
        At {
            frame,
            point,
            profile: selection.profile,
            class: selection.class,
        }
    }

    /// The gate checks at the latest frame: sink, speed, bank, flaps. A
    /// check is left out where its value is unknown or its rule does not
    /// apply.
    fn checks(&self) -> Vec<Check> {
        let (Some(frame), Some(point)) = (&self.frame, self.track.last()) else {
            return Vec::new();
        };
        let at = self.at(frame, point);
        let rule = |id: &str| {
            let applies = |r: &&Rule| r.aircraft.is_none() || r.aircraft == at.class;
            self.station
                .rules()
                .iter()
                .find(|r| r.id == id)
                .filter(applies)
        };
        let gauge = |name, id| {
            let rule = rule(id)?;
            let (value, ok) = at.check(rule)?;
            Some(Check {
                name,
                ok,
                value: Some(value),
                limit: rule.limits.first(Side::Above),
                unit: rule.limits.unit().map_or("", |u| u.symbol()),
            })
        };
        let unit = |name| Lfl::V1.by_name(name).map_or("", |s| s.unit.symbol());
        // IAS against VFE: the flap-overspeed rule's measure is the excess.
        let speed = param(frame, "ias").map(|ias| {
            let rule = rule(SPEED_RULE);
            let excess = rule.and_then(|r| at.check(r));
            let vfe = at.get(Var::Profile(ProfileField::Vfe));
            let over = rule.and_then(|r| r.limits.first(Side::Above));
            Check {
                name: "speed",
                ok: excess.is_none_or(|(_, ok)| ok),
                value: Some(ias),
                limit: vfe.zip(over).map(|(vfe, over)| vfe + over),
                unit: unit("ias"),
            }
        });
        let flaps = param(frame, "flap.te_angle").map(|deg| Check {
            name: "flaps",
            ok: deg > FLAP_TOLERANCE_DEG,
            value: Some(deg),
            limit: None,
            unit: unit("flap.te_angle"),
        });
        [
            gauge("sink", SINK_RULE),
            speed,
            gauge("bank", BANK_RULE),
            flaps,
        ]
        .into_iter()
        .flatten()
        .collect()
    }
}

/// Where the gates are shown and passed.
fn approach(point: &LivePoint) -> bool {
    matches!(point.phase, Phase::Approach | Phase::Landing)
}

fn finite(x: f64) -> Option<f64> {
    x.is_finite().then_some(x)
}

/// A parameter of a frame, if valid.
fn param(frame: &Conditioned, name: &str) -> Option<f64> {
    let id = Lfl::V1.by_name(name)?.id;
    let (value, ssm) = (*frame.values.get(usize::from(id.0))?)?;
    finite(value).filter(|_| ssm == Ssm::Normal)
}

/// The values a rule reads at the latest frame, as the rule engine reads
/// them from the analysed flight.
struct At<'a> {
    frame: &'a Conditioned,
    point: LivePoint,
    profile: Option<&'a Profile>,
    class: Option<Aircraft>,
}

impl At<'_> {
    /// The rule's measure and whether it stays within the rule's limits;
    /// `None` where the rule does not apply (`when`) or the measure is unknown.
    fn check(&self, rule: &Rule) -> Option<(f64, bool)> {
        if !rule.when.as_ref().map_or(Some(true), |w| w.truth(self))? {
            return None;
        }
        let m = rule.measure.eval(self)?;
        Some((m, rule.limits.crossed(m).is_none()))
    }
}

impl Values<Var> for At<'_> {
    fn get(&self, var: Var) -> Option<f64> {
        match var {
            Var::Param(id) => {
                let name = Lfl::V1.get(id)?.name;
                param(self.frame, name)
            }
            Var::Vs => finite(self.point.vs_fpm),
            Var::Height => finite(self.point.aal_ft),
            Var::Ra => finite(self.point.ra_ft),
            Var::Td(_) | Var::Takeoff(_) => None,
            Var::Profile(field) => self
                .profile?
                .value(field, || param(self.frame, "flap.te_angle")),
        }
    }
}

#[cfg(test)]
mod tests {
    use fdm_core::AircraftId;
    use serde_json::{Value, json};

    use super::*;

    fn station() -> Arc<Station> {
        Arc::new(Station::core().unwrap())
    }

    /// A raw frame with these SimVar values, the rest missing.
    fn frame(acq: &Acquisition, t: f64, set: &[(&str, f64)]) -> Telemetry {
        let mut raw = vec![f64::NAN; acq.columns().len()];
        raw[0] = t;
        for &(simvar, v) in set {
            let i = acq.columns().iter().position(|c| c.simvar == simvar);
            raw[i.unwrap()] = v;
        }
        Telemetry::Frame(raw)
    }

    /// A C172 descending at `sink` fpm from 1300 ft (aerodrome at 0 ft)
    /// to `down_to` ft, banked `bank` deg (FDR sign) at `ias` kt with
    /// flaps `flaps`, after a takeoff and a climb that topped at 1300 ft.
    fn c172(sink: f64, bank: f64, ias: f64, flaps: f64, down_to: f64) -> Monitor {
        let mut m = Monitor::new(station());
        let acq = Acquisition::new(&Lfl::V1);
        let id = AircraftId {
            title: "C172SP".into(),
            atc_model: "ATCCOM.AC_MODEL C172.0.text".into(),
            atc_type: String::new(),
            atc_id: String::new(),
            facts: Default::default(),
        };
        m.record(&Telemetry::Event(SimEvent::Aircraft(id)));
        let ground = [("SIM ON GROUND", 1.0), ("PRESSURE ALTITUDE", 0.0)];
        m.record(&frame(&acq, 0.0, &ground));
        let mut t = 0.0;
        let mut alt = 0.0;
        let mut fly = |m: &mut Monitor, alt_ft: f64, vs: f64| {
            t += 0.125;
            m.record(&frame(
                &acq,
                t,
                &[
                    ("SIM ON GROUND", 0.0),
                    ("PRESSURE ALTITUDE", alt_ft),
                    ("GROUND VELOCITY", ias),
                    ("AIRSPEED INDICATED", ias),
                    ("G FORCE", 1.0),
                    ("PLANE PITCH DEGREES", 0.0),
                    // MSFS bank is left-wing-down positive.
                    ("PLANE BANK DEGREES", -bank),
                    ("TRAILING EDGE FLAPS LEFT ANGLE", flaps),
                    ("VERTICAL SPEED", vs),
                ],
            ));
        };
        while alt < 1300.0 {
            alt += 10.0;
            fly(&mut m, alt, 4800.0);
        }
        while alt > down_to {
            alt -= sink / 60.0 / 8.0;
            fly(&mut m, alt, -sink);
        }
        m
    }

    fn json(m: &Monitor) -> Value {
        serde_json::to_value(m.live().unwrap()).unwrap()
    }

    #[test]
    fn gates_are_ahead_until_passed_and_frozen_after() {
        let v = json(&c172(600.0, 2.0, 70.0, 20.0, 700.0));
        assert_eq!(v["phase"], "Approach");
        assert_eq!(v["v"], 2);
        let gates = v["gates"].as_array().unwrap();
        assert_eq!(gates.len(), 2);
        assert_eq!(
            (&gates[0]["at_ft"], &gates[0]["state"]),
            (&json!(1000), &json!("stable"))
        );
        assert_eq!(
            (&gates[1]["at_ft"], &gates[1]["state"]),
            (&json!(500), &json!("ahead"))
        );
        // The C172's VFE beyond 10 deg of flap is 85 kt (profiles/C172.yaml).
        assert_eq!(v["vfe_kt"], 85.0);
        let names: Vec<_> = gates[0]["checks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["name"].clone())
            .collect();
        assert_eq!(names, ["sink", "speed", "bank", "flaps"]);
        let speed = &gates[0]["checks"][1];
        assert_eq!(
            speed,
            &json!({"name": "speed", "ok": true, "value": 70.0, "limit": 85.0, "unit": "kt"})
        );
    }

    #[test]
    fn an_unstable_gate_names_the_checks_that_failed() {
        // 1300 fpm, banked 10 deg right, 95 kt at flaps 20.
        let v = json(&c172(1300.0, 10.0, 95.0, 20.0, 400.0));
        assert_eq!(v["bank_deg"], 10.0);
        let gate = &v["gates"][1];
        assert_eq!(
            (&gate["at_ft"], &gate["state"]),
            (&json!(500), &json!("unstable"))
        );
        let failed: Vec<_> = gate["checks"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| c["ok"] == false)
            .map(|c| (c["name"].clone(), c["limit"].clone()))
            .collect();
        // Limits from rules/core.yaml: 1000 fpm, VFE + 0 kt, 7 deg.
        assert_eq!(
            failed,
            [
                (json!("sink"), json!(1000.0)),
                (json!("speed"), json!(85.0)),
                (json!("bank"), json!(7.0))
            ]
        );
    }

    #[test]
    fn flaps_up_fail_the_flaps_check_and_leave_speed_without_a_limit() {
        let v = json(&c172(600.0, 0.0, 70.0, 0.0, 700.0));
        let checks = &v["gates"][0]["checks"];
        assert_eq!(
            checks[1],
            json!({"name": "speed", "ok": true, "value": 70.0, "unit": "kt"})
        );
        assert_eq!(
            checks[3],
            json!({"name": "flaps", "ok": false, "value": 0.0, "unit": "deg"})
        );
        assert_eq!(v["vfe_kt"], Value::Null);
    }

    #[test]
    fn no_gates_outside_the_approach() {
        let v = json(&c172(600.0, 0.0, 70.0, 0.0, 1300.0));
        assert_eq!(v["phase"], "EnRoute");
        assert_eq!(v["gates"], json!([]));
        assert!(Monitor::new(station()).live().is_none());
    }

    /// Contacts at `sink` fpm with `g` on the first ground frame, then 5 g
    /// half a second later; frames at 10 Hz.
    fn contact(sink: f64, g: f64) -> Vec<Option<Impact>> {
        let (mut m, acq) = (Monitor::new(station()), Acquisition::new(&Lfl::V1));
        let mut taken = Vec::new();
        for i in 0..30 {
            let t = f64::from(i) / 10.0;
            let ground = f64::from(u8::from(i >= 10));
            let nz = match i {
                10 => g,
                15 => 5.0,
                _ => 1.0,
            };
            let set = [
                ("SIM ON GROUND", ground),
                ("VERTICAL SPEED", -sink),
                ("G FORCE", nz),
            ];
            m.record(&frame(&acq, t, &set));
            taken.push(m.take_impact());
        }
        taken
    }

    /// A landing at 10 Hz: airborne for 1 s, then on the ground at `gs(t)`
    /// kt; the stops taken, with their times.
    fn stops(gs: impl Fn(f64) -> f64, until_s: f64) -> Vec<f64> {
        let (mut m, acq) = (Monitor::new(station()), Acquisition::new(&Lfl::V1));
        let mut stops = Vec::new();
        for i in 0..=(until_s * 10.0) as u32 {
            let t = f64::from(i) / 10.0;
            let ground = f64::from(u8::from(t >= 1.0));
            let set = [
                ("SIM ON GROUND", ground),
                ("GROUND VELOCITY", gs(t)),
                ("G FORCE", 1.0),
            ];
            m.record(&frame(&acq, t, &set));
            stops.extend(m.take_stopped());
        }
        stops
    }

    #[test]
    fn a_stop_after_a_landing_is_told_once_after_the_hold() {
        // 60 kt at touchdown, 4 kt/s down: below 2 kt from 15.5 s, held 3 s.
        let stops = stops(|t| (60.0 - 4.0 * (t - 1.0)).clamp(0.0, 60.0), 30.0);
        assert_eq!(stops.len(), 1);
        assert!((stops[0] - 18.5).abs() < 0.15, "{stops:?}");
    }

    #[test]
    fn a_short_slow_moment_on_the_rollout_is_no_stop() {
        // Below 2 kt for 2 s only, then taxiing on.
        assert!(stops(|t| if (5.0..7.0).contains(&t) { 1.0 } else { 10.0 }, 20.0).is_empty());
    }

    #[test]
    fn an_impact_is_measured_a_second_after_contact_and_taken_once() {
        let taken = contact(9000.0, 2.0);
        let impacts: Vec<_> = taken
            .iter()
            .enumerate()
            .filter_map(|(i, x)| x.map(|x| (i, x)))
            .collect();
        assert_eq!(impacts.len(), 1);
        let (at, impact) = impacts[0];
        assert_eq!(at, 20);
        assert_eq!(impact.t_s, 1.0);
        assert_eq!(impact.sink_fpm, 9000.0);
        assert_eq!(impact.peak_g, 5.0);
    }

    /// A C172 airborne for 1 s, then on the ground at 10 Hz with the
    /// attitude `at(t)` = (roll, pitch) in FDR signs; the strikes taken.
    fn rollout(at: impl Fn(f64) -> (f64, f64)) -> Vec<Strike> {
        let (mut m, acq) = (Monitor::new(station()), Acquisition::new(&Lfl::V1));
        let id = AircraftId {
            title: "C172SP".into(),
            atc_model: "ATCCOM.AC_MODEL C172.0.text".into(),
            atc_type: String::new(),
            atc_id: String::new(),
            facts: Default::default(),
        };
        m.record(&Telemetry::Event(SimEvent::Aircraft(id)));
        let mut taken = Vec::new();
        for i in 0..60 {
            let t = f64::from(i) / 10.0;
            let (roll, pitch) = at(t);
            let set = [
                ("SIM ON GROUND", f64::from(u8::from(t >= 1.0))),
                ("VERTICAL SPEED", -250.0),
                ("G FORCE", 1.0),
                // MSFS signs: bank left-wing-down and pitch nose-down positive.
                ("PLANE BANK DEGREES", -roll),
                ("PLANE PITCH DEGREES", -pitch),
            ];
            m.record(&frame(&acq, t, &set));
            taken.extend(m.take_strike());
        }
        taken
    }

    #[test]
    fn a_normal_rollout_strikes_nothing() {
        assert!(rollout(|t| (1.5 * (t * 4.0).sin(), 6.0 - t)).is_empty());
    }

    #[test]
    fn a_ground_loop_onto_a_wingtip_is_a_strike_once_held() {
        // The 2026-10-30 LTBJ 34L C172: -25 deg roll, -17 deg pitch.
        let s = rollout(|t| if t >= 3.0 { (-25.0, -17.0) } else { (0.0, 2.0) });
        assert_eq!(s.len(), 1, "{s:?}");
        assert!((3.39..3.51).contains(&s[0].t_s), "{:?}", s[0]);
        assert_eq!(s[0].reason(), "wing and prop/nose strike");
    }

    #[test]
    fn a_single_frame_beyond_the_limits_is_noise() {
        let s = rollout(|t| {
            if (t - 3.0).abs() < 0.05 {
                (30.0, -20.0)
            } else {
                (0.0, 2.0)
            }
        });
        assert!(s.is_empty(), "{s:?}");
    }

    #[test]
    fn a_touchdown_on_a_wingtip_is_a_strike_at_contact() {
        let s = rollout(|t| if t < 1.05 { (14.0, 4.0) } else { (2.0, 3.0) });
        assert_eq!(s.len(), 1, "{s:?}");
        // Monitor time counts from the flight's first frame, t = 0.
        assert_eq!((s[0].t_s, s[0].reason()), (1.0, "wing strike"));
    }

    #[test]
    fn a_flight_starting_on_the_ground_makes_no_impact() {
        let (mut m, acq) = (Monitor::new(station()), Acquisition::new(&Lfl::V1));
        for i in 0..30 {
            let set = [
                ("SIM ON GROUND", 1.0),
                ("VERTICAL SPEED", 0.0),
                ("G FORCE", 1.0),
            ];
            m.record(&frame(&acq, f64::from(i) / 10.0, &set));
            assert_eq!(m.take_impact(), None);
        }
    }
}
