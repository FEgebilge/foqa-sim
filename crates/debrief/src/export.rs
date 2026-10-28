//! A flight for other tools (docs/exports.md): Parquet for analysis
//! (`analyzer::export`), CSV for spreadsheets, KML for Google Earth, GPX for
//! map viewers and Little Navmap, and the flight list as a Little Navmap
//! logbook. `fdm export` and the app both write through here.
//!
//! Privacy: nothing written names a file path, and the aircraft's title and
//! tail number appear only with `Options::identity`; the ICAO type always.

use std::fmt::Write as _;
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use analyzer::Flight;
use fdm_core::{SignalDef, SimTime, Unit};

use crate::{
    Analysis, AssessmentDetail, Error, Event, FlightSummary, Grade, Level, Touchdown, iso_utc,
    start_unix,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Parquet,
    Csv,
    Kml,
    Gpx,
}

impl Format {
    pub const ALL: [Self; 4] = [Self::Parquet, Self::Csv, Self::Kml, Self::Gpx];

    /// The file extension, which is also its name on the command line.
    pub const fn ext(self) -> &'static str {
        match self {
            Self::Parquet => "parquet",
            Self::Csv => "csv",
            Self::Kml => "kml",
            Self::Gpx => "gpx",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.ext() == name)
    }
}

/// What an export may say beyond the flight data.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Options {
    /// The aircraft's title (livery) and tail number, as set in the sim.
    pub identity: bool,
}

/// The events file written beside a Parquet export: `x.parquet` →
/// `x.events.parquet`.
pub fn events_path(samples: &Path) -> PathBuf {
    samples.with_extension("events.parquet")
}

/// The second file `format` would write at `path` that already exists: the
/// OS save dialog confirms only `path` itself.
pub fn existing_extra(format: Format, path: &Path) -> Option<PathBuf> {
    Some(events_path(path)).filter(|p| format == Format::Parquet && p.exists())
}

/// Long-format Parquet of the recorded samples, and the events beside it
/// (`events_path`). Needs no analysis, so any readable flight exports.
pub fn parquet(flight: &Flight, options: Options, path: &Path) -> Result<Vec<PathBuf>, Error> {
    let events = events_path(path);
    analyzer::export::write_samples(flight, path)?;
    analyzer::export::write_events(flight, &events, options.identity)?;
    Ok(vec![path.to_owned(), events])
}

/// Writes the logbook to `path`; how many flights it holds.
pub fn logbook_file(
    summaries: &[FlightSummary],
    options: Options,
    path: &Path,
) -> io::Result<usize> {
    let mut out = BufWriter::new(File::create(path)?);
    let n = logbook(summaries, options, &mut out)?;
    out.flush()?;
    Ok(n)
}

/// Track points per second in KML and GPX (the CSV keeps every track point).
const PATH_HZ: usize = 1;
/// Decimals of latitude and longitude: 0.1 m, as the debrief sends them.
const COORD_DECIMALS: usize = 6;
/// Line colours of the flight phases, the app's dark-theme palette
/// (app/src/lib/palette.ts), as `#rrggbb`.
const PHASE_COLORS: [(&str, &str); 8] = [
    ("Standing", "#5d6679"),
    ("Taxi", "#9a8b5e"),
    ("Takeoff", "#34b87e"),
    ("InitialClimb", "#3cbab2"),
    ("EnRoute", "#5294e4"),
    ("Approach", "#9c79e8"),
    ("Landing", "#d963ab"),
    ("GoAround", "#e58f42"),
];
const LEVEL_COLORS: [(&str, &str); 4] = [
    ("high", "#f0605a"),
    ("medium", "#f2a33a"),
    ("low", "#e2cf5c"),
    ("advisory", "#73aee6"),
];
const TOUCHDOWN_COLOR: &str = "#ffffff";

/// One point of the exported path.
struct Point {
    t: SimTime,
    lat: f64,
    lon: f64,
    /// Altitude above mean sea level, m: what the altimeter read.
    alt_m: f64,
    phase: &'static str,
}

impl Analysis<'_> {
    /// Writes `format` to `path`; the files written (Parquet writes two).
    pub fn export(
        &self,
        format: Format,
        options: Options,
        path: &Path,
    ) -> Result<Vec<PathBuf>, Error> {
        if format == Format::Parquet {
            return parquet(&self.flight, options, path);
        }
        let mut out = BufWriter::new(File::create(path)?);
        match format {
            Format::Csv => self.csv(&mut out)?,
            Format::Kml => self.kml(options, &mut out)?,
            Format::Gpx => self.gpx(options, &mut out)?,
            Format::Parquet => unreachable!(),
        }
        out.flush()?;
        Ok(vec![path.to_owned()])
    }

    /// One row per track point (8 Hz): `t_s`, `utc`, `phase`, then every
    /// parameter of the LFL under its own name, in its engineering unit,
    /// interpolated as the analysis reads it; empty where it has no value.
    pub fn csv(&self, out: &mut impl Write) -> io::Result<()> {
        let params: Vec<&SignalDef> = self.flight.parameters().map(|(def, _)| def).collect();
        let mut line = String::from("t_s,utc,phase");
        for def in &params {
            line.push(',');
            line.push_str(def.name);
        }
        writeln!(out, "{line}")?;
        for &t in &self.track.t {
            line.clear();
            let utc = self.utc(t, true).unwrap_or_default();
            let _ = write!(line, "{:.3},{utc},{}", t.as_secs_f64(), self.phase_at(t));
            for def in &params {
                line.push(',');
                if let Some(v) = self.flight.value(def.id, t) {
                    line.push_str(&decimal(v, value_decimals(def)));
                }
            }
            writeln!(out, "{line}")?;
        }
        Ok(())
    }

    /// The path in 3D, a line per phase in its colour; a placemark per
    /// touchdown with its values and per event with its texts.
    pub fn kml(&self, options: Options, out: &mut impl Write) -> io::Result<()> {
        let events = self.events();
        let detail = self.assessment(&events);
        let touchdowns = self.touchdown_views();
        let path = self.path();
        let mut x = String::from(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<kml xmlns=\"http://www.opengis.net/kml/2.2\">\n<Document>\n",
        );
        let _ = writeln!(x, "<name>{}</name>", xml(&self.title(options)));
        let _ = writeln!(
            x,
            "<description>{}</description>",
            html(&[self.about(&detail, events.len())])
        );
        for (phase, color) in PHASE_COLORS {
            let _ = writeln!(
                x,
                "<Style id=\"phase-{phase}\"><LineStyle><color>{}</color><width>4</width></LineStyle><PolyStyle><color>{}</color></PolyStyle></Style>",
                kml_color(color, 0xff),
                kml_color(color, 0x40)
            );
        }
        for (level, color) in LEVEL_COLORS.iter().chain(&[("touchdown", TOUCHDOWN_COLOR)]) {
            let _ = writeln!(
                x,
                "<Style id=\"{level}\"><IconStyle><color>{}</color></IconStyle></Style>",
                kml_color(color, 0xff)
            );
        }

        x.push_str("<Folder>\n<name>Flight path</name>\n");
        for segment in segments(&path) {
            let (first, last) = (&segment[0], &segment[segment.len() - 1]);
            let phase = segment[1.min(segment.len() - 1)].phase;
            let _ = write!(
                x,
                "<Placemark><name>{}</name><styleUrl>#phase-{phase}</styleUrl>",
                phase_name(phase)
            );
            if let (Some(begin), Some(end)) = (self.utc(first.t, false), self.utc(last.t, false)) {
                let _ = write!(
                    x,
                    "<TimeSpan><begin>{begin}</begin><end>{end}</end></TimeSpan>"
                );
            }
            x.push_str("<LineString><extrude>1</extrude><altitudeMode>absolute</altitudeMode><coordinates>");
            for (k, p) in segment.iter().enumerate() {
                let sep = if k == 0 { "" } else { " " };
                let _ = write!(x, "{sep}{},{},{:.1}", coord(p.lon), coord(p.lat), p.alt_m);
            }
            x.push_str("</coordinates></LineString></Placemark>\n");
        }
        x.push_str("</Folder>\n<Folder>\n<name>Touchdowns</name>\n");
        for (label, td) in contact_labels(&touchdowns) {
            let Some(p) = self.point_at(SimTime::from_secs_f64(td.t_s)) else {
                continue;
            };
            let lines = touchdown_lines(td, &detail);
            self.placemark(&mut x, &label, "touchdown", &lines, &p);
        }
        x.push_str("</Folder>\n<Folder>\n<name>Events</name>\n");
        for e in &events {
            let Some(p) = self.point_at(SimTime::from_secs_f64(e.peak_s)) else {
                continue;
            };
            self.placemark(&mut x, &e.title, level_name(e), &event_lines(e), &p);
        }
        x.push_str("</Folder>\n</Document>\n</kml>\n");
        out.write_all(x.as_bytes())
    }

    /// GPX 1.1: a waypoint per touchdown, then the track with elevation and,
    /// when the start is known, time.
    pub fn gpx(&self, options: Options, out: &mut impl Write) -> io::Result<()> {
        let events = self.events();
        let detail = self.assessment(&events);
        let touchdowns = self.touchdown_views();
        let title = xml(&self.title(options));
        let mut x = String::from(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<gpx version=\"1.1\" creator=\"FOQA-Sim\" xmlns=\"http://www.topografix.com/GPX/1/1\">\n",
        );
        let _ = write!(
            x,
            "<metadata><name>{title}</name><desc>{}</desc>",
            xml(&self.about(&detail, events.len()))
        );
        if let Some(start) = self.track.t.first().and_then(|&t| self.utc(t, false)) {
            let _ = write!(x, "<time>{start}</time>");
        }
        x.push_str("</metadata>\n");
        for (label, td) in contact_labels(&touchdowns) {
            let Some(p) = self.point_at(SimTime::from_secs_f64(td.t_s)) else {
                continue;
            };
            let _ = write!(
                x,
                "<wpt lat=\"{}\" lon=\"{}\">{}",
                coord(p.lat),
                coord(p.lon),
                self.gpx_point(&p)
            );
            let desc = touchdown_lines(td, &detail).join(". ");
            let _ = writeln!(
                x,
                "<name>{label}</name><desc>{}</desc><type>Touchdown</type></wpt>",
                xml(&desc)
            );
        }
        let _ = writeln!(x, "<trk><name>{title}</name><type>Flight</type><trkseg>");
        for p in self.path() {
            let _ = writeln!(
                x,
                "<trkpt lat=\"{}\" lon=\"{}\">{}</trkpt>",
                coord(p.lat),
                coord(p.lon),
                self.gpx_point(&p)
            );
        }
        x.push_str("</trkseg></trk>\n</gpx>\n");
        out.write_all(x.as_bytes())
    }

    fn gpx_point(&self, p: &Point) -> String {
        let time = self
            .utc(p.t, true)
            .map_or_else(String::new, |t| format!("<time>{t}</time>"));
        format!("<ele>{:.1}</ele>{time}", p.alt_m)
    }

    fn placemark(&self, x: &mut String, name: &str, style: &str, lines: &[String], p: &Point) {
        let _ = write!(
            x,
            "<Placemark><name>{}</name><description>{}</description><styleUrl>#{style}</styleUrl>",
            xml(name),
            html(lines)
        );
        if let Some(t) = self.utc(p.t, false) {
            let _ = write!(x, "<TimeStamp><when>{t}</when></TimeStamp>");
        }
        let _ = writeln!(
            x,
            "<Point><altitudeMode>absolute</altitudeMode><coordinates>{},{},{:.1}</coordinates></Point></Placemark>",
            coord(p.lon),
            coord(p.lat),
            p.alt_m
        );
    }

    /// UTC at sim time `t`: the recording's start (from its file name) plus
    /// the sim time since; a pause or a sim rate other than 1 shifts it.
    /// `None` when the start is not known.
    fn utc(&self, t: SimTime, millis: bool) -> Option<String> {
        Some(iso_utc(
            start_unix(&self.id)? * 1_000_000 + t.as_micros(),
            millis,
        ))
    }

    fn phase_at(&self, t: SimTime) -> &'static str {
        self.phases
            .iter()
            .find(|s| t < s.end)
            .or(self.phases.last())
            .map_or("", |s| s.phase.name())
    }

    /// `C172 · LTFA → LTFJ · 28 Oct 2026`, with the title and tail when asked.
    fn title(&self, options: Options) -> String {
        let aircraft = self.flight.aircraft();
        let mut name = aircraft
            .and_then(|a| a.icao_type())
            .unwrap_or_else(|| "Aircraft".into());
        if let Some(id) = aircraft.filter(|_| options.identity) {
            for part in [&id.title, &id.atc_id] {
                if !part.trim().is_empty() {
                    let _ = write!(name, " {}", part.trim());
                }
            }
        }
        let summary = self.summary();
        let route = |icao: &Option<String>| icao.clone().unwrap_or_else(|| "----".into());
        let date = summary
            .started_utc
            .as_deref()
            .map_or(String::new(), |d| format!(" · {}", &d[..10]));
        format!(
            "{name} · {} → {}{date}",
            route(&summary.departure),
            route(&summary.arrival)
        )
    }

    /// `FOQA-Sim debrief: Good, 87.3 of 100. 2 landings, 3 events.`
    fn about(&self, detail: &AssessmentDetail, events: usize) -> String {
        let score = detail
            .score
            .map_or(String::new(), |s| format!(", {s:.1} of 100"));
        let landings = self.touchdowns.iter().filter(|td| !td.bounce).count();
        format!(
            "FOQA-Sim debrief: {}{score}. {landings} landing{}, {} event{}.",
            grade_name(detail.grade),
            plural(landings),
            events,
            plural(events)
        )
    }

    fn touchdown_views(&self) -> Vec<Touchdown> {
        self.touchdowns
            .iter()
            .map(|td| Touchdown::new(td, &self.flight))
            .collect()
    }

    /// The path at `PATH_HZ` plus every touchdown's own point, where the
    /// position and altitude are known.
    fn path(&self) -> Vec<Point> {
        let step = (analyzer::TRACK_HZ as usize / PATH_HZ).max(1);
        let mut at: Vec<usize> = (0..self.track.t.len()).step_by(step).collect();
        at.extend(self.touchdowns.iter().map(|td| self.index(td.t)));
        at.sort_unstable();
        at.dedup();
        at.into_iter().filter_map(|i| self.point(i)).collect()
    }

    fn index(&self, t: SimTime) -> usize {
        let i = self.track.t.partition_point(|&x| x < t);
        i.min(self.track.t.len().saturating_sub(1))
    }

    fn point_at(&self, t: SimTime) -> Option<Point> {
        self.point(self.index(t))
    }

    fn point(&self, i: usize) -> Option<Point> {
        let t = *self.track.t.get(i)?;
        let (lat, lon) = (self.flight.at("pos.lat", t)?, self.flight.at("pos.lon", t)?);
        let pressure_ft = self.track.alt_ft[i];
        // (0, 0) is where the sim reports a position before a flight loads.
        if !pressure_ft.is_finite() || (lat == 0.0 && lon == 0.0) {
            return None;
        }
        let baro = self.flight.at("baro.setting", t);
        Some(Point {
            t,
            lat,
            lon,
            alt_m: indicated_ft(pressure_ft, baro) * 0.3048,
            phase: self.phase_at(t),
        })
    }
}

/// The altimeter's reading at `pressure_ft` with `baro_hpa` set (ISA, the
/// pressure altitude of the setting subtracted); pressure altitude without
/// a setting.
fn indicated_ft(pressure_ft: f64, baro_hpa: Option<f64>) -> f64 {
    let setting_ft = |p: f64| 145_366.45 * (1.0 - (p / 1013.25).powf(0.190_284));
    match baro_hpa.filter(|p| (800.0..1100.0).contains(p)) {
        Some(p) => pressure_ft - setting_ft(p),
        None => pressure_ft,
    }
}

/// The path cut into runs of one phase; each run after the first starts at
/// the previous run's last point, so the line stays joined.
fn segments(path: &[Point]) -> Vec<&[Point]> {
    let mut out = Vec::new();
    let mut start = 0usize;
    for i in 1..=path.len() {
        if i == path.len() || path[i].phase != path[i - 1].phase {
            let from = start.saturating_sub(1);
            if i - from >= 2 {
                out.push(&path[from..i]);
            }
            start = i;
        }
    }
    out
}

/// `TD1` for a landing's first contact, `TD1b`, `TD1c`… for its bounces,
/// as the app labels them.
fn contact_labels(touchdowns: &[Touchdown]) -> Vec<(String, &Touchdown)> {
    let (mut landing, mut bounce) = (0, 0u8);
    touchdowns
        .iter()
        .map(|td| {
            if td.bounce && landing > 0 {
                bounce += 1;
            } else {
                landing += 1;
                bounce = 0;
            }
            let suffix = match bounce {
                0 => String::new(),
                b => char::from(b'a' + b).to_string(),
            };
            (format!("TD{landing}{suffix}"), td)
        })
        .collect()
}

fn touchdown_lines(td: &Touchdown, detail: &AssessmentDetail) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(l) = detail
        .landings
        .iter()
        .find(|l| (l.t_s - td.t_s).abs() < 1e-3)
        && let (Some(score), Some(grade)) = (l.score, l.grade)
    {
        lines.push(format!(
            "Landing score {score:.1} of 100, {}",
            grade_name(grade)
        ));
    }
    lines.push(format!(
        "Sink rate {:.0} fpm, peak load {:.2} g",
        td.sink_fpm, td.peak_g
    ));
    if let Some(r) = &td.runway {
        let side = if r.cross_m < 0.0 { "left" } else { "right" };
        lines.push(format!(
            "{} {}: {:.0} m past the threshold ({}), {:.1} m {side} of the centreline",
            r.icao,
            r.runway,
            r.along_m,
            r.zone,
            r.cross_m.abs()
        ));
    }
    if let Some(w) = &td.wind {
        let along = if w.headwind_kt < 0.0 {
            "tailwind"
        } else {
            "headwind"
        };
        let side = if w.crosswind_kt < 0.0 {
            "left"
        } else {
            "right"
        };
        lines.push(format!(
            "Wind {:03.0}° {:.0} kt: {:.0} kt {along}, {:.0} kt from the {side}",
            w.from_deg,
            w.speed_kt,
            w.headwind_kt.abs(),
            w.crosswind_kt.abs()
        ));
    }
    if td.bounce {
        lines.push("A bounce: a further contact of the landing before it".into());
    }
    lines
}

fn event_lines(e: &Event) -> Vec<String> {
    let mut lines = vec![format!("{} · T+{:.0} s", level_word(e), e.start_s)];
    lines.extend(e.detail.clone());
    lines.extend(
        [&e.description]
            .into_iter()
            .filter(|d| !d.is_empty())
            .cloned(),
    );
    lines.extend(
        e.why_it_matters
            .as_ref()
            .map(|w| format!("Why it matters: {w}")),
    );
    lines.extend(e.advice.as_ref().map(|a| format!("What to do: {a}")));
    lines
}

/// Columns of a Little Navmap logbook CSV, in its order (atools
/// `LogdataManager`, `csv::Index`), with the header its own export writes.
pub const LOGBOOK_COLUMNS: [&str; 36] = [
    "Aircraft Name",
    "Aircraft Type",
    "Aircraft Registration",
    "Flightplan Number",
    "Flightplan Cruise Altitude",
    "Flightplan File",
    "Performance File",
    "Block Fuel",
    "Trip Fuel",
    "Used Fuel",
    "Is Jetfuel",
    "Gross Weight",
    "Distance",
    "Distance Flown",
    "Departure Ident",
    "Departure Name",
    "Departure Runway",
    "Departure Lonx",
    "Departure Laty",
    "Departure Alt",
    "Departure Time",
    "Departure Time Sim",
    "Destination Ident",
    "Destination Name",
    "Destination Runway",
    "Destination Lonx",
    "Destination Laty",
    "Destination Alt",
    "Destination Time",
    "Destination Time Sim",
    "Route String",
    "Simulator",
    "Description",
    "Flightplan",
    "Aircraft Perf",
    "Aircraft Trail",
];

/// The flights as a Little Navmap logbook CSV (docs/exports.md), in the
/// order given; how many it holds. A flight with neither a departure nor an
/// arrival airport is left out: Little Navmap refuses such a line, and with
/// it the whole file.
pub fn logbook(
    summaries: &[FlightSummary],
    options: Options,
    out: &mut impl Write,
) -> io::Result<usize> {
    writeln!(out, "{}", LOGBOOK_COLUMNS.join(","))?;
    let col = |name: &str| LOGBOOK_COLUMNS.iter().position(|c| *c == name).unwrap();
    let mut n = 0;
    for f in summaries
        .iter()
        .filter(|f| f.departure.is_some() || f.arrival.is_some())
    {
        let mut row = vec![String::new(); LOGBOOK_COLUMNS.len()];
        if let Some(a) = &f.aircraft {
            if options.identity {
                row[col("Aircraft Name")] = a.title.clone();
            }
            row[col("Aircraft Type")] = a.icao.clone().unwrap_or_default();
        }
        row[col("Departure Ident")] = f.departure.clone().unwrap_or_default();
        row[col("Destination Ident")] = f.arrival.clone().unwrap_or_default();
        if let Some(unix) = start_unix(&f.id) {
            let end_us = unix * 1_000_000 + (f.duration_s * 1e6) as i64;
            row[col("Departure Time")] = iso_utc(unix * 1_000_000, false);
            row[col("Destination Time")] = iso_utc(end_us, false);
        }
        row[col("Simulator")] = "MSFS 2024".into();
        row[col("Description")] = logbook_note(f);
        let line: Vec<String> = row.iter().map(|v| csv_field(v)).collect();
        writeln!(out, "{}", line.join(","))?;
        n += 1;
    }
    Ok(n)
}

/// `FOQA-Sim flight-1793027213: Good, 87.3 of 100. 2 landings, 1 medium and 3 low events.`
fn logbook_note(f: &FlightSummary) -> String {
    let score = f
        .assessment
        .score
        .map_or(String::new(), |s| format!(", {s:.1} of 100"));
    let counts: Vec<String> = f
        .event_counts
        .iter()
        .rev()
        .filter(|(_, n)| **n > 0)
        .map(|(level, n)| format!("{n} {}", format!("{level:?}").to_lowercase()))
        .collect();
    let events = match counts.len() {
        0 => "no events".to_owned(),
        _ => format!(
            "{} event{}",
            counts.join(", "),
            plural(f.event_counts.values().sum())
        ),
    };
    format!(
        "FOQA-Sim {}: {}{score}. {} landing{}, {events}.",
        f.id,
        grade_name(f.assessment.grade),
        f.landings,
        plural(f.landings)
    )
}

/// A CSV text field, quoted when it holds a comma, a quote or a line break.
/// One a spreadsheet would run as a formula (`= + - @`, tab, CR) gets a
/// leading apostrophe (OWASP CSV injection); numbers never pass through here.
fn csv_field(v: &str) -> String {
    if v.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        return csv_field(&format!("'{v}"));
    }
    match v.contains([',', '"', '\n', '\r']) {
        true => format!("\"{}\"", v.replace('"', "\"\"")),
        false => v.to_owned(),
    }
}

/// `v` to `decimals`, without trailing zeros: `12.5`, `3`, `-0.25`.
/// Empty for NaN and infinity.
fn decimal(v: f64, decimals: usize) -> String {
    if !v.is_finite() {
        return String::new();
    }
    let s = format!("{v:.decimals$}");
    let s = match s.contains('.') {
        true => s.trim_end_matches('0').trim_end_matches('.'),
        false => &s,
    };
    match s {
        "-0" => "0".into(),
        s => s.into(),
    }
}

fn value_decimals(def: &SignalDef) -> usize {
    match def.unit {
        Unit::Discrete | Unit::Index => 2,
        _ if def.name.starts_with("pos.") => COORD_DECIMALS,
        _ => 4,
    }
}

fn coord(v: f64) -> String {
    format!("{v:.COORD_DECIMALS$}")
}

/// `#rrggbb` as KML's `aabbggrr`.
fn kml_color(hex: &str, alpha: u8) -> String {
    let h = hex.trim_start_matches('#');
    format!("{alpha:02x}{}{}{}", &h[4..6], &h[2..4], &h[0..2])
}

/// Text for XML content and attributes.
fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Lines as a KML balloon: HTML in CDATA, each line escaped.
fn html(lines: &[String]) -> String {
    let body: Vec<String> = lines.iter().map(|l| xml(l)).collect();
    format!("<![CDATA[{}]]>", body.join("<br/>"))
}

/// `InitialClimb` → `Initial climb`, as the app writes phases.
fn phase_name(phase: &str) -> String {
    let mut out = String::new();
    for (i, c) in phase.chars().enumerate() {
        if i > 0 && c.is_uppercase() {
            out.push(' ');
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

fn level_name(e: &Event) -> &'static str {
    match e.level {
        Level::High => "high",
        Level::Medium => "medium",
        Level::Low => "low",
        Level::Advisory => "advisory",
    }
}

fn level_word(e: &Event) -> &'static str {
    match e.level {
        Level::High => "High",
        Level::Medium => "Medium",
        Level::Low => "Low",
        Level::Advisory => "Advisory",
    }
}

fn grade_name(g: Grade) -> &'static str {
    match g {
        Grade::Good => "Good",
        Grade::Fair => "Fair",
        Grade::Poor => "Poor",
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

#[cfg(test)]
mod tests {
    use std::sync::OnceLock;

    use recorder::SimEvent;
    use testkit::*;

    use super::*;
    use crate::Station;

    const ID: &str = "flight-1793027213";
    const PRIVATE: [&str; 4] = ["N123FQ", "B738 test", "Users", "Documents"];

    fn station() -> &'static Station {
        static STATION: OnceLock<Station> = OnceLock::new();
        STATION.get_or_init(|| Station::core().unwrap())
    }

    /// The synthetic airliner landing on runway 20 of `TEST`, with what a
    /// real flight carries besides: the flight file and the identity.
    fn landed() -> &'static Analysis<'static> {
        static LANDED: OnceLock<Analysis<'static>> = OnceLock::new();
        LANDED.get_or_init(|| {
            let events = [
                (
                    0.0,
                    SimEvent::FlightLoaded(r"C:\Users\Ege\Documents\b738.FLT".into()),
                ),
                (0.0, identity("B738")),
                (0.0, SimEvent::Airport(runway_behind_touchdown(400.0))),
                (60.0, SimEvent::PilotReport("long flare".into())),
            ];
            let journal = journal(&events, AIRLINER_LB, TD_S + ROLLOUT_S, |t| {
                landing(t, TD_S, 1.3)
            });
            let flight = Flight::read(&journal[..]).unwrap();
            station().analyse(ID.into(), flight).unwrap()
        })
    }

    fn text(write: impl Fn(&mut Vec<u8>) -> io::Result<()>) -> String {
        let mut out = Vec::new();
        write(&mut out).unwrap();
        String::from_utf8(out).unwrap()
    }

    /// Tags open and close in order: the document is well formed as far as
    /// elements go. CDATA sections and the declaration are skipped.
    fn balanced(doc: &str) -> Result<(), String> {
        let mut open: Vec<&str> = Vec::new();
        let mut rest = doc;
        while let Some(at) = rest.find('<') {
            rest = &rest[at..];
            if let Some(cdata) = rest.strip_prefix("<![CDATA[") {
                rest = &cdata[cdata.find("]]>").ok_or("open CDATA")? + 3..];
                continue;
            }
            let end = rest.find('>').ok_or("open tag")?;
            let tag = &rest[1..end];
            rest = &rest[end + 1..];
            if tag.starts_with('?') || tag.ends_with('/') {
                continue;
            }
            match tag.strip_prefix('/') {
                Some(name) => match open.pop() {
                    Some(o) if o == name => {}
                    o => return Err(format!("</{name}> closes {o:?}")),
                },
                None => open.push(tag.split_whitespace().next().unwrap_or(tag)),
            }
        }
        match open.is_empty() {
            true => Ok(()),
            false => Err(format!("unclosed {open:?}")),
        }
    }

    /// Every `<tag>…</tag>` text in the document.
    fn inner<'a>(doc: &'a str, tag: &str) -> Vec<&'a str> {
        let (open, close) = (format!("<{tag}>"), format!("</{tag}>"));
        doc.split(&open)
            .skip(1)
            .map(|s| &s[..s.find(&close).unwrap()])
            .collect()
    }

    /// A CSV line's fields, quotes undone.
    fn fields(line: &str) -> Vec<String> {
        let (mut out, mut field, mut quoted) = (Vec::new(), String::new(), false);
        let mut chars = line.chars().peekable();
        while let Some(c) = chars.next() {
            match (c, quoted) {
                ('"', true) if chars.peek() == Some(&'"') => {
                    field.push('"');
                    chars.next();
                }
                ('"', _) => quoted = !quoted,
                (',', false) => out.push(std::mem::take(&mut field)),
                (c, _) => field.push(c),
            }
        }
        out.push(field);
        out
    }

    fn decimals(v: &str) -> usize {
        v.split_once('.').map_or(0, |(_, d)| d.len())
    }

    #[test]
    fn csv_has_a_row_per_track_point_under_the_lfl_names() {
        let a = landed();
        let csv = text(|w| a.csv(w));
        let mut lines = csv.lines();
        let header: Vec<&str> = lines.next().unwrap().split(',').collect();
        let names: Vec<&str> = fdm_core::Lfl::V1.signals().iter().map(|d| d.name).collect();
        assert_eq!(header[..3], ["t_s", "utc", "phase"]);
        assert_eq!(header[3..], names[..]);
        let rows: Vec<Vec<&str>> = lines.map(|l| l.split(',').collect()).collect();
        assert_eq!(rows.len(), a.track.t.len());
        assert!(rows.iter().all(|r| r.len() == header.len()));
        let col = |name: &str| header.iter().position(|h| *h == name).unwrap();
        // UTC: the file name's start plus sim time, to the millisecond.
        assert_eq!(rows[0][col("t_s")], "0.000");
        assert_eq!(rows[0][col("utc")], "2026-10-26T15:06:53.000Z");
        assert_eq!(rows[9][col("t_s")], "1.125");
        assert_eq!(rows[9][col("utc")], "2026-10-26T15:06:54.125Z");
        assert!(rows.iter().all(|r| r[col("utc")].ends_with('Z')));
        // Values in the LFL's units: pressure altitude in feet.
        let alt: f64 = rows[0][col("alt.pressure")].parse().unwrap();
        assert!((alt - (FIELD_FT + 1500.0)).abs() < 1.0, "{alt}");
        assert!(
            rows.iter()
                .all(|r| decimals(r[col("pos.lat")]) <= COORD_DECIMALS)
        );
        assert!(rows.iter().any(|r| r[col("phase")] == "Landing"));
        for private in PRIVATE {
            assert!(!csv.contains(private), "{private}");
        }
    }

    #[test]
    fn kml_is_a_3d_path_by_phase_with_touchdown_and_event_placemarks() {
        let a = landed();
        let kml = text(|w| a.kml(Options::default(), w));
        balanced(&kml).unwrap();
        assert!(kml.starts_with(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<kml xmlns=\"http://www.opengis.net/kml/2.2\">"
        ));
        assert!(
            kml.contains("<name>B738 · ---- → TEST · 2026-10-26</name>"),
            "{}",
            &kml[..400]
        );
        assert!(kml.contains("<styleUrl>#phase-Approach</styleUrl>"));
        assert!(kml.contains("<styleUrl>#phase-Landing</styleUrl>"));
        let td = kml.split("<Placemark><name>TD1</name>").nth(1).unwrap();
        let td = &td[..td.find("</Placemark>").unwrap()];
        assert!(td.contains("TEST 20: "), "{td}");
        assert!(td.contains("Sink rate"), "{td}");
        assert!(td.contains("<styleUrl>#touchdown</styleUrl>"), "{td}");
        let placemarks = segments(&a.path()).len() + a.touchdowns.len() + a.events().len();
        assert_eq!(kml.matches("<Placemark>").count(), placemarks);
        for coords in inner(&kml, "coordinates") {
            for point in coords.split(' ') {
                let [lon, lat, alt] = point.split(',').collect::<Vec<_>>()[..] else {
                    panic!("{point}")
                };
                let places = (decimals(lon), decimals(lat), decimals(alt));
                assert_eq!(places, (6, 6, 1), "{point}");
                let (lon, lat): (f64, f64) = (lon.parse().unwrap(), lat.parse().unwrap());
                assert!((lat - START.0).abs() < 0.2, "{point}");
                assert!((lon - START.1).abs() < 0.2, "{point}");
            }
        }
        let begins = inner(&kml, "begin");
        assert!(!begins.is_empty());
        assert!(
            begins
                .iter()
                .all(|t| t.starts_with("2026-10-26T") && t.ends_with('Z'))
        );
        for private in PRIVATE {
            assert!(!kml.contains(private), "{private}");
        }
        let named = text(|w| a.kml(Options { identity: true }, w));
        assert!(named.contains("<name>B738 B738 test N123FQ · ---- → TEST · 2026-10-26</name>"));
        assert!(!named.contains("Users"));
    }

    #[test]
    fn gpx_is_a_timed_track_with_elevation_and_touchdown_waypoints() {
        let a = landed();
        let gpx = text(|w| a.gpx(Options::default(), w));
        balanced(&gpx).unwrap();
        assert!(gpx.contains(
            "<gpx version=\"1.1\" creator=\"FOQA-Sim\" xmlns=\"http://www.topografix.com/GPX/1/1\">"
        ));
        // GPX 1.1 order: metadata, waypoints, then the track.
        let meta = gpx.find("<metadata>").unwrap();
        let (wpt, trk) = (gpx.find("<wpt ").unwrap(), gpx.find("<trk>").unwrap());
        assert!(meta < wpt && wpt < trk);
        assert!(inner(&gpx, "metadata")[0].contains("<time>2026-10-26T15:06:53Z</time>"));
        let points = gpx.matches("<trkpt ").count();
        assert_eq!(points, a.path().len());
        assert!(points as f64 >= (TD_S + ROLLOUT_S) * PATH_HZ as f64);
        let track = &gpx[trk..];
        let times = inner(track, "time");
        assert_eq!(times.len(), points);
        assert!(times.iter().all(|t| t.len() == 24 && t.ends_with('Z')));
        assert!(times.windows(2).all(|w| w[0] < w[1]), "time order");
        let ele: f64 = inner(track, "ele")[0].parse().unwrap();
        assert!((ele - (FIELD_FT + 1500.0) * 0.3048).abs() < 2.0, "{ele}");
        for private in PRIVATE {
            assert!(!gpx.contains(private), "{private}");
        }
    }

    #[test]
    fn export_writes_the_file_of_each_format() {
        let tmp = tempfile::tempdir().unwrap();
        for format in Format::ALL {
            let path = tmp.path().join(format!("x.{}", format.ext()));
            let written = landed().export(format, Options::default(), &path).unwrap();
            assert_eq!(written[0], path);
            assert!(written.iter().all(|p| p.is_file()));
        }
        let events = events_path(Path::new("a/x.parquet"));
        assert_eq!(events, Path::new("a/x.events.parquet"));
        assert_eq!(Format::parse("gpx"), Some(Format::Gpx));
        assert_eq!(Format::parse("xlsx"), None);
    }

    #[test]
    fn logbook_is_little_navmaps_csv() {
        let summary = landed().summary();
        let mut nowhere = summary.clone();
        nowhere.departure = None;
        nowhere.arrival = None;
        let list = [summary, nowhere];
        let mut out = Vec::new();
        assert_eq!(logbook(&list, Options::default(), &mut out).unwrap(), 1);
        let csv = String::from_utf8(out).unwrap();
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines.len(), 2);
        // The header Little Navmap's import skips: lower-cased and without
        // spaces, it starts with these columns.
        let header = lines[0].replace(' ', "").to_lowercase();
        assert!(
            header.starts_with("aircraftname,aircrafttype,aircraftregistration,flightplannumber,")
        );
        let row = fields(lines[1]);
        assert_eq!(row.len(), 36);
        let col = |name| LOGBOOK_COLUMNS.iter().position(|c| *c == name).unwrap();
        assert_eq!(row[col("Aircraft Name")], "");
        assert_eq!(row[col("Aircraft Type")], "B738");
        assert_eq!(row[col("Departure Ident")], "");
        assert_eq!(row[col("Destination Ident")], "TEST");
        assert_eq!(row[col("Departure Time")], "2026-10-26T15:06:53Z");
        let end = iso_utc(
            (1_793_027_213 + list[0].duration_s as i64) * 1_000_000,
            false,
        );
        assert_eq!(row[col("Destination Time")], end);
        assert_eq!(row[col("Simulator")], "MSFS 2024");
        let note = &row[col("Description")];
        assert!(note.starts_with("FOQA-Sim flight-1793027213: "), "{note}");
        let mut named = Vec::new();
        logbook(&list[..1], Options { identity: true }, &mut named).unwrap();
        assert!(
            String::from_utf8(named)
                .unwrap()
                .contains("\nB738 test,B738,")
        );
    }

    /// docs/exports.md lists every CSV column with its unit.
    #[test]
    fn the_csv_columns_are_documented() {
        let doc = include_str!("../../../docs/exports.md");
        for def in fdm_core::Lfl::V1.signals() {
            let unit = match def.unit {
                Unit::Discrete => "0/1",
                Unit::Index => "index",
                Unit::Ratio => "ratio",
                u => u.symbol(),
            };
            let row = format!("| `{}` | {unit} |", def.name);
            assert!(doc.contains(&row), "{row}");
        }
    }

    #[test]
    fn values_are_written_plainly() {
        assert_eq!(decimal(12.5, 4), "12.5");
        assert_eq!(decimal(3.0, 4), "3");
        assert_eq!(decimal(-0.00001, 4), "0");
        assert_eq!(decimal(38.123_456_78, 6), "38.123457");
        assert_eq!(csv_field("a, \"b\""), "\"a, \"\"b\"\"\"");
        assert_eq!(csv_field("plain"), "plain");
        assert_eq!(decimal(f64::NAN, 4), "");
        assert_eq!(decimal(f64::INFINITY, 4), "");
        assert_eq!(decimal(f64::NEG_INFINITY, 4), "");
    }

    #[test]
    fn text_a_spreadsheet_would_run_is_defused() {
        for lead in ["=", "+", "-", "@", "\t", "\r"] {
            let field = csv_field(&format!("{lead}SUM(A1)"));
            assert!(field.trim_start_matches('"').starts_with('\''), "{field:?}");
        }
        assert_eq!(csv_field("=1,2"), "\"'=1,2\"");
        assert_eq!(csv_field("a=b"), "a=b");
    }

    #[test]
    fn a_second_parquet_file_is_reported_only_when_it_exists() {
        let dir = std::env::temp_dir().join(format!("foqa-extra-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("x.parquet");
        assert_eq!(existing_extra(Format::Parquet, &out), None);
        std::fs::write(events_path(&out), b"").unwrap();
        assert_eq!(
            existing_extra(Format::Parquet, &out),
            Some(events_path(&out))
        );
        assert_eq!(existing_extra(Format::Csv, &out), None);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(kml_color("#34b87e", 0x40), "407eb834");
        assert_eq!(xml("a<b & \"c\""), "a&lt;b &amp; &quot;c&quot;");
        assert_eq!(html(&["x]]>".into()]), "<![CDATA[x]]&gt;]]>");
        assert_eq!(phase_name("InitialClimb"), "Initial climb");
        // The altimeter: the standard setting reads pressure altitude, a
        // higher QNH reads higher, about 27 ft per hPa near sea level.
        assert_eq!(indicated_ft(1000.0, Some(1013.25)), 1000.0);
        assert_eq!(indicated_ft(1000.0, None), 1000.0);
        let qnh = indicated_ft(1000.0, Some(1023.25)) - 1000.0;
        assert!((qnh - 273.0).abs() < 5.0, "{qnh}");
    }

    #[test]
    fn contacts_are_labelled_as_the_app_does() {
        let td = |bounce| Touchdown {
            t_s: 0.0,
            bounce,
            sink_fpm: 0.0,
            sim_sink_fpm: None,
            peak_g: 1.0,
            pitch_deg: None,
            roll_deg: None,
            ias_kt: None,
            runway: None,
            wind: None,
        };
        let tds = [td(false), td(true), td(true), td(false)];
        let labels: Vec<String> = contact_labels(&tds).into_iter().map(|(l, _)| l).collect();
        assert_eq!(labels, ["TD1", "TD1b", "TD1c", "TD2"]);
    }
}
