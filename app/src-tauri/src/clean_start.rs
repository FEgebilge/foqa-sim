//! Clean starts: the flight the sim saves right after a challenge's first
//! placement, one per challenge, so that Fly again reloads a pristine
//! aircraft (docs/challenges.md, "Fly again"). The sim reports neither
//! damage nor a crash, so every Fly again reloads it. A clean start is kept
//! with the ICAO type it was saved with and only reloaded into the same
//! type: a reload brings back the saved aircraft, not the loaded one.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use challenge::{Challenge, Clock};
use serde::{Deserialize, Serialize};

use crate::library;
use crate::weather_file::wpr as wpr_text;

/// The sim's own extension for a saved flight.
const FLT: &str = "FLT";
/// A weather file's, beside the flight that names it.
const WPR: &str = "WPR";
/// No AI air traffic in a reloaded clean start, as the sim's own events and
/// mission templates turn it off (`asobo-liveevent` LiveEvent.FLT): a
/// challenge is flown alone, without another aircraft's wake on final.
/// Not their `[TrafficManager] disable=True` too: with it MSFS 2024 never
/// ends its move to the runway after a load (no `RTC_END`; bench
/// 2026-10-31), so the start was placed before the sim moved the aircraft
/// back onto the runway.
const NO_TRAFFIC: &[(&str, &str)] = &[("[AirTraffic]", "Status=0")];

/// `<key>.json` beside the flight: what it was saved with.
#[derive(Debug, Serialize, Deserialize)]
struct Saved {
    icao: String,
}

/// The clean starts folder.
pub struct CleanStarts {
    dir: PathBuf,
}

impl CleanStarts {
    pub fn new(dir: PathBuf) -> io::Result<Self> {
        fs::create_dir_all(&dir)?;
        Ok(Self { dir })
    }

    /// Where the sim saves library entry `key`'s clean start: no extension,
    /// the sim adds `.FLT`. A new one replaces it.
    pub fn path(&self, key: &str) -> Result<PathBuf, String> {
        library::checked_key(key)?;
        Ok(self.dir.join(key))
    }

    /// Forgets the clean start at `path` before another is saved there: a
    /// save the sim then rejects leaves none, not one of another type.
    pub fn forget(&self, path: &Path) {
        let files = [FLT, WPR, "json"].map(|ext| path.with_extension(ext));
        for file in files {
            match fs::remove_file(&file) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => {
                    eprintln!("{}: could not delete: {e}", file.display())
                }
                _ => {}
            }
        }
    }

    /// Waits, up to `within`, until the sim has written the flight saved at
    /// `path`: it answers the save before the file is on disk (2026-10-30),
    /// so the file is taken once it exists and its size holds still.
    pub fn written(&self, path: &Path, within: Duration) -> Result<(), String> {
        let flt = path.with_extension(FLT);
        let until = Instant::now() + within;
        let mut last = None;
        while Instant::now() < until {
            let size = fs::metadata(&flt).ok().map(|m| m.len());
            if size.is_some_and(|s| s > 0) && size == last {
                return Ok(());
            }
            last = size;
            thread::sleep(Duration::from_millis(200));
        }
        Err("the sim did not write the saved flight".into())
    }

    /// The flight at `path` was saved for an `icao` aircraft at `challenge`'s
    /// start: it departs from the challenge's runway, under its sky and clock
    /// if it sets them, its traffic is turned off and its type kept beside it.
    pub fn saved(&self, path: &Path, icao: &str, challenge: &Challenge) -> Result<(), String> {
        let flt = path.with_extension(FLT);
        match fs::read(&flt) {
            Ok(bytes) => {
                let mut bytes = departing_from(
                    &without_traffic(&bytes),
                    &challenge.airport,
                    &challenge.runway,
                );
                if let Some(clock) = &challenge.conditions.time {
                    bytes = at_time(&bytes, clock);
                }
                if let Some(sky) = challenge.conditions.weather {
                    let wpr = path.with_extension(WPR);
                    fs::write(&wpr, wpr_text(sky, challenge.conditions.wind.as_ref()))
                        .map_err(|e| e.to_string())?;
                    bytes = with_weather_file(&bytes, &wpr);
                }
                fs::write(&flt, bytes).map_err(|e| e.to_string())?
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
        let saved = Saved { icao: icao.into() };
        let json = serde_json::to_vec(&saved).map_err(|e| e.to_string())?;
        fs::write(path.with_extension("json"), json).map_err(|e| e.to_string())
    }

    /// The clean start of `key` to reload into the `icao` aircraft loaded
    /// now, or in plain words why there is none.
    pub fn find(&self, key: &str, icao: Option<&str>) -> Result<PathBuf, String> {
        let path = self.path(key)?;
        let saved = fs::read(path.with_extension("json"))
            .ok()
            .and_then(|b| serde_json::from_slice::<Saved>(&b).ok());
        let Some(saved) = saved.filter(|_| path.with_extension(FLT).is_file()) else {
            return Err("none saved yet".into());
        };
        match icao {
            Some(icao) if icao == saved.icao => Ok(path),
            Some(icao) => Err(format!(
                "the one saved is for a {}, a {icao} is loaded",
                saved.icao
            )),
            None => Err("the loaded aircraft's type is not known yet".into()),
        }
    }
}

/// `flt` with `NO_TRAFFIC`'s sections appended, those it lacks; bytes, not
/// text: the sim writes Latin-1 names.
fn without_traffic(flt: &[u8]) -> Vec<u8> {
    let has = |section: &str| flt.windows(section.len()).any(|w| w == section.as_bytes());
    let mut out = flt.to_vec();
    for (section, line) in NO_TRAFFIC.iter().filter(|(s, _)| !has(s)) {
        out.extend_from_slice(format!("\n{section}\n{line}\n").as_bytes());
    }
    out
}

/// `flt` with its `[Departure]` at `airport`'s `runway` (`06`, `27L`). The
/// sim saves the runway the flight began on and after loading the flight
/// moves the aircraft there (bench, 2026-11-02): a start at another airport
/// would go across the world and back on every reload.
fn departing_from(flt: &[u8], airport: &str, runway: &str) -> Vec<u8> {
    let digits = runway.trim_end_matches(|c: char| !c.is_ascii_digit());
    let number = digits.trim_start_matches('0');
    let designator = match &runway[digits.len()..] {
        "" => "NONE",
        side => side,
    };
    with_keys(
        flt,
        "[Departure]",
        &[
            ("ICAO", airport),
            ("RunwayNumber", number),
            ("RunwayDesignator", designator),
        ],
    )
}

/// `flt` starting at `clock`, local at the place it is saved (bench,
/// 2026-11-02: `Hours=12` at UTC+10 gave LOCAL TIME 12:00, ZULU TIME 02:00).
fn at_time(flt: &[u8], clock: &Clock) -> Vec<u8> {
    let (year, day) = clock.day_of_year().unwrap_or_default();
    let (hours, minutes) = clock.hour_minute().unwrap_or_default();
    with_keys(
        flt,
        "[DateTimeSeason]",
        &[
            ("Year", &year.to_string()),
            ("Day", &day.to_string()),
            ("Hours", &hours.to_string()),
            ("Minutes", &minutes.to_string()),
            ("Seconds", "0"),
            ("UseZuluTime", "False"),
        ],
    )
}

/// `flt` under the weather in the file at `wpr`, not the live one (bench,
/// 2026-11-02: the wind read back from the sim was the file's).
fn with_weather_file(flt: &[u8], wpr: &Path) -> Vec<u8> {
    with_keys(
        flt,
        "[Weather]",
        &[
            ("UseWeatherFile", "True"),
            ("UseLiveWeather", "False"),
            ("WeatherPresetFile", &wpr.to_string_lossy()),
            ("WeatherCanBeLive", "False"),
        ],
    )
}

/// `flt` with `keys` set in `section`, added with it if it has none; other
/// lines stay as the sim wrote them, in its line ends.
fn with_keys(flt: &[u8], section: &str, keys: &[(&str, &str)]) -> Vec<u8> {
    let eol = if flt.windows(2).any(|w| w == b"\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let lines: String = keys.iter().map(|(k, v)| format!("{k}={v}{eol}")).collect();
    let is_key = |line: &[u8]| {
        keys.iter().any(|(k, _)| {
            line.strip_prefix(k.as_bytes())
                .is_some_and(|rest| rest.starts_with(b"="))
        })
    };
    let mut out = Vec::with_capacity(flt.len() + lines.len());
    let (mut inside, mut found) = (false, false);
    for line in flt.split_inclusive(|&b| b == b'\n') {
        let text = line.trim_ascii();
        if text.starts_with(b"[") {
            inside = text == section.as_bytes();
            out.extend_from_slice(line);
            if inside {
                out.extend_from_slice(lines.as_bytes());
                found = true;
            }
        } else if !inside || !is_key(text) {
            out.extend_from_slice(line);
        }
    }
    if !found {
        out.extend_from_slice(format!("{eol}{section}{eol}{lines}").as_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_save_counts_once_the_file_is_on_disk() {
        let tmp = tempfile::tempdir().unwrap();
        let starts = CleanStarts::new(tmp.path().to_path_buf()).unwrap();
        let path = starts.path("ltbj-34l-v1").unwrap();
        assert!(starts.written(&path, Duration::from_millis(300)).is_err());
        fs::write(path.with_extension(FLT), "[Main]").unwrap();
        assert!(starts.written(&path, Duration::from_secs(2)).is_ok());
    }

    #[test]
    fn a_clean_start_is_saved_without_traffic_once() {
        let once = without_traffic(b"[Main]\nTitle=x\xe9\n");
        let text = String::from_utf8_lossy(&once);
        assert!(text.ends_with("\n[AirTraffic]\nStatus=0\n"));
        assert!(!text.contains("[TrafficManager]"));
        assert_eq!(without_traffic(&once), once);
    }

    #[test]
    fn a_clean_start_departs_from_its_challenges_runway() {
        let flt = b"[Main]\nTitle=x\xe9\n\n[Departure]\nICAO=LELL\nRunwayNumber=31\nRunwayDesignator=NONE\n\n[Loading]\nTips0=a\n";
        assert_eq!(
            departing_from(flt, "LTFJ", "06L"),
            b"[Main]\nTitle=x\xe9\n\n[Departure]\nICAO=LTFJ\nRunwayNumber=6\nRunwayDesignator=L\n\n[Loading]\nTips0=a\n"
        );
        let plain = departing_from(flt, "YMAA", "15");
        let text = String::from_utf8_lossy(&plain);
        assert!(text.contains("ICAO=YMAA\nRunwayNumber=15\nRunwayDesignator=NONE\n"));
        assert!(!text.contains("LELL"));
        // As the sim writes it: its line ends. No such section: one, once.
        let crlf = departing_from(
            b"[Departure]\r\nICAO=LELL\r\n\r\n[Loading]\r\n",
            "YMAA",
            "15",
        );
        let crlf = String::from_utf8_lossy(&crlf);
        assert!(
            crlf.contains("ICAO=YMAA\r\nRunwayNumber=15\r\nRunwayDesignator=NONE\r\n\r\n[Loading]")
        );
        let added = departing_from(b"[Main]\n", "YMAA", "15");
        let count = String::from_utf8_lossy(&added)
            .matches("[Departure]")
            .count();
        assert_eq!(count, 1);
        assert_eq!(departing_from(&added, "YMAA", "15"), added);
    }

    fn challenge(conditions: &str) -> Challenge {
        Challenge::parse(&format!(
            r#"{{"id":"a","version":1,"title":"a","airport":"LTBJ","runway":"34L",
                "start":{{"distance_nm":8,"height_ft":2500,"ias_kt":160,"mode":"teleport"}},
                "gate":{{"distance_nm":3}},"conditions":{conditions},
                "scoring":{{"version":"landing-1"}}}}"#
        ))
        .unwrap()
    }

    #[test]
    fn a_clean_start_keeps_the_challenges_sky_and_clock() {
        let tmp = tempfile::tempdir().unwrap();
        let starts = CleanStarts::new(tmp.path().to_path_buf()).unwrap();
        let path = starts.path("a-v1").unwrap();
        let flt = path.with_extension(FLT);
        let saved = b"[Weather]\r\nUseWeatherFile=False\r\nUseLiveWeather=True\r\nWeatherPresetFile=.\\WeatherPresets\\storm.WPR\r\nFixedClouds=False\r\n\r\n[DateTimeSeason]\r\nSeason=Fall\r\nYear=2026\r\nDay=306\r\nHours=7\r\nMinutes=38\r\nSeconds=58\r\nUseZuluTime=False\r\n\r\n[Departure]\r\nICAO=LELL\r\n";
        fs::write(&flt, saved).unwrap();
        let c = challenge(
            r#"{"weather":"rain","time":{"date":"2026-07-15","local":"18:30"},
                "wind":{"from_deg":40,"speed_kt":12,"tolerance_deg":20,"tolerance_kt":4}}"#,
        );
        starts.saved(&path, "C172", &c).unwrap();
        let text = String::from_utf8(fs::read(&flt).unwrap()).unwrap();
        let wpr = path.with_extension(WPR);
        for want in [
            "UseWeatherFile=True\r\n".to_owned(),
            "UseLiveWeather=False\r\n".to_owned(),
            format!("WeatherPresetFile={}\r\n", wpr.display()),
            "FixedClouds=False\r\n".to_owned(),
            "Season=Fall\r\n".to_owned(),
            "Year=2026\r\nDay=196\r\nHours=18\r\nMinutes=30\r\nSeconds=0\r\n".to_owned(),
            "ICAO=LTBJ\r\nRunwayNumber=34\r\nRunwayDesignator=L\r\n".to_owned(),
        ] {
            assert!(text.contains(&want), "{want} in {text}");
        }
        assert_eq!(text.matches("UseZuluTime=").count(), 1);
        assert!(
            fs::read_to_string(&wpr)
                .unwrap()
                .contains("Value=\"40.000\"")
        );
        // Forgotten with its flight.
        starts.forget(&path);
        assert!(!wpr.exists());

        // A challenge that sets neither leaves them as the pilot's.
        fs::write(&flt, saved).unwrap();
        starts.saved(&path, "C172", &challenge("{}")).unwrap();
        let text = String::from_utf8(fs::read(&flt).unwrap()).unwrap();
        assert!(text.contains("UseLiveWeather=True\r\n") && text.contains("Hours=7\r\n"));
        assert!(!wpr.exists());
    }

    #[test]
    fn a_clean_start_is_reloaded_only_into_its_own_type() {
        let tmp = tempfile::tempdir().unwrap();
        let starts = CleanStarts::new(tmp.path().join("challenge-snapshots")).unwrap();
        let key = "ltbj-34l-v1";
        assert_eq!(starts.find(key, Some("C172")), Err("none saved yet".into()));
        let path = starts.path(key).unwrap();
        starts.saved(&path, "C172", &challenge("{}")).unwrap();
        // The meta without the sim's file: the save never happened.
        assert!(starts.find(key, Some("C172")).is_err());
        fs::write(path.with_extension(FLT), "[Main]").unwrap();
        assert_eq!(starts.find(key, Some("C172")), Ok(path.clone()));
        let other = starts.find(key, Some("B738")).unwrap_err();
        assert_eq!(other, "the one saved is for a C172, a B738 is loaded");
        assert!(starts.find(key, None).is_err());
        // Forgotten before another save, whatever its type.
        starts.forget(&path);
        assert_eq!(starts.find(key, Some("C172")), Err("none saved yet".into()));
        assert!(starts.path("../x").is_err());
    }
}
