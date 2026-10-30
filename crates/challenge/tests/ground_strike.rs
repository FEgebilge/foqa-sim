//! A C172 landing on LTBJ 34L flown in MSFS 2024 on 2026-10-30: a fair
//! touchdown (247 fpm, 1.29 g, roll 1.9 deg), then at 45 kt on the runway
//! it rolled to -25 deg and pitched to -17 deg for about 5 s, then to +25
//! deg: a wingtip and the propeller on the runway, a crash in MSFS's own
//! challenges. The app scored it 70.5 and it counted. Cut from the
//! recording (flight-1793384681) with `fdm slice --from 150 --to 240`.

use std::path::Path;

use challenge::{CURRENT, Challenge, LANDING_1, LANDING_3, evaluate};
use debrief::{Station, load};
use rules::{StrikeLimits, strikes};

const CHALLENGE: &str = r#"{
  "id": "ltbj-34l", "version": 1, "title": "Izmir Cigli 34L",
  "airport": "LTBJ", "runway": "34L",
  "aircraft": { "types": ["C172"] },
  "start": { "distance_nm": 2, "height_ft": 700, "ias_kt": 70, "gear_down": true, "flaps": 1, "mode": "self" },
  "gate": { "distance_nm": 0.3 },
  "scoring": { "version": "landing-1" }
}"#;

fn analysis() -> debrief::Analysis<'static> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/c172-ltbj-34l-ground-strike.fqz");
    let station = Box::leak(Box::new(Station::core().unwrap()));
    station
        .analyse("ltbj-34l-strike".into(), load(&path).unwrap())
        .unwrap()
}

#[test]
fn the_wing_and_prop_strike_is_found_once_on_the_rollout() {
    let a = analysis();
    let found = strikes(&a.track, StrikeLimits::of(a.selection.class));
    assert_eq!(found.len(), 1, "{found:?}");
    let s = found[0];
    assert!((201.0..=210.0).contains(&s.t_s), "{s:?}");
    assert!(s.wing, "{s:?}");
    let rules: Vec<&str> = a.events.iter().map(|e| e.rule.as_str()).collect();
    assert!(rules.contains(&"ground-strike"), "{rules:?}");
}

#[test]
fn the_attempt_does_not_count() {
    let a = analysis();
    for scoring in [LANDING_1, LANDING_3] {
        let json = CHALLENGE.replace("landing-1", scoring);
        let attempt = evaluate(&Challenge::parse(&json).unwrap(), &a);
        let strike = attempt.checks.iter().find(|c| c.name == "ground_strike");
        let strike = strike.unwrap_or_else(|| panic!("{scoring}: {:?}", attempt.checks));
        assert!(!strike.ok, "{scoring}: {strike:?}");
        assert!(strike.detail.contains("wing"), "{}", strike.detail);
        assert!(!attempt.valid, "{scoring}");
        let score = attempt.score.unwrap();
        assert_eq!(
            (score.version, score.total),
            (CURRENT, 0.0),
            "{scoring}: held at 0"
        );
    }
}

#[test]
fn the_assessment_is_capped_by_the_strike() {
    let a = analysis();
    let detail = a.assessment(&a.events());
    let json = serde_json::to_value(&detail).unwrap();
    let caps = json.to_string();
    assert!(
        caps.contains("struck the ground with a wing or the nose"),
        "{caps}"
    );
}
