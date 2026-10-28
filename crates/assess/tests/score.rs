//! The landing-2 score from given measures: the worked examples of
//! docs/scoring.md §6 as golden tests, the caps, the Data4Safety unstable
//! rule, n/a redistribution and the flight grade.

use assess::*;
use rules::{Domain, Level, RefSpeeds, ScoringClass};

const FT_PER_M: f64 = 1.0 / 0.3048;

fn method() -> Method {
    Method::landing_2().unwrap_or_else(|e| panic!("{e}"))
}

fn scalar(id: &str, height: Height, x: f64) -> (String, Value) {
    (id.to_owned(), Value::Scalar(height, Some(x)))
}

/// A landing with only `values`, on a runway described by `ctx`.
fn measures(class: ScoringClass, context: Context, values: Vec<(String, Value)>) -> Measures {
    Measures {
        class,
        context,
        heights_ft: Vec::new(),
        values,
        findings: Vec::new(),
        outcome: Outcome::default(),
        partial: Vec::new(),
    }
}

fn runway(target_m: f64, tdz_m: f64, lda_m: f64, width_m: f64, track_ft: f64) -> Context {
    Context {
        add_kt: Some(5.0),
        gust_kt: Some(0.0),
        rod3_fpm: Some(5.31 * 62.0),
        target_ft: Some(target_m * FT_PER_M),
        tdz_ft: Some(tdz_m * FT_PER_M),
        lda_ft: Some(lda_m * FT_PER_M),
        width_ft: Some(width_m * FT_PER_M),
        track_ft: Some(track_ft),
        hard_g: None,
    }
}

fn domain(s: &LandingScore, d: Domain) -> &DomainScore {
    s.domains.iter().find(|x| x.domain == d).unwrap()
}

fn near(x: f64, want: f64, tol: f64) -> bool {
    (x - want).abs() <= tol
}

/// Worked example 1: a C172 on a 1600 m runway, a challenge target at the
/// 300 m aiming point, TDZ 533 m, light main-gear track 2.5 m.
fn example_1() -> Measures {
    use Height::*;
    measures(
        ScoringClass::Light,
        runway(300.0, 533.0, 1600.0, 30.0, 2.5 * FT_PER_M),
        vec![
            scalar("A1", Below, 12.0),
            scalar("A2", Below, 820.0),
            scalar("B1", All, 62.0 - 44.0),
            scalar("C1", All, (470.0 - 300.0) * FT_PER_M),
            scalar("D1", All, 2.4 * FT_PER_M),
            scalar("D2", All, 3.0),
            scalar("E1", All, 1.52),
            scalar("E2", All, 260.0),
            scalar("F2", All, 0.04),
        ],
    )
}

#[test]
fn worked_example_1_c172_normal_landing() {
    let m = method();
    let s = m.score(&example_1());
    let lost = |d| {
        let d = domain(&s, d);
        (d.weight * d.loss.unwrap() * 100.0).round() / 100.0
    };
    // A1 +12 kt: ℓ 0.70, loss 0.20 of 25.
    assert_eq!(lost(Domain::Stability), 5.0);
    assert_eq!(
        domain(&s, Domain::Stability).set_by.as_deref(),
        Some("A1 below")
    );
    // B1 62 ft against the 44 ft wheel path to a 300 m target: +18 ft, ℓ 0.65.
    assert_eq!(lost(Domain::Energy), 1.75);
    // C1 558 ft past the target: L1 (target + 800 ft = 544 m) is held at
    // the TDZ end (533 m), so ℓ 0.72, loss 0.21 of 20.
    let c1 = &domain(&s, Domain::Point).criteria[0];
    assert!(near(c1.ell, 0.716, 0.001), "{c1:?}");
    assert_eq!(lost(Domain::Point), 4.16);
    assert_eq!(lost(Domain::Lateral), 2.18);
    assert_eq!(lost(Domain::Contact), 1.47);
    assert_eq!(lost(Domain::Rollout), 0.0);
    assert_eq!((s.score, s.grade), (85.4, Grade::Good));
    assert!(s.caps.is_empty());
    assert_eq!(s.confidence, Confidence::Full);
}

/// Worked example 2: a B737-800, Vref 141 kt and Vapp 146 kt (additive 5),
/// LDA 3000 m, aiming point 400 m, TDZ 914 m, jet track 6 m.
fn example_2(sink_below_500: f64) -> Measures {
    use Height::*;
    let mut ctx = runway(400.0, 914.0, 3000.0, 45.0, 6.0 * FT_PER_M);
    ctx.hard_g = Some(2.2);
    measures(
        ScoringClass::Jet,
        ctx,
        vec![
            scalar("A2", Below, sink_below_500),
            scalar("A1", Above, 22.0),
            scalar("B3", All, 9.0),
            scalar("C1", All, (650.0 - 400.0) * FT_PER_M),
            scalar("D1", All, 4.1 * FT_PER_M),
            scalar("E1", All, 1.48),
            scalar("F1", All, 2.5),
        ],
    )
}

#[test]
fn worked_example_2_b738_late_descent() {
    let m = method();
    let s = m.score(&example_2(1100.0));
    let a = domain(&s, Domain::Stability);
    // 1100 fpm below 500 ft: tol 1000 (0.5) to L2 1200 (2), ℓ 1.25.
    let a2 = a.criteria.iter().find(|c| c.id == "A2").unwrap();
    assert_eq!((a2.ell, a2.level), (1.25, 1));
    let a1 = a.criteria.iter().find(|c| c.id == "A1").unwrap();
    assert!(near(a1.ell, 1.133, 0.001), "{a1:?}");
    assert!(near(a.loss.unwrap() * 25.0, 10.625, 1e-9));
    assert_eq!((s.score, s.grade), (77.9, Grade::Fair));
    assert!(s.caps.is_empty(), "{:?}", s.caps);

    // 1250 fpm below 500 ft is SL2B: unstable and continued, Poor.
    let s = m.score(&example_2(1250.0));
    let kinds: Vec<_> = s.caps.iter().map(|c| c.kind).collect();
    assert_eq!(kinds, [CapKind::Material, CapKind::Unstable]);
    assert_eq!((s.score, s.grade), (59.0, Grade::Poor));
}

#[test]
fn the_loss_curve_follows_its_table() {
    let m = method();
    let losses = ANCHOR_ELL.map(|ell| m.loss(ell));
    assert_eq!(losses, [0.0, 0.10, 0.35, 0.65, 1.0]);
    assert!(near(m.loss(1.5), 0.5, 1e-12));
    assert_eq!(m.loss(4.0), 1.0);
}

/// Data4Safety: three criteria at the first threshold above 500 ft, or one
/// at the first threshold below it (SL2B), is an unstable approach;
/// two above it are not.
#[test]
fn the_unstable_rule_counts_like_data4safety() {
    let m = method();
    let jet = |values| {
        m.score(&measures(
            ScoringClass::Jet,
            runway(400.0, 914.0, 3000.0, 45.0, 19.7),
            values,
        ))
    };
    let unstable = |s: &LandingScore| s.caps.iter().any(|c| c.kind == CapKind::Unstable);
    use Height::*;
    let two = vec![scalar("A1", Above, 22.0), scalar("A2", Above, 1250.0)];
    assert!(!unstable(&jet(two.clone())));
    let mut three = two;
    three.push(scalar("A3", Above, 16.0));
    let s = jet(three);
    assert!(unstable(&s));
    assert!(s.score <= 59.0);
    assert!(unstable(&jet(vec![scalar("A3", Below, 16.0)])));
    // Glideslope and localizer are one criterion (A5): two in all.
    let ils = vec![
        scalar("A1", Above, 22.0),
        scalar("A5G", Above, 1.2),
        scalar("A5L", Above, 1.2),
    ];
    assert!(!unstable(&jet(ils)));
    // Late flap below 500 ft (A6 at L3) is SL3 on its own.
    assert!(unstable(&jet(vec![scalar("A6", All, 3.0)])));
}

#[test]
fn each_safety_cap_names_its_cause() {
    let m = method();
    let base = || example_2(900.0);
    let caps = |x: Measures| {
        let s = m.score(&x);
        (
            s.caps.iter().map(|c| (c.kind, c.score)).collect::<Vec<_>>(),
            s.score,
        )
    };
    assert_eq!(caps(base()), (vec![], caps(base()).1));
    let finding = |rule: &str, domain, level| Finding {
        rule: rule.into(),
        title: format!("Title of {rule}"),
        domain,
        level,
        height_ft: None,
        event: None,
    };
    let mut x = base();
    x.findings
        .push(finding("bounced-landing", Domain::Contact, Level::Medium));
    assert_eq!(caps(x).0, [(CapKind::Material, 84.9)]);
    let mut x = base();
    x.findings.push(finding(
        "hard-landing-inspection",
        Domain::Contact,
        Level::High,
    ));
    assert_eq!(
        caps(x).0,
        [(CapKind::Material, 84.9), (CapKind::Inspection, 39.0)]
    );
    // A tail strike: no tail clearance left at touchdown.
    let mut x = base();
    x.values.push(scalar("E4", Height::All, -0.5));
    assert!(caps(x).0.contains(&(CapKind::Inspection, 39.0)));
    let mut x = base();
    x.outcome.undershoot = true;
    assert_eq!(caps(x), (vec![(CapKind::Undershoot, 39.0)], 39.0));
    let mut x = base();
    x.outcome.excursion = Some("Left the side of the runway".into());
    let s = m.score(&x);
    assert_eq!((s.score, s.grade), (0.0, Grade::Poor));
    assert_eq!(
        s.caps[0].reason,
        "Left the side of the runway, which holds this landing at zero."
    );
    // A stability finding at medium below 500 ft is SL2B: unstable.
    let mut x = base();
    x.findings.push(Finding {
        height_ft: Some(300.0),
        ..finding("high-sink-below-500", Domain::Stability, Level::Medium)
    });
    assert!(caps(x).0.contains(&(CapKind::Unstable, 59.0)));
}

/// Without any value or finding a domain is n/a and its weight goes to the
/// others: one L1 in stability alone loses 35% of everything.
#[test]
fn n_a_domains_redistribute_their_weight() {
    let m = method();
    let only_a = measures(
        ScoringClass::Jet,
        runway(400.0, 914.0, 3000.0, 45.0, 19.7),
        vec![scalar("A3", Height::Above, 15.0)],
    );
    let s = m.score(&only_a);
    assert_eq!(s.domains.iter().filter(|d| d.loss.is_some()).count(), 1);
    assert_eq!(s.raw, 65.0);
    // An advisory finding makes its domain available at no loss.
    let mut with_e = only_a;
    with_e.findings.push(Finding {
        rule: "x".into(),
        title: "X".into(),
        domain: Domain::Contact,
        level: Level::Advisory,
        height_ft: None,
        event: None,
    });
    // 100 - 25 x 0.35 x 100 / 45.
    assert_eq!(m.score(&with_e).raw, 80.6);
    // Missing expected data keeps the score but marks it partial.
    let mut partial = example_1();
    partial.partial.push("No Vref for C172".into());
    assert_eq!(m.score(&partial).confidence, Confidence::Partial);
}

#[test]
fn a_series_is_graded_by_its_sustained_worst_and_gives_the_stable_height() {
    let m = method();
    // Five points from 1000 ft down; the sink held 1300 fpm at 800 ft,
    // within tolerance from 600 ft.
    let heights = vec![1000.0, 800.0, 600.0, 400.0, 200.0];
    let sink = vec![900.0, 1300.0, 900.0, 800.0, 700.0];
    let mut x = measures(
        ScoringClass::Jet,
        runway(400.0, 914.0, 3000.0, 45.0, 19.7),
        vec![],
    );
    x.context.rod3_fpm = Some(700.0);
    x.heights_ft = heights;
    x.values.push((
        "A2".into(),
        Value::Series {
            high: sink.clone(),
            low: sink,
        },
    ));
    let s = m.score(&x);
    let a2: Vec<_> = domain(&s, Domain::Stability)
        .criteria
        .iter()
        .map(|c| (c.height, c.value))
        .collect();
    assert_eq!(a2, [(Height::Above, 1300.0), (Height::Below, 800.0)]);
    assert_eq!(s.stable_from_ft, Some(600.0));
    // Where it left the band and where it was worst, as window points: the
    // lower half never left it.
    let at: Vec<_> = domain(&s, Domain::Stability)
        .criteria
        .iter()
        .map(|c| (c.level, c.first_out, c.worst))
        .collect();
    assert_eq!(at[0].1, Some(1));
    assert!(at[0].0 >= 1);
    assert_eq!(at[1], (0, None, Some(3)));
}

#[test]
fn the_last_full_stop_landing_sets_the_flight_grade() {
    let m = method();
    let good = m.score(&example_1());
    let mut short = example_1();
    short.outcome.undershoot = true;
    let short = m.score(&short);
    // A touch-and-go after the full stop does not set the grade.
    let g = m
        .flight_grade(&[(&good, true), (&short, false)], 0)
        .unwrap();
    assert_eq!((g.score, g.grade), (39.0, Grade::Poor));
    assert_eq!(
        g.reasons,
        [
            "Landing 2: Touched down before the runway threshold, which holds the flight below Fair (max 39.0)."
        ]
    );
    let g = m.flight_grade(&[(&good, false), (&good, true)], 0).unwrap();
    assert_eq!((g.score, g.grade, g.reasons.len()), (85.4, Grade::Good, 0));
    let g = m.flight_grade(&[(&good, true)], 1).unwrap();
    assert_eq!((g.score, g.grade), (84.9, Grade::Fair));
    let g = m.flight_grade(&[(&good, true)], 2).unwrap();
    assert_eq!(g.grade, Grade::Poor);
    assert_eq!(m.flight_grade(&[], 0), None);
}

/// Decision 1: the profile's Vref with the wind additive; the selected
/// speed only outside a ranked attempt; none at all otherwise.
#[test]
fn reference_speeds_follow_the_vref_sources() {
    let m = method();
    let table = || {
        Some(RefSpeeds {
            vref: 141.0,
            vapp: None,
            approximate: false,
            source: "FCOM".into(),
        })
    };
    let vapp = |headwind, at| {
        m.speeds(table(), None, Some(headwind), at, true)
            .unwrap()
            .vapp_kt
    };
    assert_eq!(vapp(0.0, false), 146.0);
    assert_eq!(vapp(20.0, false), 151.0);
    assert_eq!(vapp(40.0, false), 156.0);
    assert_eq!(vapp(-10.0, false), 146.0);
    assert_eq!(vapp(40.0, true), 146.0);
    let light = RefSpeeds {
        vref: 60.0,
        vapp: Some(65.0),
        approximate: false,
        source: "POH".into(),
    };
    let s = m
        .speeds(Some(light), Some(80.0), None, false, true)
        .unwrap();
    assert_eq!(
        (s.vref_kt, s.vapp_kt, s.source),
        (60.0, 65.0, SpeedSource::Profile)
    );
    let s = m.speeds(None, Some(150.0), None, false, false).unwrap();
    assert_eq!(
        (s.vref_kt, s.vapp_kt, s.source),
        (145.0, 150.0, SpeedSource::Selected)
    );
    assert_eq!(m.speeds(None, Some(150.0), None, false, true), None);
}

#[test]
fn a_method_is_checked_when_loaded() {
    let rules = rules::load(include_str!("../../../rules/core.yaml")).unwrap();
    let yaml = include_str!("../../../scoring/landing-2.yaml");
    let err = |from: &str, to: &str| {
        Method::load(&yaml.replace(from, to), &rules)
            .unwrap_err()
            .to_string()
    };
    assert!(
        err("levels_from: hard-landing,", "levels_from: firm-landing,")
            .contains("no rule `firm-landing`")
    );
    assert!(
        err(
            "levels_from: hard-landing,",
            "levels_from: high-sink-touchdown,"
        )
        .contains("has no thresholds in g")
    );
    let e = err("[1.3 g, 1.5 g,", "[1.3 kt, 1.5 g,");
    assert!(
        e.contains("criterion `E1`") && e.contains("is kt, but the criterion is g"),
        "{e}"
    );
    assert!(err("rollout: 10 }", "rollout: 0 }").contains("positive weight"));
    assert!(err("id: F2", "id: F1").contains("duplicate criterion `F1`"));
}
