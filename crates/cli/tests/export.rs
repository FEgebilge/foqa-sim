//! `fdm export` and `fdm logbook` on the golden challenge flight, which
//! carries what a real flight does: the flight file's path, the aircraft's
//! title and tail number, a pilot report.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const ID: &str = "flight-1793027213";
/// The tail number, the sim title and the flight file's folders.
const IDENTITY: [&str; 2] = ["N123FQ", "C172 test"];
const PATHS: [&str; 2] = ["Users", "Documents"];

/// The golden flight under a recorder's name, in a fresh folder.
fn flight() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/challenge/flight.fqz");
    let path = tmp.path().join(format!("{ID}.fqz"));
    fs::copy(golden, &path).unwrap();
    (tmp, path)
}

fn fdm(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fdm"))
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

fn ok(out: Output) -> String {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn each_format_writes_its_file_named_after_the_flight() {
    let (tmp, path) = flight();
    let flight = path.to_str().unwrap();
    for (format, starts) in [
        ("csv", "t_s,utc,phase,time.zulu,alt.pressure,"),
        ("kml", "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<kml "),
        ("gpx", "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<gpx "),
    ] {
        let printed = ok(fdm(tmp.path(), &["export", flight, "--format", format]));
        let file = tmp.path().join(format!("{ID}.{format}"));
        assert_eq!(printed.trim(), format!("{ID}.{format}"));
        let text = fs::read_to_string(&file).unwrap();
        assert!(text.starts_with(starts), "{format}: {}", &text[..80]);
        for private in IDENTITY.iter().chain(&PATHS) {
            assert!(!text.contains(private), "{format}: {private}");
        }
    }
    let kml = fs::read_to_string(tmp.path().join(format!("{ID}.kml"))).unwrap();
    assert!(kml.contains("<name>Pilot report</name>"));
}

#[test]
fn parquet_stays_the_default_and_a_folder_still_works() {
    let (tmp, path) = flight();
    let flight = path.to_str().unwrap();
    let printed = ok(fdm(tmp.path(), &["export", flight]));
    let lines: Vec<&str> = printed.lines().collect();
    assert_eq!(
        lines,
        [format!("{ID}.parquet"), format!("{ID}.events.parquet")]
    );
    let out = tmp.path().join("out");
    fs::create_dir(&out).unwrap();
    ok(fdm(tmp.path(), &["export", flight, "out"]));
    assert!(out.join(format!("{ID}.parquet")).is_file());
    assert!(out.join(format!("{ID}.events.parquet")).is_file());
}

#[test]
fn identity_only_when_asked_and_output_where_told() {
    let (tmp, path) = flight();
    let flight = path.to_str().unwrap();
    ok(fdm(
        tmp.path(),
        &[
            "export",
            flight,
            "--format",
            "gpx",
            "--identity",
            "-o",
            "named.gpx",
        ],
    ));
    let gpx = fs::read_to_string(tmp.path().join("named.gpx")).unwrap();
    for name in IDENTITY {
        assert!(gpx.contains(name), "{name}");
    }
    for private in PATHS {
        assert!(!gpx.contains(private), "{private}");
    }
}

#[test]
fn logbook_of_a_folder_or_a_flight() {
    let (tmp, path) = flight();
    let dir = tmp.path().to_str().unwrap();
    assert_eq!(ok(fdm(tmp.path(), &["logbook", dir])).trim(), "logbook.csv");
    let csv = fs::read_to_string(tmp.path().join("logbook.csv")).unwrap();
    let lines: Vec<&str> = csv.lines().collect();
    assert!(lines[0].starts_with("Aircraft Name,Aircraft Type,Aircraft Registration,"));
    assert_eq!(lines.len(), 2, "{csv}");
    assert!(lines[1].starts_with(",C172,,"), "{}", lines[1]);
    assert!(lines[1].contains(",2026-10-26T15:06:53Z,"), "{}", lines[1]);
    ok(fdm(
        tmp.path(),
        &[
            "logbook",
            path.to_str().unwrap(),
            "--identity",
            "-o",
            "one.csv",
        ],
    ));
    let one = fs::read_to_string(tmp.path().join("one.csv")).unwrap();
    assert!(one.lines().nth(1).unwrap().starts_with("C172 test,C172,"));
}

#[test]
fn bad_options_print_the_usage() {
    let (tmp, path) = flight();
    let flight = path.to_str().unwrap();
    for args in [
        &["export", flight, "--format", "xlsx"][..],
        &["logbook", flight, "--format", "csv"],
    ] {
        let out = fdm(tmp.path(), args);
        assert!(!out.status.success());
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("usage: fdm"),
            "{args:?}"
        );
    }
}
