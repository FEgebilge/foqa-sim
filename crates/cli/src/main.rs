//! fdm: ground-segment command line.
//!
//! `fdm analyze <flight> [--rules file.yaml]`   phases, touchdowns, events
//! `fdm debrief <flight> [--json]`              the app's debrief, as JSON
//! `fdm export <flight> [out-dir]`              long-format Parquet (+ events)
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
//! `--rules`, the built-in core rule set is used.

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use analyzer::{RunwayFix, export};
use debrief::{Analysis, Index, Station, flight_id};
use fdm_core::SimTime;
use rules::Event;

const USAGE: &str =
    "usage: fdm analyze <flight.fqj|flight.fqz|capture.raw.jsonl.gz> [--rules file.yaml]
       fdm debrief <flight> [--json]
       fdm export <flight> [out-dir]
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
        ["export", path] => export(Path::new(path), Path::new(".")),
        ["export", path, out] => export(Path::new(path), Path::new(out)),
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

fn export(path: &Path, out: &Path) -> Result<(), Box<dyn Error>> {
    let flight = debrief::load(path)?;
    let base: PathBuf = out.join(flight_id(path));
    let samples = base.with_extension("parquet");
    let events = base.with_extension("events.parquet");
    export::write_samples(&flight, &samples)?;
    export::write_events(&flight, &events)?;
    println!("{}\n{}", samples.display(), events.display());
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
    let attempt = challenge::evaluate(&challenge, &open(&station, path)?);
    println!("{}", serde_json::to_string_pretty(&attempt)?);
    Ok(())
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
    let sliced = recorder::slice(fs::File::open(path)?, cut)?.ok_or_else(|| match cut {
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
