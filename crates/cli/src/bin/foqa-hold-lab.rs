//! foqa-hold-lab: flies a challenge's start hands off on the running sim, by
//! the code an attempt runs (`Service`, `SimRequests`), without the app's
//! windows: optionally reloads a saved flight, places the start with its hold,
//! and logs what a second connection reads every 2 s. To tune and check
//! `capture::hold` per aircraft.
//!
//! foqa-hold-lab <challenge.json> <seconds> [flight to reload, no extension]

use std::error::Error;
use std::path::Path;
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use capture::{ASK_TIMEOUT, HoldUntil, InitPosition, RELOAD_TIMEOUT, Service, SimRequests, Start};
use challenge::{Challenge, Mode};
use debrief::Station;
use fdm_core::geo::distance_m;
use sim_link::{Recv, SimConnect};

const DEFINE: u32 = 1;
const M_PER_NM: f64 = 1852.0;
const VARS: [(&str, &str); 15] = [
    ("PLANE LATITUDE", "degrees"),
    ("PLANE LONGITUDE", "degrees"),
    ("PLANE ALTITUDE", "feet"),
    ("AIRSPEED INDICATED", "knots"),
    ("VERTICAL SPEED", "feet per minute"),
    ("PLANE PITCH DEGREES", "degrees"),
    ("PLANE BANK DEGREES", "degrees"),
    ("GENERAL ENG THROTTLE LEVER POSITION:1", "percent"),
    ("GENERAL ENG MIXTURE LEVER POSITION:1", "percent"),
    ("ELEVATOR TRIM PCT", "percent"),
    ("YOKE Y POSITION", "percent"),
    ("AMBIENT WIND DIRECTION", "degrees"),
    ("AMBIENT WIND VELOCITY", "knots"),
    ("AMBIENT PRECIP STATE", "mask"),
    ("LOCAL TIME", "seconds"),
];

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [definition, seconds, reload @ ..] = &args[..] else {
        return Err("usage: foqa-hold-lab <challenge.json> <seconds> [flight to reload]".into());
    };
    let challenge = Challenge::parse(&std::fs::read_to_string(definition)?)?;
    let seconds: f64 = seconds.parse()?;

    let dir = std::env::temp_dir().join("foqa-hold-lab");
    let service = Service::new(dir, Arc::new(Station::core()?))?;
    let sim = service.sim_requests();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || service.run(tx));
    thread::spawn(move || rx.iter().for_each(drop));

    let started = Instant::now();
    if let Some(flight) = reload.first() {
        let answer = sim.reload(Path::new(flight));
        let settled = answer.recv_timeout(RELOAD_TIMEOUT + ASK_TIMEOUT)??;
        println!(
            "reloaded in {:.1} s: {:?}",
            started.elapsed().as_secs_f64(),
            settled.settled
        );
    }
    let threshold = place(&sim, &challenge)?;
    println!("placed; {seconds} s of hands off");
    watch(threshold, seconds)
}

/// What an attempt's placement sends; the threshold it is flown to.
fn place(sim: &SimRequests, c: &Challenge) -> Result<(f64, f64), Box<dyn Error>> {
    if c.start.mode != Mode::Teleport {
        return Err("this challenge is not placed".into());
    }
    let airport = sim.airport(&c.airport).recv_timeout(ASK_TIMEOUT)??;
    let s = challenge::start_state(c, &airport).ok_or_else(|| c.no_runway(&airport))?;
    let (lat, lon, gate_m) = c.gate_fix(&airport).ok_or("no gate")?;
    let start = Start {
        pos: InitPosition {
            latitude_deg: s.lat_deg,
            longitude_deg: s.lon_deg,
            altitude_ft: s.alt_ft,
            pitch_deg: 0.0,
            bank_deg: 0.0,
            heading_deg: s.heading_deg,
            on_ground: 0,
            airspeed_kt: s.ias_kt.round() as u32,
        },
        gear_down: c.start.gear_down,
        flaps: Some(c.start.flaps),
        nav1_hz: None,
        hold: Some(HoldUntil {
            ias_kt: s.ias_kt,
            threshold_lat_deg: lat,
            threshold_lon_deg: lon,
            gate_m,
        }),
    };
    sim.place(start).recv_timeout(ASK_TIMEOUT)??;
    Ok((lat, lon))
}

/// Logs the aircraft every 2 s over a connection of its own.
fn watch(threshold: (f64, f64), seconds: f64) -> Result<(), Box<dyn Error>> {
    let mut sim = SimConnect::open("FOQA-Sim hold lab")?;
    for (simvar, unit) in VARS {
        sim.add_f64(DEFINE, simvar, unit)?;
    }
    sim.request_sim_frames(DEFINE, DEFINE, 0)?;
    let start = Instant::now();
    let mut next_log = 0.0;
    while start.elapsed().as_secs_f64() < seconds {
        while let Some(recv) = sim.dispatch() {
            let v = match recv {
                Recv::Quit => return Ok(()),
                Recv::Data { values, .. } => values,
                _ => continue,
            };
            let t = start.elapsed().as_secs_f64();
            if t < next_log {
                continue;
            }
            next_log = t + 2.0;
            println!(
                "{t:5.0} {:4.1}nm alt {:5.0} ias {:5.1} vs {:6.0} pitch {:5.1} bank {:5.1} lever {:5.1} mix {:4.0} trim {:6.1} yoke {:5.1} wind {:3.0}/{:4.1} precip {:.0} local {:02.0}:{:02.0}",
                distance_m((v[0], v[1]), threshold) / M_PER_NM,
                v[2],
                v[3],
                v[4],
                -v[5],
                v[6],
                v[7],
                v[8],
                v[9],
                v[10],
                v[11],
                v[12],
                v[13],
                (v[14] / 3600.0).floor(),
                (v[14] / 60.0 % 60.0).floor(),
            );
        }
        sim.wait(Duration::from_millis(50));
    }
    Ok(())
}
