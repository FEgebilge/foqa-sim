//! Synthetic golden flights for tests: scripted profiles pushed through the
//! whole on-board chain (wiring, FDAU, journal) into a `Flight`, so the
//! ground segment can be checked against flights whose answer is known by
//! construction. Every flight lands on a 600 ft aerodrome, 0 ft AGL being
//! the wheels on the runway.

use std::collections::HashMap;

use analyzer::{Flight, Track, touchdowns};
use fdm_core::geo::LocalFrame;
use fdm_core::{AircraftId, Airport, Designator, Lfl, Runway, RunwayEnd};
use recorder::{Recorder, SimEvent, Telemetry};

pub const FRAME_HZ: f64 = 60.0;
/// Sim time of the first frame: the recording is relative to it.
pub const SIM_START_S: f64 = 1000.0;
pub const FIELD_FT: f64 = 600.0;
pub const AIRLINER_LB: f64 = 140_000.0;
pub const LIGHT_LB: f64 = 2_300.0;
/// Time from 1500 ft on the approach to touchdown.
pub const APPROACH_S: f64 = 124.0;
/// Touchdown time of the single-approach flights (the flight starts on the approach).
pub const TD_S: f64 = APPROACH_S;
/// Time from touchdown to the end of the recording, after the rollout.
pub const ROLLOUT_S: f64 = 45.0;

/// Heading of every scripted flight over the ground (runway 20 on a 3000 m
/// runway), and where the approach starts.
pub const TRACK_DEG: f64 = 200.0;
pub const START: (f64, f64) = (38.5, 27.0);
pub const KT_MS: f64 = 1852.0 / 3600.0;

/// The aircraft state at one instant, in aviation units.
pub struct Pt {
    /// Height above the runway (also the radio height), ft.
    pub alt: f64,
    /// Indicated and ground speed, kt.
    pub speed: f64,
    /// Normal load, g.
    pub g: f64,
    /// Pitch attitude, nose-up positive, deg.
    pub pitch: f64,
    /// Trailing-edge flap angle, deg, 0 = retracted.
    pub flaps: f64,
    /// Landing gear extension, %.
    pub gear: f64,
    /// Wind direction (from, true) and speed, (deg, kt).
    pub wind: (f64, f64),
    /// Magnetic heading (the airports have no variation), deg.
    pub heading: f64,
    /// Direction flown over the ground, true, deg.
    pub track: f64,
    /// Simulation rate, 1 = real time.
    pub sim_rate: f64,
    /// Autopilot engaged.
    pub ap: bool,
    pub ground: bool,
    /// SimVars missing from this frame (a dropout), recorded as `NaN`.
    pub lost: &'static [&'static str],
    /// Other SimVars and their raw values in this frame (reversers,
    /// brakes, spoilers, ILS needles).
    pub set: Vec<(&'static str, f64)>,
}

/// Piecewise-linear interpolation through `(t, value)` keys, held at both ends.
pub fn lerp(keys: &[(f64, f64)], t: f64) -> f64 {
    let next = keys.partition_point(|k| k.0 <= t);
    match (next.checked_sub(1).map(|i| keys[i]), keys.get(next)) {
        (Some((t0, v0)), Some(&(t1, v1))) => v0 + (t - t0) / (t1 - t0) * (v1 - v0),
        (Some((_, v)), None) | (None, Some(&(_, v))) => v,
        (None, None) => unreachable!("no keys"),
    }
}

/// A 1500 ft approach at 140 kt, about 750 fpm down, flared to a 300 fpm
/// touchdown at `td`, then a rollout slowing to 20 kt. `peak_g` is held for
/// 0.2 s from contact, the gear compressing.
pub fn landing(t: f64, td: f64, peak_g: f64) -> Pt {
    let g = if (td..td + 0.2).contains(&t) {
        peak_g
    } else {
        1.0
    };
    let descent = [
        (td - APPROACH_S, 1500.0),
        (td - 12.0, 100.0),
        (td - 2.0, 10.0),
        (td, 0.0),
    ];
    Pt {
        alt: lerp(&descent, t),
        speed: lerp(&[(td, 140.0), (td + 40.0, 20.0)], t),
        g,
        pitch: 3.0,
        flaps: 0.0,
        gear: 100.0,
        wind: (0.0, 0.0),
        heading: TRACK_DEG,
        track: TRACK_DEG,
        sim_rate: 1.0,
        ap: false,
        ground: t >= td,
        lost: &[],
        set: Vec::new(),
    }
}

/// Records the script at 60 Hz as raw MSFS frames and analyses it. Bank is
/// fixed at zero; attitude is written the way MSFS reports it: -3.0 for
/// 3 deg nose-up (nose-up is negative), left-wing-down positive.
pub fn fly(weight_lb: f64, duration_s: f64, script: impl Fn(f64) -> Pt) -> (Flight, Track) {
    fly_to(&[], weight_lb, duration_s, script)
}

/// `fly`, with `events` (airport snapshots, aircraft identity) recorded at
/// the start. The aircraft flies from `START` along its `track` at its
/// ground speed.
pub fn fly_to(
    events: &[SimEvent],
    weight_lb: f64,
    duration_s: f64,
    script: impl Fn(f64) -> Pt,
) -> (Flight, Track) {
    let at_start: Vec<_> = events.iter().map(|e| (0.0, e.clone())).collect();
    let journal = journal(&at_start, weight_lb, duration_s, script);
    let flight = Flight::read(&journal[..]).unwrap();
    let track = Track::new(&flight).unwrap();
    (flight, track)
}

/// The journal `fly_to` analyses, with each event recorded before the
/// first frame at or after its time (s from the start).
pub fn journal(
    events: &[(f64, SimEvent)],
    weight_lb: f64,
    duration_s: f64,
    script: impl Fn(f64) -> Pt,
) -> Vec<u8> {
    let mut recorder = Recorder::new(&Lfl::V1, Vec::new()).unwrap();
    let columns: HashMap<_, _> = recorder
        .acquisition()
        .columns()
        .iter()
        .enumerate()
        .map(|(i, c)| (c.simvar, i))
        .collect();
    let mut events = events.iter().peekable();
    let frame = LocalFrame::new(START.0, START.1);
    let (mut east, mut north) = (0.0, 0.0);
    for k in 0..=(duration_s * FRAME_HZ) as usize {
        let t = k as f64 / FRAME_HZ;
        while let Some((_, e)) = events.next_if(|(at, _)| *at <= t) {
            recorder.record(&Telemetry::Event(e.clone())).unwrap();
        }
        let p = script(t);
        let (sin, cos) = p.track.to_radians().sin_cos();
        let step_m = p.speed * KT_MS / FRAME_HZ;
        (east, north) = (east + step_m * sin, north + step_m * cos);
        let (lat, lon) = frame.point(east, north);
        let mut raw = vec![f64::NAN; columns.len()];
        for (simvar, value) in [
            ("SIMULATION TIME", SIM_START_S + t),
            ("PLANE LATITUDE", lat),
            ("PLANE LONGITUDE", lon),
            ("PRESSURE ALTITUDE", FIELD_FT + p.alt),
            ("RADIO HEIGHT", p.alt),
            ("AIRSPEED INDICATED", p.speed),
            ("GROUND VELOCITY", p.speed),
            ("G FORCE", p.g),
            ("PLANE PITCH DEGREES", -p.pitch),
            ("PLANE BANK DEGREES", 0.0),
            ("PLANE HEADING DEGREES MAGNETIC", p.heading),
            ("SIM ON GROUND", f64::from(u8::from(p.ground))),
            // MSFS sends a 0-1 fraction whatever the unit (docs/format.md).
            ("GEAR TOTAL PCT EXTENDED", p.gear / 100.0),
            ("AMBIENT WIND DIRECTION", p.wind.0),
            ("AMBIENT WIND VELOCITY", p.wind.1),
            ("SIMULATION RATE", p.sim_rate),
            ("IS SLEW ACTIVE", 0.0),
            ("AUTOPILOT MASTER", f64::from(u8::from(p.ap))),
            ("TRAILING EDGE FLAPS LEFT ANGLE", p.flaps),
            ("AUTOPILOT AIRSPEED HOLD VAR", 140.0),
            ("STALL WARNING", 0.0),
            ("TOTAL WEIGHT", weight_lb),
        ] {
            raw[columns[simvar]] = value;
        }
        for &(simvar, value) in &p.set {
            raw[columns[simvar]] = value;
        }
        for simvar in p.lost {
            raw[columns[simvar]] = f64::NAN;
        }
        recorder.record(&Telemetry::Frame(raw)).unwrap();
    }
    recorder.into_inner()
}

/// Runway 20 whose threshold lies `along_m` behind where the stable
/// airliner landing (`landing`) touches down. The flight is flown once to
/// find that point.
pub fn runway_behind_touchdown(along_m: f64) -> Airport {
    let (flight, _) = fly(AIRLINER_LB, TD_S + 1.0, |t| landing(t, TD_S, 1.2));
    let t = touchdowns(&flight, &Track::new(&flight).unwrap())[0].t;
    let (lat, lon) = (
        flight.at("pos.lat", t).unwrap(),
        flight.at("pos.lon", t).unwrap(),
    );
    // The centre is half the runway past the threshold.
    let (sin, cos) = TRACK_DEG.to_radians().sin_cos();
    let past = 1500.0 - along_m;
    let (lat_deg, lon_deg) = LocalFrame::new(lat, lon).point(past * sin, past * cos);
    let end = |number| RunwayEnd {
        number,
        designator: Designator::None,
        displaced_m: 0.0,
    };
    Airport {
        icao: "TEST".into(),
        region: String::new(),
        lat_deg,
        lon_deg,
        elevation_m: 0.0,
        magvar_deg: 0.0,
        runways: vec![Runway {
            lat_deg,
            lon_deg,
            elevation_m: 0.0,
            heading_deg: TRACK_DEG,
            length_m: 3000.0,
            width_m: 45.0,
            primary: end(20),
            secondary: end(2),
        }],
    }
}

/// The identity MSFS 2024 reports for ICAO type `icao`.
pub fn identity(icao: &str) -> SimEvent {
    SimEvent::Aircraft(AircraftId {
        title: format!("{icao} test"),
        atc_model: format!("ATCCOM.AC_MODEL {icao}.0.text"),
        atc_type: String::new(),
        atc_id: "N123FQ".into(),
    })
}
