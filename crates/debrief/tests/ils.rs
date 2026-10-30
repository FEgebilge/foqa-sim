//! A C172 visual approach to LTBJ 34L flown in MSFS 2024 on 2026-10-30,
//! landed 3 m off the centreline with NAV1 on another runway's localizer:
//! its deviation sat at full scale (2 dots) all the way down. Cut from the
//! recording with `fdm slice --from 95 --to 275`.

use std::path::Path;

use debrief::{Station, load};

#[test]
fn another_runways_localizer_is_no_deviation() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/c172-ltbj-34l-visual.fqz");
    let flight = load(&path).unwrap();
    // Recorded before the localizer course was: which runway NAV1 served
    // is unknown, so its deviations are too. (Its needle, full scale at the
    // touchdown, would leave them out with a course as well.)
    assert!(flight.series("nav.loc_course").is_empty());
    assert!(
        (flight.series("nav.loc_dev").iter()).any(|s| s.is_valid() && s.value.abs() > 1.9),
        "the recording holds the full-scale deviation"
    );
    let station = Station::core().unwrap();
    let analysis = station.analyse("ltbj-34l".into(), flight).unwrap();
    let rules: Vec<&str> = analysis.events.iter().map(|e| e.rule.as_str()).collect();
    assert_eq!(rules, ["unstable-speed-light"]);
}
