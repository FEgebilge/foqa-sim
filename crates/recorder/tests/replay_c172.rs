//! Records the M1 C172 traffic pattern through the whole on-board chain
//! (wiring, FDAU, journal) and checks what reads back.

use std::sync::OnceLock;

use fdm_core::{Lfl, Sample, SimTime};
use recorder::{JournalReader, Record, Recorder, ReplaySource, TOUCHDOWN_WINDOW_US};

const C172: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/c172-pattern.raw.jsonl.gz"
);

fn recorded() -> &'static [Sample] {
    static OUT: OnceLock<Vec<Sample>> = OnceLock::new();
    OUT.get_or_init(|| {
        let mut recorder = Recorder::new(&Lfl::V1, Vec::new()).unwrap();
        let mut source = ReplaySource::open(C172, recorder.acquisition()).unwrap();
        recorder.run(&mut source).unwrap();
        let journal = recorder.into_inner();
        let mut reader = JournalReader::new(&journal[..]).unwrap();
        let samples = reader
            .by_ref()
            .filter_map(|r| match r.unwrap() {
                Record::Sample(s) => Some(s),
                Record::Event(..) => None,
            })
            .collect();
        assert_eq!(reader.damaged_at(), None);
        samples
    })
}

fn series(name: &str) -> Vec<&'static Sample> {
    let id = Lfl::V1.by_name(name).unwrap().id;
    let mut s: Vec<_> = recorded().iter().filter(|s| s.signal == id).collect();
    s.sort_by_key(|s| s.t);
    s
}

/// First air-to-ground transition.
fn touchdown() -> SimTime {
    let ground = series("air_ground");
    ground
        .windows(2)
        .find(|w| w[0].value < 0.5 && w[1].value >= 0.5)
        .unwrap()[1]
        .t
}

fn near_touchdown(s: &Sample) -> bool {
    (s.t.as_micros() - touchdown().as_micros()).abs() <= TOUCHDOWN_WINDOW_US
}

#[test]
fn one_hz_parameter_keeps_its_rate_outside_the_window() {
    let alt = series("alt.pressure");
    let span = alt.last().unwrap().t.as_secs_f64() - 20.0;
    let outside = alt.iter().filter(|s| !near_touchdown(s)).count() as f64;
    assert!(
        (outside - span).abs() <= 2.0,
        "{outside} samples over {span:.0} s"
    );
}

#[test]
fn touchdown_window_is_recorded_at_frame_rate() {
    let in_window = series("accel.normal")
        .into_iter()
        .filter(|s| near_touchdown(s))
        .count();
    assert!(
        in_window > 20 * 60,
        "only {in_window} samples in the 20 s window"
    );
}

#[test]
fn no_parameter_repeats_a_timestamp() {
    for def in Lfl::V1.signals() {
        let s = series(def.name);
        assert!(
            s.windows(2).all(|w| w[0].t < w[1].t),
            "{} repeats a timestamp",
            def.name
        );
    }
}

#[test]
fn climb_pitch_is_nose_up_positive() {
    let ground = series("air_ground");
    let liftoff = ground
        .windows(2)
        .find(|w| w[0].value >= 0.5 && w[1].value < 0.5)
        .unwrap()[1]
        .t;
    let climb = series("pitch")
        .into_iter()
        .filter(|s| (s.t > liftoff) && s.t.as_secs_f64() < liftoff.as_secs_f64() + 20.0);
    assert!(climb.map(|s| s.value).fold(f64::MIN, f64::max) > 5.0);
}

#[test]
fn parameters_the_capture_lacks_are_absent_not_zero() {
    assert!(series("eng1.n1").is_empty());
    assert!(!series("alt.radio").is_empty());
}
