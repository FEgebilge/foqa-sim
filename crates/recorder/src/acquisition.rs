//! Sensor wiring: which SimVar feeds each LFL parameter, in which unit and
//! with which conditioning. The counterpart of label/bus assignment on a real
//! aircraft; the LFL itself stays simulator-agnostic.

use fdm_core::{Lfl, SignalId, Ssm};

/// Standard gravity in ft/s², to express body accelerations in g.
const FT_S2_PER_G: f64 = 32.174;
/// MSFS reports pitch nose-down positive and bank left-wing-down positive;
/// FDR convention is nose-up and right-wing-down positive (verified in M1).
const FLIP: f64 = -1.0;
/// Needle deflection is ±127 (LOC) / ±119 (GS); full scale taken as 2 dots.
/// Unverified: check against an ILS approach.
const LOC_DOTS: f64 = 2.0 / 127.0;
const GS_DOTS: f64 = 2.0 / 119.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Column {
    pub simvar: &'static str,
    pub unit: &'static str,
}

const SIM_TIME: Column = Column {
    simvar: "SIMULATION TIME",
    unit: "seconds",
};
const ENGINE_COUNT: Column = Column {
    simvar: "NUMBER OF ENGINES",
    unit: "number",
};

enum Valid {
    Always,
    /// `NoComputedData` while this boolean SimVar is false.
    Flag(&'static str),
    /// Recorded only if the aircraft has at least this many engines.
    Engine(u8),
}

struct Wire {
    signal: &'static str,
    column: Column,
    scale: f64,
    valid: Valid,
}

const fn w(signal: &'static str, simvar: &'static str, unit: &'static str) -> Wire {
    scaled(signal, simvar, unit, 1.0)
}

const fn scaled(
    signal: &'static str,
    simvar: &'static str,
    unit: &'static str,
    scale: f64,
) -> Wire {
    Wire {
        signal,
        column: Column { simvar, unit },
        scale,
        valid: Valid::Always,
    }
}

const fn gated(signal: &'static str, simvar: &'static str, scale: f64, flag: &'static str) -> Wire {
    Wire {
        signal,
        column: Column {
            simvar,
            unit: "number",
        },
        scale,
        valid: Valid::Flag(flag),
    }
}

const fn engine(n: u8, signal: &'static str, simvar: &'static str, unit: &'static str) -> Wire {
    Wire {
        signal,
        column: Column { simvar, unit },
        scale: 1.0,
        valid: Valid::Engine(n),
    }
}

#[rustfmt::skip]
const WIRES: &[Wire] = &[
    w("time.zulu", "ZULU TIME", "seconds"),
    w("alt.pressure", "PRESSURE ALTITUDE", "feet"),
    w("ias", "AIRSPEED INDICATED", "knots"),
    w("heading.mag", "PLANE HEADING DEGREES MAGNETIC", "degrees"),
    w("groundspeed", "GROUND VELOCITY", "knots"),
    w("pos.lat", "PLANE LATITUDE", "degrees"),
    w("pos.lon", "PLANE LONGITUDE", "degrees"),
    w("accel.normal", "G FORCE", "gforce"),
    scaled("accel.long", "ACCELERATION BODY Z", "feet per second squared", 1.0 / FT_S2_PER_G),
    scaled("accel.lat", "ACCELERATION BODY X", "feet per second squared", 1.0 / FT_S2_PER_G),
    scaled("pitch", "PLANE PITCH DEGREES", "degrees", FLIP),
    scaled("roll", "PLANE BANK DEGREES", "degrees", FLIP),
    w("aoa", "INCIDENCE ALPHA", "degrees"),
    w("sideslip", "INCIDENCE BETA", "degrees"),
    w("alt.radio", "RADIO HEIGHT", "feet"),
    gated("nav.loc_dev", "NAV CDI:1", LOC_DOTS, "NAV HAS LOCALIZER:1"),
    gated("nav.gs_dev", "NAV GSI:1", GS_DOTS, "NAV HAS GLIDE SLOPE:1"),
    w("air_ground", "SIM ON GROUND", "bool"),
    w("flap.handle", "FLAPS HANDLE INDEX", "number"),
    w("flap.te_angle", "TRAILING EDGE FLAPS LEFT ANGLE", "degrees"),
    w("gear.handle", "GEAR HANDLE POSITION", "bool"),
    w("gear.extended", "GEAR TOTAL PCT EXTENDED", "percent"),
    w("spoiler.armed", "SPOILERS ARMED", "bool"),
    w("spoiler.handle", "SPOILERS HANDLE POSITION", "percent"),
    w("trim.pitch", "ELEVATOR TRIM POSITION", "degrees"),
    w("ctl.pitch", "YOKE Y POSITION", "percent"),
    w("ctl.roll", "YOKE X POSITION", "percent"),
    w("ctl.yaw", "RUDDER PEDAL POSITION", "percent"),
    engine(1, "eng1.n1", "TURB ENG N1:1", "percent"),
    engine(2, "eng2.n1", "TURB ENG N1:2", "percent"),
    engine(3, "eng3.n1", "TURB ENG N1:3", "percent"),
    engine(4, "eng4.n1", "TURB ENG N1:4", "percent"),
    engine(1, "eng1.throttle", "GENERAL ENG THROTTLE LEVER POSITION:1", "percent"),
    engine(2, "eng2.throttle", "GENERAL ENG THROTTLE LEVER POSITION:2", "percent"),
    engine(3, "eng3.throttle", "GENERAL ENG THROTTLE LEVER POSITION:3", "percent"),
    engine(4, "eng4.throttle", "GENERAL ENG THROTTLE LEVER POSITION:4", "percent"),
    engine(1, "eng1.reverser", "GENERAL ENG REVERSE THRUST ENGAGED:1", "bool"),
    engine(2, "eng2.reverser", "GENERAL ENG REVERSE THRUST ENGAGED:2", "bool"),
    engine(3, "eng3.reverser", "GENERAL ENG REVERSE THRUST ENGAGED:3", "bool"),
    engine(4, "eng4.reverser", "GENERAL ENG REVERSE THRUST ENGAGED:4", "bool"),
    w("brake.left", "BRAKE LEFT POSITION", "percent"),
    w("brake.right", "BRAKE RIGHT POSITION", "percent"),
    w("ap.engaged", "AUTOPILOT MASTER", "bool"),
    w("at.engaged", "AUTOPILOT MANAGED THROTTLE ACTIVE", "bool"),
    w("sel.speed", "AUTOPILOT AIRSPEED HOLD VAR", "knots"),
    w("sel.altitude", "AUTOPILOT ALTITUDE LOCK VAR", "feet"),
    w("sel.vs", "AUTOPILOT VERTICAL HOLD VAR", "feet per minute"),
    w("sel.heading", "AUTOPILOT HEADING LOCK DIR", "degrees"),
    w("stall.warning", "STALL WARNING", "bool"),
    w("wind.speed", "AMBIENT WIND VELOCITY", "knots"),
    w("wind.dir", "AMBIENT WIND DIRECTION", "degrees"),
    w("oat", "AMBIENT TEMPERATURE", "celsius"),
    w("baro.setting", "KOHLSMAN SETTING MB", "millibars"),
    w("dh", "DECISION HEIGHT", "feet"),
    w("weight.gross", "TOTAL WEIGHT", "pounds"),
    w("fuel.total", "FUEL TOTAL QUANTITY WEIGHT", "pounds"),
    w("sim.vs", "VERTICAL SPEED", "feet per minute"),
    w("sim.td.vs", "PLANE TOUCHDOWN NORMAL VELOCITY", "feet per minute"),
    scaled("sim.td.pitch", "PLANE TOUCHDOWN PITCH DEGREES", "degrees", FLIP),
    scaled("sim.td.bank", "PLANE TOUCHDOWN BANK DEGREES", "degrees", FLIP),
    w("sim.rate", "SIMULATION RATE", "number"),
    w("sim.slew", "IS SLEW ACTIVE", "bool"),
];

enum Gate {
    Always,
    Flag(usize),
    Engine { count: usize, n: u8 },
}

struct Resolved {
    signal: SignalId,
    column: usize,
    scale: f64,
    gate: Gate,
}

/// One frame in engineering units, indexed by `SignalId`.
#[derive(Debug, Clone, PartialEq)]
pub struct Conditioned {
    pub sim_time_s: f64,
    pub values: Vec<Option<(f64, Ssm)>>,
}

/// The wiring resolved against an LFL: the raw columns to capture and how
/// each one becomes a parameter value.
pub struct Acquisition {
    columns: Vec<Column>,
    wires: Vec<Resolved>,
    signal_count: usize,
}

impl Acquisition {
    pub fn new(lfl: &Lfl) -> Self {
        let mut columns = vec![SIM_TIME];
        let mut index = |c: Column| match columns.iter().position(|&x| x == c) {
            Some(i) => i,
            None => {
                columns.push(c);
                columns.len() - 1
            }
        };
        let wires = WIRES
            .iter()
            .map(|wire| Resolved {
                signal: lfl
                    .by_name(wire.signal)
                    .unwrap_or_else(|| panic!("unknown signal {}", wire.signal))
                    .id,
                column: index(wire.column),
                scale: wire.scale,
                gate: match wire.valid {
                    Valid::Always => Gate::Always,
                    Valid::Flag(simvar) => Gate::Flag(index(Column {
                        simvar,
                        unit: "bool",
                    })),
                    Valid::Engine(n) => Gate::Engine {
                        count: index(ENGINE_COUNT),
                        n,
                    },
                },
            })
            .collect();
        Self {
            columns,
            wires,
            signal_count: lfl.signals().len(),
        }
    }

    /// Raw columns a source must deliver, in frame order. Column 0 is sim time.
    pub fn columns(&self) -> &[Column] {
        &self.columns
    }

    /// Converts one raw frame to engineering units. `None` if the frame has
    /// no sim time.
    pub fn condition(&self, raw: &[f64]) -> Option<Conditioned> {
        let get = |i: usize| raw.get(i).copied().filter(|v| v.is_finite());
        let sim_time_s = get(0)?;
        let mut values = vec![None; self.signal_count];
        for wire in &self.wires {
            let Some(v) = get(wire.column) else { continue };
            let ssm = match wire.gate {
                Gate::Always => Ssm::Normal,
                Gate::Flag(i) if get(i).is_some_and(|f| f < 0.5) => Ssm::NoComputedData,
                Gate::Flag(_) => Ssm::Normal,
                Gate::Engine { count, n } if get(count).is_some_and(|c| c < f64::from(n)) => {
                    continue;
                }
                Gate::Engine { .. } => Ssm::Normal,
            };
            values[usize::from(wire.signal.0)] = Some((v * wire.scale, ssm));
        }
        Some(Conditioned { sim_time_s, values })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v1_resolves_every_wire() {
        // `new` panics on a wire naming an unknown signal.
        Acquisition::new(&Lfl::V1);
    }

    fn frame(acq: &Acquisition, set: &[(&str, f64)]) -> Vec<f64> {
        let mut raw = vec![f64::NAN; acq.columns().len()];
        for &(simvar, v) in set {
            raw[acq
                .columns()
                .iter()
                .position(|c| c.simvar == simvar)
                .unwrap()] = v;
        }
        raw
    }

    fn value(c: &Conditioned, name: &str) -> Option<(f64, Ssm)> {
        c.values[usize::from(Lfl::V1.by_name(name).unwrap().id.0)]
    }

    #[test]
    fn wires_every_frame_parameter_except_event_fed_ones() {
        let acq = Acquisition::new(&Lfl::V1);
        let wired: Vec<_> = acq.wires.iter().map(|w| w.signal).collect();
        let unwired: Vec<_> = Lfl::V1
            .signals()
            .iter()
            .filter(|s| !wired.contains(&s.id))
            .map(|s| s.name)
            .collect();
        assert_eq!(unwired, ["sim.paused"]);
    }

    #[test]
    fn flips_attitude_to_fdr_convention() {
        let acq = Acquisition::new(&Lfl::V1);
        let c = acq
            .condition(&frame(
                &acq,
                &[("SIMULATION TIME", 1.0), ("PLANE PITCH DEGREES", -7.5)],
            ))
            .unwrap();
        assert_eq!(value(&c, "pitch"), Some((7.5, Ssm::Normal)));
        assert_eq!(value(&c, "roll"), None);
    }

    #[test]
    fn glideslope_without_signal_is_no_computed_data() {
        let acq = Acquisition::new(&Lfl::V1);
        let raw = frame(
            &acq,
            &[
                ("SIMULATION TIME", 1.0),
                ("NAV GSI:1", 0.0),
                ("NAV HAS GLIDE SLOPE:1", 0.0),
            ],
        );
        assert_eq!(
            value(&acq.condition(&raw).unwrap(), "nav.gs_dev"),
            Some((0.0, Ssm::NoComputedData))
        );
    }

    #[test]
    fn engines_beyond_the_installed_count_are_not_recorded() {
        let acq = Acquisition::new(&Lfl::V1);
        let raw = frame(
            &acq,
            &[
                ("SIMULATION TIME", 1.0),
                ("NUMBER OF ENGINES", 2.0),
                ("TURB ENG N1:2", 60.0),
                ("TURB ENG N1:3", 0.0),
            ],
        );
        let c = acq.condition(&raw).unwrap();
        assert_eq!(value(&c, "eng2.n1"), Some((60.0, Ssm::Normal)));
        assert_eq!(value(&c, "eng3.n1"), None);
    }

    #[test]
    fn frame_without_sim_time_is_rejected() {
        let acq = Acquisition::new(&Lfl::V1);
        assert_eq!(acq.condition(&frame(&acq, &[])), None);
    }
}
