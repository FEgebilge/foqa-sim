//! What every shipped type profile must hold, so a wrong number fails the
//! build instead of grading landings: the 52 kt C172 reference (2026-11-02)
//! looked like any other result. The gaps known today are listed by type and
//! may only shrink.

use rules::{Profile, ProfileSet};

/// No Vref table, approach speeds or stalling speed: speed items not graded.
const NO_SPEED_SOURCE: [&str; 6] = ["A310", "AT76", "BE36", "PC12", "PC24", "SF34"];
/// The same speeds as another type's table. Each is right at its own MLW,
/// which is all that was published (Boeing and Airbus airport planning data:
/// 153 and 136 kt); the rows below it are scaled by the root of the weight.
const COPIED_VREF: [(&str, &str); 2] = [("A21N", "A320"), ("B748", "B78X")];

/// A speed this close to the stall is no approach (the usual reference is
/// 1.3 VSO).
const MIN_APPROACH_OVER_VSO: f64 = 1.2;
/// The light touchdown band's ideal edge below the approach speed
/// (`scoring/landing-3.yaml` B2): it must stay above the stall.
const TOUCHDOWN_IDEAL_BELOW_KT: f64 = 10.0;

fn profiles() -> ProfileSet {
    ProfileSet::shipped().unwrap()
}

/// The speeds a type's landings are graded against, per flap row.
fn approach_lows(p: &Profile) -> Vec<f64> {
    // At the heaviest weight: VSO is the one at maximum weight.
    let table = p
        .vref
        .iter()
        .flat_map(|t| &t.flaps)
        .map(|c| c.kt[c.kt.len() - 1]);
    let published = p.approach_speed.iter().flat_map(|a| &a.flaps);
    table.chain(published.map(|(_, [low, _])| *low)).collect()
}

#[test]
fn every_type_has_a_speed_source_but_the_known_gaps() {
    let lacking: Vec<String> = profiles()
        .iter()
        .filter(|p| p.vref.is_none() && p.approach_speed.is_none() && p.vso.is_none())
        .map(|p| p.icao.clone())
        .collect();
    assert_eq!(lacking, NO_SPEED_SOURCE);
}

#[test]
fn approach_speeds_stay_above_the_stall() {
    for p in profiles().iter() {
        let Some(vso) = &p.vso else { continue };
        for low in approach_lows(p) {
            assert!(
                low >= MIN_APPROACH_OVER_VSO * vso.value,
                "{}: {low} kt against VSO {}",
                p.icao,
                vso.value
            );
        }
    }
}

#[test]
fn the_ideal_touchdown_of_a_light_type_stays_above_the_stall() {
    for p in profiles().iter() {
        let (Some(vso), Some(approach)) = (&p.vso, &p.approach_speed) else {
            continue;
        };
        for (flap, [low, high]) in &approach.flaps {
            let ideal = (low + high) / 2.0 - TOUCHDOWN_IDEAL_BELOW_KT;
            assert!(
                ideal > vso.value,
                "{} at flap {flap}: ideal up to {ideal} kt against VSO {}",
                p.icao,
                vso.value
            );
        }
    }
}

#[test]
fn published_ranges_read_low_to_high_and_fit_the_flap_limit() {
    for p in profiles().iter() {
        let Some(approach) = &p.approach_speed else {
            continue;
        };
        for (flap, [low, high]) in &approach.flaps {
            assert!(low <= high, "{}", p.icao);
            let band = p
                .flaps
                .iter()
                .find(|b| b.up_to.is_none_or(|up_to| *flap <= up_to));
            if let Some(band) = band {
                assert!(
                    *low <= band.vfe.value,
                    "{} at flap {flap}: approach from {low} kt, VFE {}",
                    p.icao,
                    band.vfe.value
                );
            }
        }
    }
}

#[test]
fn vref_rises_with_weight() {
    for p in profiles().iter() {
        for curve in p.vref.iter().flat_map(|t| &t.flaps) {
            assert!(
                curve.kt.windows(2).all(|w| w[0] <= w[1]),
                "{} at flap {}: {:?}",
                p.icao,
                curve.flap,
                curve.kt
            );
            assert!(
                curve.weights_lb.windows(2).all(|w| w[0] < w[1]),
                "{}",
                p.icao
            );
        }
    }
}

#[test]
fn no_two_types_share_a_vref_table_but_the_known_ones() {
    let set = profiles();
    let all: Vec<&Profile> = set.iter().collect();
    let mut copied = Vec::new();
    for (i, a) in all.iter().enumerate() {
        for b in &all[i + 1..] {
            let same = a
                .vref
                .iter()
                .flat_map(|t| &t.flaps)
                .any(|x| b.vref.iter().flat_map(|t| &t.flaps).any(|y| x.kt == y.kt));
            if same {
                let (mut x, mut y) = (a.icao.as_str(), b.icao.as_str());
                if x > y {
                    (x, y) = (y, x);
                }
                copied.push((x.to_owned(), y.to_owned()));
            }
        }
    }
    copied.sort();
    let mut known: Vec<(String, String)> = COPIED_VREF
        .iter()
        .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
        .collect();
    known.sort();
    assert_eq!(copied, known);
}
