//! foqa-reset-lab: finds out, on the running sim, which call puts a damaged
//! or crashed aircraft back in its pristine state for a challenge's "Fly
//! again": a saved flight reloaded, a repair key event, the sim's own
//! situation reset. A bench test before the recorder relies on one.
//!
//! Type a letter and Enter: `s` save the flight (and the position), `l`
//! load it, `p` place the aircraft at the saved position, `r`
//! REPAIR_AND_REFUEL, `x` SITUATION_RESET, `a` RELOAD_USER_AIRCRAFT, `q`
//! quit. Damage SimVars and sim events are printed as they change, with the
//! time since start, and copied to `foqa-reset-lab.log`.

use std::error::Error;
use std::fs::File;
use std::io::{BufRead, Write};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use sim_link::{InitPosition, Recv, SimConnect};

const STATE: u32 = 1;
const POSITION: u32 = 2;
const PLACE: u32 = 3;
const SYSTEM: [&str; 8] = [
    "SimStart",
    "SimStop",
    "Crashed",
    "CrashReset",
    "FlightLoaded",
    "FlightSaved",
    "Sim",
    "Pause_EX1",
];
/// Ids of the key events, apart from the system events (one id space).
const KEYS: [(char, u32, &str); 3] = [
    ('r', 20, "REPAIR_AND_REFUEL"),
    ('x', 21, "SITUATION_RESET"),
    ('a', 22, "RELOAD_USER_AIRCRAFT"),
];
/// What tells damage, a crash and a reset apart.
const WATCH: [(&str, &str); 9] = [
    ("CRASH FLAG", "number"),
    ("CRASH SEQUENCE", "number"),
    ("FLAP DAMAGE BY SPEED", "bool"),
    ("GEAR DAMAGE BY SPEED", "bool"),
    ("ENG FAILED:1", "bool"),
    ("FLAPS HANDLE INDEX", "number"),
    ("FUEL TOTAL QUANTITY", "gallons"),
    ("SIM ON GROUND", "bool"),
    ("PLANE ALT ABOVE GROUND", "feet"),
];
const WHERE: [(&str, &str); 5] = [
    ("PLANE LATITUDE", "degrees"),
    ("PLANE LONGITUDE", "degrees"),
    ("PLANE ALTITUDE", "feet"),
    ("PLANE HEADING DEGREES TRUE", "degrees"),
    ("AIRSPEED INDICATED", "knots"),
];

fn main() -> Result<(), Box<dyn Error>> {
    let mut sim = SimConnect::open("FOQA-Sim reset lab")
        .map_err(|e| format!("{e}\nis MSFS 2024 running?"))?;
    let mut log = Log::new(File::create("foqa-reset-lab.log")?);
    // Send id → what was asked, to name the call an exception refers to.
    let mut sent: Vec<(u32, String)> = Vec::new();
    for (i, name) in SYSTEM.iter().enumerate() {
        sim.subscribe_system_event(i as u32 + 1, name)?;
    }
    sim.subscribe_flow_events()?;
    for (_, id, name) in KEYS {
        sent.push((sim.map_event(id, name)?, format!("map {name}")));
    }
    for (define, vars) in [(STATE, &WATCH[..]), (POSITION, &WHERE[..])] {
        for (simvar, unit) in vars {
            sent.push((
                sim.add_f64(define, simvar, unit)?,
                format!("simvar {simvar}"),
            ));
        }
        sim.request_on_change(define, define)?;
    }
    let flt = std::env::temp_dir().join("foqa-reset-lab");
    let flt = flt.to_str().ok_or("temp path is not UTF-8")?.to_owned();
    log.line(&format!("connected; flight file {flt}.FLT"));
    log.line(
        "commands: s save, l load, p place, r repair, x situation reset, a reload aircraft, q quit",
    );

    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        for line in std::io::stdin().lock().lines().map_while(Result::ok) {
            if tx.send(line.trim().chars().next()).is_err() {
                break;
            }
        }
    });
    let mut state: Option<Vec<f64>> = None;
    let mut here: Option<Vec<f64>> = None;
    let mut saved: Option<Vec<f64>> = None;
    loop {
        while let Some(recv) = sim.dispatch() {
            match recv {
                Recv::Quit => return Ok(()),
                Recv::Exception { code, send_id, .. } => {
                    let what = sent.iter().find(|s| s.0 == send_id).map_or("?", |s| &s.1);
                    log.line(&format!("EXCEPTION {code} on {what}"));
                }
                Recv::Event { id, data } => {
                    let name = SYSTEM.get(id as usize - 1).unwrap_or(&"?");
                    log.line(&format!("event {name} data={data}"));
                }
                Recv::EventFilename { id, file } => {
                    let name = SYSTEM.get(id as usize - 1).unwrap_or(&"?");
                    log.line(&format!("event {name} {file}"));
                }
                Recv::Flow { event, flt_path } => log.line(&format!("flow {event} {flt_path}")),
                Recv::Data {
                    request_id: STATE,
                    values,
                } => {
                    // Height changes every second; print when anything else does.
                    let changed = state.as_ref().is_none_or(|s| s[..8] != values[..8]);
                    if changed {
                        let shown: Vec<String> = WATCH
                            .iter()
                            .zip(&values)
                            .map(|((n, _), v)| format!("{n}={v:.1}"))
                            .collect();
                        log.line(&shown.join("  "));
                    }
                    state = Some(values);
                }
                Recv::Data {
                    request_id: POSITION,
                    values,
                } => here = Some(values),
                _ => {}
            }
        }
        match rx.try_recv() {
            Ok(Some('q')) | Err(mpsc::TryRecvError::Disconnected) => return Ok(()),
            Ok(Some('s')) => {
                sent.push((
                    sim.flight_save(&flt, "FOQA-Sim reset lab")?,
                    "FlightSave".into(),
                ));
                saved.clone_from(&here);
                log.line(&format!("FlightSave sent; position {:?}", saved));
            }
            Ok(Some('l')) => {
                sent.push((sim.flight_load(&flt)?, "FlightLoad".into()));
                log.line("FlightLoad sent");
            }
            Ok(Some('p')) => match &saved {
                Some(p) => {
                    let pos = InitPosition {
                        latitude_deg: p[0],
                        longitude_deg: p[1],
                        altitude_ft: p[2],
                        pitch_deg: 0.0,
                        bank_deg: 0.0,
                        heading_deg: p[3],
                        on_ground: 0,
                        airspeed_kt: p[4].round() as u32,
                    };
                    sent.push((sim.set_init_position(PLACE, &pos)?, "InitPosition".into()));
                    log.line("placed at the saved position");
                }
                None => log.line("save first (s): no position yet"),
            },
            Ok(Some(c)) => match KEYS.iter().find(|k| k.0 == c) {
                Some(&(_, id, name)) => {
                    sent.push((sim.transmit_event(id, 0)?, name.into()));
                    log.line(&format!("{name} sent"));
                }
                None => log.line(&format!("unknown command {c}")),
            },
            Ok(None) | Err(mpsc::TryRecvError::Empty) => {}
        }
        sim.wait(Duration::from_millis(50));
    }
}

/// Lines with the seconds since start, on screen and in the log file.
struct Log {
    file: File,
    start: Instant,
}

impl Log {
    fn new(file: File) -> Self {
        Self {
            file,
            start: Instant::now(),
        }
    }

    fn line(&mut self, text: &str) {
        let line = format!("{:8.1}  {text}", self.start.elapsed().as_secs_f64());
        println!("{line}");
        let _ = writeln!(self.file, "{line}");
    }
}
