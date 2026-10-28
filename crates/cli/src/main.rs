//! fdm: ground-segment command line.
//!
//! `fdm analyze <flight> [--rules file.yaml]`   phases, touchdowns, events
//! `fdm debrief <flight> [--json]`              the app's debrief, as JSON
//! `fdm export <flight> [--format parquet|csv|kml|gpx] [--identity] [-o file]`
//!                                              the flight for other tools (docs/exports.md)
//! `fdm export <flight> <out-dir>`               long-format Parquet (+ events) into a folder
//! `fdm logbook <folder|flight> [--identity] [-o file]` a Little Navmap logbook CSV
//! `fdm compact <journal.fqj|folder>`           journals to compact flights
//! `fdm list <folder>`                          the app's flight list, as JSON
//! `fdm challenge <challenge.json> <flight.fqz>` a landing-challenge attempt, as JSON
//! `fdm challenge --check <challenge.json>`      validates a definition (exit 1 if invalid)
//! `fdm slice <flight> --ref <ref> [-o out.fqz]` an attempt's recording slice, what a site receives
//! `fdm slice <flight> --from <s> --to <s> [-o out.fqz]` the same, by recording time
//!
//! `<flight>` is a journal (`.fqj`), a compact flight (`.fqz`) or a raw
//! capture (`.raw.jsonl.gz`), which is first recorded through the on-board
//! chain in memory. `compact` replaces closed journals, so not while
//! recording. Without
//! `--rules`, the built-in core rule set is used. A compact flight given to
//! `challenge` or `slice` may decompress to 32 MiB at most (a site runs them
//! on uploads), others to 256 MiB.

use std::error::Error;
use std::fs;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use analyzer::RunwayFix;
use debrief::export::{self, Format, Options};
use debrief::{Analysis, Index, Station, flight_id};
use fdm_core::SimTime;
use rules::Event;

const USAGE: &str =
    "usage: fdm analyze <flight.fqj|flight.fqz|capture.raw.jsonl.gz> [--rules file.yaml]
       fdm debrief <flight> [--json]
       fdm export <flight> [--format parquet|csv|kml|gpx] [--identity] [-o file]
       fdm export <flight> <out-dir>
       fdm logbook <folder|flight> [--identity] [-o file]
       fdm compact <journal.fqj|folder>
       fdm list <folder>
       fdm challenge <challenge.json> <flight.fqz>
       fdm challenge --check <challenge.json>
       fdm slice <flight.fqj|flight.fqz> (--ref <ref> | --from <s> --to <s>) [-o out.fqz]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["analyze", path] => analyze(Path::new(path), None),
        ["analyze", path, "--rules", rules] => analyze(Path::new(path), Some(Path::new(rules))),
        ["debrief", path] | ["debrief", path, "--json"] => debrief(Path::new(path)),
        ["export", path, ref rest @ ..] => match export_args(rest) {
            Some(a) => export(Path::new(path), a),
            None => Err(USAGE.into()),
        },
        ["logbook", path, ref rest @ ..] => match export_args(rest) {
            Some(a @ Export { format: None, .. }) => logbook(Path::new(path), a),
            _ => Err(USAGE.into()),
        },
        ["compact", path] => compact(Path::new(path)),
        ["list", dir] => list(Path::new(dir)),
        ["challenge", "--check", def] => check(Path::new(def)),
        ["challenge", def, path] => challenge(Path::new(def), Path::new(path)),
        ["slice", path, ref rest @ ..] => match slice_args(rest) {
            Some((cut, out)) => slice(Path::new(path), cut, out),
            None => Err(USAGE.into()),
        },
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("fdm: {e}");
            ExitCode::FAILURE
        }
    }
}

/// The flight at `path` graded by `station`, noting a cut-off journal.
fn open<'s>(station: &'s Station, path: &Path) -> Result<Analysis<'s>, Box<dyn Error>> {
    let analysis = station.open(path)?;
    if let Some(offset) = analysis.flight.damaged_at {
        eprintln!(
            "note: journal cut off at byte {offset} (recorder was stopped); read up to there"
        );
    }
    Ok(analysis)
}

fn debrief(path: &Path) -> Result<(), Box<dyn Error>> {
    let station = Station::core()?;
    let debrief = open(&station, path)?.debrief();
    println!("{}", serde_json::to_string_pretty(&debrief)?);
    Ok(())
}

fn analyze(path: &Path, rules_path: Option<&Path>) -> Result<(), Box<dyn Error>> {
    // Rules and profiles first: a broken file fails before any flight work.
    let rules = match rules_path {
        Some(path) => rules::load(&fs::read_to_string(path)?)?,
        None => debrief::core_rules()?,
    };
    let station = Station::new(rules)?;
    let a = open(&station, path)?;

    let duration = a.track.t.last().map_or(0.0, |t| t.as_secs_f64());
    println!(
        "{}  ({duration:.0} s, LFL v{}, {})",
        a.id,
        a.flight.lfl_version(),
        a.selection
    );
    println!("\nPhases (CICTT)");
    for s in &a.phases {
        println!(
            "  {:>7.1} – {:>7.1} s  {:<12}  {}",
            s.start.as_secs_f64(),
            s.end.as_secs_f64(),
            s.phase.name(),
            debrief::phase_trigger(&s.cause)
        );
    }
    println!("\nTouchdowns");
    for td in &a.touchdowns {
        let opt = |v: Option<f64>| v.map_or("—".to_owned(), |v| format!("{v:.1}"));
        println!(
            "  {:>7.1} s  sink {:.0} fpm (sim {})  {:.2} g  pitch {}°  roll {}°  IAS {} kt  {}{}",
            td.t.as_secs_f64(),
            td.sink_fpm,
            td.sim_sink_fpm
                .map_or("—".to_owned(), |v| format!("{v:.0}")),
            td.peak_g,
            opt(td.pitch_deg),
            opt(td.roll_deg),
            opt(td.ias_kt),
            td.runway
                .as_ref()
                .map_or("no runway data".to_owned(), runway_text),
            if td.bounce { "  BOUNCE" } else { "" },
        );
    }
    println!("\nEvents ({} rules)", station.rules().len());
    if a.events.is_empty() {
        println!("  none");
    }
    for e in &a.events {
        println!("  {}", describe(e));
    }
    Ok(())
}

/// `LTBJ 16L  +412 m from threshold, 3 m R, In TDZ`
fn runway_text(fix: &RunwayFix) -> String {
    format!(
        "{} {}  {:+.0} m from threshold, {:.0} m {}, {}",
        fix.icao,
        fix.runway,
        fix.along_m,
        fix.cross_m.abs(),
        if fix.cross_m < 0.0 { "L" } else { "R" },
        fix.zone(),
    )
}

/// `MEDIUM  229.5–233.0 s  high-sink-below-1000@1  High descent rate…: -vs = 1180 fpm > 1000 fpm`
fn describe(e: &Event) -> String {
    let time = if e.start == e.end {
        secs(e.start)
    } else {
        format!("{}–{}", secs(e.start), secs(e.end))
    };
    let with_unit = |x: f64| match e.unit {
        Some(unit) if !unit.symbol().is_empty() => format!("{x:.2} {}", unit.symbol()),
        _ => format!("{x:.2}"),
    };
    let detail = match (e.value, e.threshold) {
        (Some(v), Some(thr)) => {
            let cmp = if v > thr { ">" } else { "<" };
            format!(
                ": {} = {} {cmp} {}",
                e.measure,
                with_unit(v),
                with_unit(thr)
            )
        }
        _ => String::new(),
    };
    let level = format!("{:?}", e.level).to_uppercase();
    format!(
        "{level:<8}  {time:>15} s  {}@{}  {}{detail}",
        e.rule, e.version, e.title
    )
}

fn secs(t: SimTime) -> String {
    format!("{:.1}", t.as_secs_f64())
}

/// `fdm export` / `fdm logbook` options after the flight.
#[derive(Debug, Default, PartialEq)]
struct Export<'a> {
    format: Option<Format>,
    options: Options,
    out: Option<&'a str>,
    /// The older `fdm export <flight> <out-dir>`: Parquet into that folder.
    dir: Option<&'a str>,
}

/// `[--format <f>] [--identity] [-o <file>]` in any order, or one folder.
fn export_args<'a>(args: &[&'a str]) -> Option<Export<'a>> {
    if let [dir] = args
        && !dir.starts_with('-')
    {
        return Some(Export {
            dir: Some(dir),
            ..Export::default()
        });
    }
    let mut a = Export::default();
    let mut rest = args;
    while let [flag, tail @ ..] = rest {
        rest = match (*flag, tail) {
            ("--format", [f, tail @ ..]) if a.format.is_none() => {
                a.format = Some(Format::parse(f)?);
                tail
            }
            ("-o", [out, tail @ ..]) if a.out.is_none() => {
                a.out = Some(out);
                tail
            }
            ("--identity", tail) if !a.options.identity => {
                a.options.identity = true;
                tail
            }
            _ => return None,
        };
    }
    Some(a)
}

/// Writes the export and prints the files written. Parquet needs no
/// analysis, so it also takes a flight without air/ground data.
fn export(path: &Path, a: Export) -> Result<(), Box<dyn Error>> {
    let format = a.format.unwrap_or(Format::Parquet);
    let out = match (a.out, a.dir) {
        (Some(out), _) => PathBuf::from(out),
        (None, dir) => {
            let file = PathBuf::from(flight_id(path)).with_extension(format.ext());
            dir.map_or(file.clone(), |d| Path::new(d).join(file))
        }
    };
    let written = match format {
        Format::Parquet => export::parquet(&debrief::load(path)?, a.options, &out)?,
        _ => open(&Station::core()?, path)?.export(format, a.options, &out)?,
    };
    for file in written {
        println!("{}", file.display());
    }
    Ok(())
}

/// The flights of a folder (its index), or one flight, as a Little Navmap
/// logbook, `logbook.csv` by default.
fn logbook(path: &Path, a: Export) -> Result<(), Box<dyn Error>> {
    let station = Station::core()?;
    let summaries = match path.is_dir() {
        true => Index::open(path).summaries(&station),
        false => vec![open(&station, path)?.summary()],
    };
    let out = Path::new(a.out.unwrap_or("logbook.csv"));
    let n = export::logbook_file(&summaries, a.options, out)?;
    let skipped = summaries.len() - n;
    if skipped > 0 {
        eprintln!("note: {skipped} flight(s) without a departure or arrival airport left out");
    }
    println!("{}", out.display());
    Ok(())
}

fn compact(path: &Path) -> Result<(), Box<dyn Error>> {
    let journals = if path.is_dir() {
        debrief::journals(path)
    } else {
        vec![path.to_owned()]
    };
    match debrief::compact_all(journals) {
        0 => Ok(()),
        n => Err(format!("{n} journal(s) not compacted").into()),
    }
}

/// The flight graded as an attempt at the challenge: what the site's server
/// runs on an uploaded recording (docs/challenges.md).
fn challenge(def: &Path, path: &Path) -> Result<(), Box<dyn Error>> {
    let challenge = challenge::Challenge::parse(&fs::read_to_string(def)?)?;
    uploadable(path)?;
    let station = Station::core()?;
    let attempt = challenge::evaluate(&challenge, &challenge_input(&station, path)?);
    println!("{}", serde_json::to_string_pretty(&attempt)?);
    Ok(())
}

/// The records of a challenge input (`challenge`, `slice`): a compact
/// flight's body held to `recorder::CHALLENGE_MAX_BODY`, not the general cap.
fn challenge_records(path: &Path) -> std::io::Result<recorder::FlightRecords<impl std::io::Read>> {
    let file = BufReader::new(fs::File::open(path)?);
    recorder::read_records_within(file, recorder::CHALLENGE_MAX_BODY)
}

/// The flight graded as a challenge input.
fn challenge_input<'s>(station: &'s Station, path: &Path) -> Result<Analysis<'s>, Box<dyn Error>> {
    let flight = analyzer::Flight::from_records(challenge_records(path)?)?;
    let analysis = station.analyse(flight_id(path), flight);
    Ok(analysis.ok_or("no air/ground data in this flight")?)
}

/// Validates a challenge definition the way an attempt is graded against
/// it: what a site runs on its catalogue, so both agree (docs/challenges.md).
fn check(def: &Path) -> Result<(), Box<dyn Error>> {
    let c = challenge::Challenge::parse(&fs::read_to_string(def)?)?;
    let ok = serde_json::json!({ "ok": true, "id": c.id, "version": c.version });
    println!("{ok}");
    Ok(())
}

/// An upload is a compact flight (`.fqz`) only: its decompressed body is
/// capped, while a raw capture's gzip would let a small upload expand.
fn uploadable(path: &Path) -> Result<(), String> {
    match path.extension().is_some_and(|e| e == "fqz") {
        true => Ok(()),
        false => Err(format!(
            "{}: a challenge attempt is a .fqz compact flight",
            path.display()
        )),
    }
}

/// `--ref <ref>` or `--from <s> --to <s>`, then an optional `-o <out>`.
fn slice_args<'a>(args: &[&'a str]) -> Option<(recorder::Cut<'a>, Option<&'a str>)> {
    let secs = |s: &str| {
        let s: f64 = s.parse().ok().filter(|s: &f64| s.is_finite())?;
        Some(SimTime::from_secs_f64(s))
    };
    let (cut, rest) = match args {
        ["--ref", r, rest @ ..] if recorder::is_reference(r) => (recorder::Cut::Attempt(r), rest),
        ["--from", a, "--to", b, rest @ ..] => (recorder::Cut::Range(secs(a)?, secs(b)?), rest),
        _ => return None,
    };
    match rest {
        [] => Some((cut, None)),
        ["-o", out] => Some((cut, Some(out))),
        _ => None,
    }
}

/// Cuts an attempt's slice (docs/challenge-protocol.md A5) into `out`,
/// `<flight>.slice.fqz` by default, and reports it as JSON.
fn slice(path: &Path, cut: recorder::Cut, out: Option<&str>) -> Result<(), Box<dyn Error>> {
    let records = challenge_records(path)?;
    let recording = recorder::Recording::from_records(records.lfl_version(), records)?;
    let sliced = recording.slice(cut).ok_or_else(|| match cut {
        recorder::Cut::Attempt(r) => format!("{}: no attempt marker with ref {r}", path.display()),
        recorder::Cut::Range(..) => format!("{}: no data", path.display()),
    })?;
    let out = out.map_or_else(|| path.with_extension("slice.fqz"), PathBuf::from);
    let bytes = sliced.to_bytes()?;
    fs::write(&out, &bytes)?;
    let report = serde_json::json!({
        "out": out.display().to_string(),
        "bytes": bytes.len(),
        "from_s": sliced.from.as_secs_f64(),
        "to_s": sliced.to.as_secs_f64(),
        "complete": sliced.complete,
    });
    println!("{report}");
    Ok(())
}

/// The summaries through the folder's index, which it brings up to date.
fn list(dir: &Path) -> Result<(), Box<dyn Error>> {
    let summaries = Index::open(dir).summaries(&Station::core()?);
    println!("{}", serde_json::to_string_pretty(&summaries)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_takes_a_format_identity_and_output_or_a_folder() {
        let a = export_args(&["--format", "kml", "--identity", "-o", "a.kml"]).unwrap();
        assert_eq!(a.format, Some(Format::Kml));
        assert!(a.options.identity);
        assert_eq!(a.out, Some("a.kml"));
        assert_eq!(export_args(&["out"]).unwrap().dir, Some("out"));
        assert_eq!(export_args(&[]), Some(Export::default()));
        assert!(!export_args(&["-o", "x.csv"]).unwrap().options.identity);
        for bad in [
            &["--format", "xlsx"][..],
            &["--format"],
            &["-o"],
            &["--identity", "--identity"],
            &["--format", "csv", "--format", "kml"],
            &["out", "-o", "x"],
        ] {
            assert!(export_args(bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn slice_takes_a_ref_or_a_range_and_an_output() {
        let ok = |args: &[&str]| slice_args(args).is_some();
        assert!(ok(&["--ref", "Qe3v1hJ0dEa9fP2kLm7xRw"]));
        assert!(ok(&["--ref", "Qe3v1hJ0dEa9fP2kLm7xRw", "-o", "a.fqz"]));
        assert!(ok(&["--from", "10", "--to", "20.5", "-o", "a.fqz"]));
        for bad in [
            &["--ref", "short"][..],
            &["--ref", "Qe3v1hJ0dEa9fP2kLm7xRw", "-o"],
            &["--from", "x", "--to", "2"],
            &["--from", "NaN", "--to", "2"],
            &[],
        ] {
            assert!(!ok(bad), "{bad:?}");
        }
    }

    /// A compact flight past the challenge cap is refused by `challenge` and
    /// `slice`, though the app's own flights may be larger.
    #[test]
    fn challenge_inputs_are_held_to_the_lower_cap() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("big.fqz");
        let mut file = b"FQJC".to_vec();
        file.extend([1, 0, 1, 0]);
        let body = vec![0u8; recorder::CHALLENGE_MAX_BODY as usize + 1];
        file.extend(zstd::encode_all(&body[..], 3).unwrap());
        fs::write(&path, file).unwrap();
        let golden = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/challenge/challenge.json"
        );
        let too_big = "compact flight larger than this reader accepts";
        let err = challenge(Path::new(golden), &path).unwrap_err();
        assert_eq!(err.to_string(), too_big);
        let err = slice(
            &path,
            recorder::Cut::Range(SimTime::ZERO, SimTime::from_micros(1)),
            None,
        );
        assert_eq!(err.unwrap_err().to_string(), too_big);
        let err = Station::core().unwrap().open(&path).err().unwrap();
        assert_eq!(
            err.to_string(),
            "corrupt compact flight",
            "the general cap reads it"
        );
    }

    #[test]
    fn a_challenge_takes_a_compact_flight_only() {
        assert!(uploadable(Path::new("a/attempt.fqz")).is_ok());
        for other in [
            "attempt.fqj",
            "attempt.raw.jsonl.gz",
            "attempt.fqz.gz",
            "attempt",
        ] {
            assert!(uploadable(Path::new(other)).is_err(), "{other}");
        }
    }
}
