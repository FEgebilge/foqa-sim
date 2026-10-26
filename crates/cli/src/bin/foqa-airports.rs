//! foqa-airports: checks the simulator's runway data before landings are
//! matched against it, like proving a navigation database before an FDM
//! ground station relies on it. With MSFS running, prints each runway of the
//! chosen airports with the self-checks: which end the heading belongs to,
//! where the landing thresholds lie.
//!
//! `foqa-airports <lat> <lon> [radius-km]`  airports of the sim's reality
//!   bubble (the area loaded around the aircraft) near a position (15 km)
//! `foqa-airports --icao <ICAO>...`         any airports, wherever the aircraft is

use std::error::Error;
use std::thread;
use std::time::{Duration, Instant};

use analyzer::{magnetic_deg, thresholds};
use fdm_core::Airport;
use fdm_core::geo::distance_m;
use sim_link::facility::{AirportFetch, Fetched};
use sim_link::{Facility, FacilityData, Recv, SimConnect};

const USAGE: &str =
    "usage: foqa-airports <lat> <lon> [radius-km]\n       foqa-airports --icao <ICAO>...";
const DEFINE: u32 = 1;
const DEFAULT_RADIUS_KM: f64 = 15.0;
const POLL: Duration = Duration::from_millis(5);

/// Which airports to print.
enum Query {
    Near { lat: f64, lon: f64, radius_km: f64 },
    Icao(Vec<String>),
}

fn main() -> Result<(), Box<dyn Error>> {
    let query = args().ok_or(USAGE)?;
    let mut sim = SimConnect::open("FOQA-Sim airports").map_err(|e| {
        format!("{e}\nis MSFS 2024 running, and MSFS2024_SDK set in this terminal?")
    })?;
    let mut fetch = AirportFetch::new(&mut sim, DEFINE)?;
    // (lat, lon, radius in km) when searching by position.
    let near = match query {
        Query::Near {
            lat,
            lon,
            radius_km,
        } => {
            fetch.search((lat, lon), radius_km * 1000.0);
            Some((lat, lon, radius_km))
        }
        Query::Icao(icaos) => {
            icaos.iter().for_each(|i| fetch.want(i));
            None
        }
    };
    // The same non-blocking fetch foqa-rec runs, driven until it is done.
    while !fetch.is_idle() {
        match sim.dispatch() {
            Some(Recv::Facility(reply)) => {
                if let Facility::Data(node) = &reply {
                    trace(node);
                }
                fetch.feed(reply);
            }
            Some(Recv::Exception {
                code,
                send_id,
                index,
            }) => match fetch.describe(send_id) {
                Some(what) => eprintln!("SimConnect rejected {what} (exception {code})"),
                None => eprintln!("SimConnect exception {code} (send {send_id}, index {index})"),
            },
            Some(Recv::Open {
                simconnect_version, ..
            }) => eprintln!("connected to MSFS, SimConnect {simconnect_version:?}"),
            Some(Recv::Quit) => return Err("simulator closed".into()),
            Some(_) => {}
            None => thread::sleep(POLL),
        }
        match fetch.poll(&mut sim, Instant::now())? {
            Some(Fetched::Near(icaos)) => {
                if let Some((lat, lon, radius_km)) = near {
                    println!(
                        "{} airports within {radius_km} km of {lat}, {lon}",
                        icaos.len()
                    );
                }
            }
            Some(Fetched::Airport(airport)) => {
                let distance_km = near.map(|(lat, lon, _)| {
                    distance_m((lat, lon), (airport.lat_deg, airport.lon_deg)) / 1000.0
                });
                print_airport(&airport, distance_km);
            }
            Some(Fetched::Failed { icao, reason }) => println!("\n{icao}: {reason}"),
            None => {}
        }
    }
    Ok(())
}

fn args() -> Option<Query> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "--icao") {
        return (args.len() > 1)
            .then(|| Query::Icao(args[1..].iter().map(|i| i.to_uppercase()).collect()));
    }
    let num = |i: usize| args.get(i)?.parse::<f64>().ok();
    let radius_km = match args.len() {
        2 => DEFAULT_RADIUS_KM,
        3 => num(2)?,
        _ => return None,
    };
    Some(Query::Near {
        lat: num(0)?,
        lon: num(1)?,
        radius_km,
    })
}

/// One line per facility node, to check the message-flow hypotheses: node
/// types (airport 0, runway 1, threshold 23), parent links, IsListItem and
/// payload sizes (airport 48, runway 52, threshold 4 bytes).
fn trace(node: &FacilityData) {
    eprintln!(
        "  node req {} type {:>2} id {} parent {} list_item {} index {}/{} bytes {}",
        node.request_id,
        node.data_type,
        node.unique_id,
        node.parent_id,
        node.is_list_item,
        node.item_index,
        node.list_size,
        node.payload.len()
    );
}

fn print_airport(a: &Airport, distance_km: Option<f64>) {
    let distance = distance_km.map_or(String::new(), |d| format!(" {d:.1} km"));
    println!(
        "\n{} ({}){distance}  {:.6}, {:.6}  elev {:.0} m  magvar {:.1}  {} runways",
        a.icao,
        a.region,
        a.lat_deg,
        a.lon_deg,
        a.elevation_m,
        a.magvar_deg,
        a.runways.len()
    );
    for rw in &a.runways {
        let (p, s) = (&rw.primary, &rw.secondary);
        println!(
            "  {}/{}  centre {:.6}, {:.6}  hdg {:.2}  {:.0} x {:.0} m  displaced {} {:.0} m, {} {:.0} m",
            p.name(),
            s.name(),
            rw.lat_deg,
            rw.lon_deg,
            rw.heading_deg,
            rw.length_m,
            rw.width_m,
            p.name(),
            p.displaced_m,
            s.name(),
            s.displaced_m
        );
        // Runway numbers are magnetic; the ends are listed in the order the
        // analyzer will use them (the end the true heading belongs to first).
        let magnetic = magnetic_deg(rw.heading_deg, a.magvar_deg);
        let ends = thresholds(rw, a.magvar_deg);
        let order = if ends[0].runway == p.name() {
            "ok"
        } else {
            "ENDS SWAPPED"
        };
        println!(
            "    check: true hdg {:.2}, magnetic {magnetic:.1}, primary {}: {order}",
            rw.heading_deg,
            p.name()
        );
        for t in ends {
            println!(
                "    threshold {}  {:.6}, {:.6}  LDA {:.0} m",
                t.runway, t.lat_deg, t.lon_deg, t.lda_m
            );
        }
    }
}
