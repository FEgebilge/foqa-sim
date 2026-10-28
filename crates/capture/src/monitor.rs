//! What the in-sim panel shows live while a flight is recorded
//! (`FOQA_SIM_LIVE`, `docs/ingame-panel.md`): the recorded frames in
//! engineering units, the analyzer's live track and phase, and the
//! stabilised-approach gates checked against the rule set's thresholds and
//! the type profile, so the panel and the debrief judge alike.

use std::sync::Arc;

use analyzer::{LiveInput, LivePoint, LiveTrack, Phase};
use debrief::Station;
use fdm_core::{Lfl, SimTime, Ssm};
use recorder::{Acquisition, Conditioned, SimEvent, Telemetry};
use rules::expr::Values;
use rules::{Aircraft, FLAP_TOLERANCE_DEG, Profile, ProfileField, Rule, Selection, Side, Var};
use serde::Serialize;

use crate::panel::VERSION;

/// Stabilised-approach gates, ft above the aerodrome: FSF ALAR's 1000 ft
/// (IMC) and 500 ft (VMC), both with the same criteria.
const GATES_FT: [u32; 2] = [1000, 500];
/// The rules whose thresholds the gate checks apply.
const SINK_RULE: &str = "unstable-sink-500";
const BANK_RULE: &str = "unstable-bank-500";
const SPEED_RULE: &str = "flap-overspeed";

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

/// One flight followed frame by frame.
pub struct Monitor {
    station: Arc<Station>,
    acquisition: Acquisition,
    track: LiveTrack,
    frame: Option<Conditioned>,
    t0: Option<f64>,
    icao: Option<String>,
    /// Heaviest gross weight so far: the class when the type has no profile.
    weight_lb: f64,
    /// Each gate's checks as they stood when it was passed, this approach.
    passed: [Option<Vec<Check>>; 2],
}

impl Monitor {
    pub fn new(station: Arc<Station>) -> Self {
        Self {
            station,
            acquisition: Acquisition::new(&Lfl::V1),
            track: LiveTrack::default(),
            frame: None,
            t0: None,
            icao: None,
            weight_lb: f64::NAN,
            passed: [None, None],
        }
    }

    /// Follows one recorded item.
    pub fn record(&mut self, item: &Telemetry) {
        match item {
            Telemetry::Event(SimEvent::Aircraft(id)) => self.icao = id.icao_type(),
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
        // The boundaries of the aircraft as known so far.
        let phases = self.selection().phases;
        let point = self.track.step(
            &LiveInput {
                t: SimTime::from_secs_f64(frame.sim_time_s - t0),
                on_ground: param(&frame, "air_ground").map(|v| v >= 0.5),
                alt_ft: p("alt.pressure"),
                gs_kt: p("groundspeed"),
                ra_ft: p("alt.radio"),
                nz_g: p("accel.normal"),
                pitch_deg: p("pitch"),
                roll_deg: p("roll"),
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

    /// The aircraft as graded: its type profile, else the class by the
    /// heaviest weight so far.
    fn selection(&self) -> Selection<'_> {
        let weight = finite(self.weight_lb).map(Aircraft::by_weight);
        self.station
            .profiles()
            .select_type(self.icao.clone(), || weight)
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
}
