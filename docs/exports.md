# Exports

A flight leaves FOQA-Sim only as a file the pilot asks for: from the debrief's **Export / Share**
menu, the flight list's **Export** menu, or `fdm`. Every export is written on this PC and nothing
is uploaded. One implementation (`crates/debrief/src/export.rs`, Parquet in
`crates/analyzer/src/export.rs`) serves the app and the CLI, so both write the same bytes.

| Format | For | What it holds |
|---|---|---|
| [CSV](#csv) | Spreadsheets, pandas | One row per track point (8 Hz), every LFL parameter |
| [KML](#kml) | Google Earth | The 3D path coloured by phase, touchdowns and findings as placemarks |
| [GPX](#gpx) | Little Navmap, online map viewers | The track with time and elevation, touchdowns as waypoints |
| Parquet | DuckDB, pandas | Every recorded sample at its own rate, events beside it (below) |
| [Logbook](#little-navmap-logbook) | Little Navmap's logbook | The flight list, one line per flight |
| [Landing card](#landing-card) | Sharing | A PNG of one landing's score and touchdown values |

## Privacy

- No export names a file path. The flight file the sim loaded (`FlightLoaded`, a local path) is
  dropped from the Parquet events file too.
- The aircraft is named by its ICAO type (`C172`). Its sim title (livery) and tail number are
  added only when asked: the menu's "Include the aircraft's title and tail number" (off each
  time the menu opens), `--identity` on the command line. The landing card never names them.
- Pilot reports (moments marked in the in-sim panel, with their notes) are findings and appear
  in the KML like any other. The Parquet events file carries a pilot note only with identity,
  and never an attempt's reference.
- Spreadsheets run text that starts with `=`, `+`, `-`, `@`, a tab or a CR as a formula: CSV
  text fields (the logbook's) get a leading apostrophe. Numbers are written as they are; a
  value that is not finite is an empty cell.

## CLI

```powershell
fdm export <flight> [--format parquet|csv|kml|gpx] [--identity] [-o file]
fdm export <flight> <out-dir>                       # Parquet into a folder, as before
fdm logbook <folder|flight> [--identity] [-o file]  # logbook.csv by default
```

Without `-o` the file is `<flight id>.<format>` in the current folder. Parquet stays the
default and writes `<name>.parquet` and `<name>.events.parquet` (the CLI overwrites both; the
app's save dialog confirms the first file, so the app asks again before replacing an existing
events file, and cancelling writes nothing); it needs no analysis, so it
also exports a recording without air/ground data. `fdm` prints the files it wrote. For a
folder, `logbook` reads the flights through its index (`index.json`), like `fdm list`.

## Times

Track times are sim time since the recording started. A UTC time is that start (the wall clock
in the flight's file name, `flight-<unix seconds>`) plus the sim time since, in ISO 8601 with a
`Z`: `2026-10-26T15:06:53.125Z`. A pause or a sim rate other than 1 shifts it from the wall
clock, and the sim's own zulu time (`time.zulu`) can differ from both. A flight whose file name
carries no start (a renamed or test file) has no UTC times: empty in the CSV, left out of KML
and GPX.

## CSV

UTF-8, comma-separated, `.` decimals, one header line, LF line ends. Columns: `t_s` (sim time,
s, 3 decimals), `utc` (as above, ms), `phase` (the CICTT phase, [phases.md](phases.md), as the
debrief names it: `Approach`, `InitialClimb`…), then every parameter of the LFL in its order,
under its own name and in its engineering unit. Rows are the analysis track (8 Hz, the rate
of normal acceleration). A parameter recorded more slowly is read the way the analysis reads
it: interpolated between valid neighbours, the last value for discrete ones, and empty across
an invalid sample or a gap of more than two missed samples. Latitude and longitude carry 6
decimals (0.1 m), discretes 2, the rest 4, trailing zeros dropped. For each sample at its own
rate, use Parquet.

| Column | Unit | Recorded at |
|---|---|---|
| `time.zulu` | s | 1/4 Hz |
| `alt.pressure` | ft | 1 Hz |
| `ias` | kt | 1 Hz |
| `heading.mag` | deg | 1 Hz |
| `groundspeed` | kt | 1 Hz |
| `pos.lat` | deg | 1 Hz |
| `pos.lon` | deg | 1 Hz |
| `accel.normal` | g | 8 Hz |
| `accel.long` | g | 4 Hz |
| `accel.lat` | g | 4 Hz |
| `pitch` | deg | 4 Hz |
| `roll` | deg | 2 Hz |
| `aoa` | deg | 2 Hz |
| `sideslip` | deg | 1 Hz |
| `alt.radio` | ft | 1 Hz |
| `nav.loc_dev` | dot | 1 Hz |
| `nav.gs_dev` | dot | 1 Hz |
| `air_ground` | 0/1 | 4 Hz |
| `flap.handle` | index | 0.5 Hz |
| `flap.te_angle` | deg | 0.5 Hz |
| `gear.handle` | 0/1 | 1/4 Hz |
| `gear.extended` | % | 1/4 Hz |
| `spoiler.armed` | 0/1 | 2 Hz |
| `spoiler.handle` | % | 2 Hz |
| `trim.pitch` | deg | 1 Hz |
| `ctl.pitch` | % | 8 Hz |
| `ctl.roll` | % | 8 Hz |
| `ctl.yaw` | % | 8 Hz |
| `eng1.n1` | % | 1 Hz |
| `eng2.n1` | % | 1 Hz |
| `eng3.n1` | % | 1 Hz |
| `eng4.n1` | % | 1 Hz |
| `eng1.throttle` | % | 1 Hz |
| `eng2.throttle` | % | 1 Hz |
| `eng3.throttle` | % | 1 Hz |
| `eng4.throttle` | % | 1 Hz |
| `eng1.reverser` | 0/1 | 1 Hz |
| `eng2.reverser` | 0/1 | 1 Hz |
| `eng3.reverser` | 0/1 | 1 Hz |
| `eng4.reverser` | 0/1 | 1 Hz |
| `brake.left` | % | 1 Hz |
| `brake.right` | % | 1 Hz |
| `ap.engaged` | 0/1 | 1 Hz |
| `at.engaged` | 0/1 | 1 Hz |
| `sel.speed` | kt | 1 Hz |
| `sel.altitude` | ft | 1 Hz |
| `sel.vs` | fpm | 1 Hz |
| `sel.heading` | deg | 1 Hz |
| `stall.warning` | 0/1 | 1 Hz |
| `wind.speed` | kt | 1/4 Hz |
| `wind.dir` | deg | 1/4 Hz |
| `oat` | °C | 0.5 Hz |
| `baro.setting` | hPa | 1/64 Hz |
| `dh` | ft | 1/64 Hz |
| `weight.gross` | lb | 1/4 Hz |
| `fuel.total` | lb | 1/4 Hz |
| `sim.vs` | fpm | 8 Hz |
| `sim.td.vs` | fpm | 1 Hz |
| `sim.td.pitch` | deg | 1 Hz |
| `sim.td.bank` | deg | 1 Hz |
| `sim.rate` | ratio | 1 Hz |
| `sim.slew` | 0/1 | 1 Hz |
| `sim.paused` | 0/1 | 1 Hz |

`0/1` is a discrete (1 = on, on ground, armed, engaged…), `index` a handle position. `sim.*`
are the simulator's own values, kept outside the recorder frame
([format.md](format.md), LFL v1).

## KML

KML 2.2 (`http://www.opengis.net/kml/2.2`), opens in Google Earth. A `Document` named
`<type> · <departure> → <arrival> · <date>` with three folders:

- **Flight path**: a `LineString` per run of one phase, `altitudeMode` `absolute`, extruded to
  the ground, in the app's phase colours; each run starts at the previous run's last point so
  the line stays joined, and carries a `TimeSpan` when UTC is known. One point per second plus
  each touchdown's own point.
- **Touchdowns**: a placemark per contact (`TD1`, bounces `TD1b`…) with the landing's score,
  sink rate, peak load, the runway point (distance past the threshold, zone, centreline offset)
  and the wind on the runway.
- **Events**: a placemark per finding at its peak, in its level's colour, with its level, time,
  detail, what it checks, why it matters and what to do.

Altitude is what the altimeter showed, in metres: pressure altitude corrected for the
altimeter setting (`baro.setting`, ISA); pressure altitude when no setting was recorded. With
QNH set this is close to height above sea level; with the standard setting (above the
transition altitude) it is the flight level, so the path may sit slightly above or below the
terrain.

## GPX

GPX 1.1 (`http://www.topografix.com/GPX/1/1`), `creator="FOQA-Sim"`, for Little Navmap
(*File → Load GPX*, or a logbook entry's trail) and map viewers: `metadata` (name, the grade in
words, start time), a `wpt` per touchdown (named as in the KML, `type` `Touchdown`), then one
`trk` (`type` `Flight`) with one `trkseg` of points at 1 s with `ele` (m, as in the KML) and
`time` (UTC, ms).

## Little Navmap logbook

Little Navmap imports a logbook CSV under *Logbook → Import from CSV*; the format is its own
export's, read by `LogdataManager::importCsv` in its atools library
([logdatamanager.cpp](https://github.com/albar965/atools/blob/master/src/fs/userdata/logdatamanager.cpp),
checked 2026-10-28; [manual](https://www.littlenavmap.org/manuals/littlenavmap/release/latest/en/LOGBOOK.html)).
It reads comma-separated UTF-8 with `"` quoting (fields may span lines), skips a first line
that starts with its column names, needs at least the first 30 columns and refuses the whole
file at a line with neither a departure nor a destination ident. Times are ISO 8601
(`Qt::ISODate`), distances in NM, fuel and weights in lb, altitudes in ft.

FOQA-Sim writes all 36 columns in that order, with the header Little Navmap's export writes. A
flight with neither a departure nor an arrival airport is left out (`fdm logbook` says how
many). The mapping:

| Little Navmap | From |
|---|---|
| Aircraft Name | The sim title, only with `--identity` / the menu's opt-in |
| Aircraft Type | ICAO type designator |
| Departure Ident | Airport of the departure snapshot (first frame on the ground) |
| Destination Ident | Airport of the last touchdown's runway |
| Departure Time | Recording start, UTC |
| Destination Time | Recording start plus its duration, UTC |
| Simulator | `MSFS 2024` |
| Description | `FOQA-Sim <flight id>: <grade>, <score> of 100. <n> landing(s), <events>.` |

Left empty: Aircraft Registration (not in the flight list), flight plan and performance files
and contents, fuel and weights, distances (the list carries no track), airport names,
runways, coordinates and elevations, the sim times (the sim's date is not recorded) and the
route. Little Navmap fills the airport details from its own scenery database when it shows the
entry. The trail (a GPX) is not attached; export the flight's GPX for it.

## Landing card

A 1200 × 630 PNG (drawn at 2400 × 1260) of one landing, from the debrief's menu or a landing's
**Share** button: the route and runway, ICAO type, date, the landing's score and grade, the six
domain bars of the landing assessment ([scoring.md](scoring.md)) with the points each cost,
sink rate, peak load, touchdown distance past the threshold, centreline offset, the wind on the
runway, and the FOQA-Sim mark. It is drawn on a canvas in the app's theme (light or dark, the
dialog switches it) with its design tokens. **Copy image** puts it on the clipboard, **Save
as…** asks where (`save_image`); `lib/share-card.ts` says what it shows, tested on its own. An
unscored landing shows why instead of bars.

## In the app

`export_flight(id, format, identity)`, `export_logbook(identity)` and `save_image(name, png)`
([debrief-api.md](debrief-api.md)) each open a save dialog and return the file written, `null`
when cancelled. In mock mode (`npm run dev`, no app) the menus download files instead:
`public/mock/export/` holds `fdm`'s exports of `c172-pattern` (CSV, KML, GPX) and a logbook;
other flights and Parquet need the app.
