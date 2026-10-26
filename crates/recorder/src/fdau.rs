use std::collections::VecDeque;

use fdm_core::{Lfl, Sample, SignalId, SimTime, Ssm};

use crate::Conditioned;

/// Full-rate capture on either side of a touchdown (K2), in microseconds.
pub const TOUCHDOWN_WINDOW_US: i64 = 10_000_000;

/// Sim time going back by more than this is a discontinuity (flight reload),
/// not jitter.
const REBASE_BACK_US: i64 = 1_000_000;

/// Frame period assumed until two frames give a real one: 50 Hz.
const DEFAULT_STEP_US: i64 = 20_000;

/// A frame kept for the pre-touchdown buffer, with which values the regular
/// schedule already recorded.
struct Held {
    t: SimTime,
    values: Vec<Option<(f64, Ssm)>>,
    recorded: Vec<bool>,
}

/// Flight Data Acquisition Unit. Records each parameter at its LFL rate by
/// taking the instantaneous value at each slot, as a real FDAU does. Around
/// each touchdown it records every frame instead, using a 10 s ring buffer
/// for the part before the trigger, like an ACMS pre-trigger buffer.
///
/// Recorded time is monotonic. Small backward steps (up to 1 s) are jitter
/// and dropped; a larger one is a flight reload inside one recording, so the
/// time origin is moved to continue one frame step after the last recorded
/// frame instead of going silent until the sim clock overtakes the old maximum.
pub struct Fdau {
    period_us: Vec<i64>,
    next_slot_us: Vec<i64>,
    air_ground: usize,
    paused: SignalId,
    t0: Option<f64>,
    last: Option<SimTime>,
    step_us: i64,
    on_ground: Option<bool>,
    window_until: Option<SimTime>,
    buffer: VecDeque<Held>,
}

impl Fdau {
    pub fn new(lfl: &Lfl) -> Self {
        let id = |name| {
            lfl.by_name(name)
                .unwrap_or_else(|| panic!("LFL lacks {name}"))
                .id
        };
        let n = lfl.signals().len();
        Self {
            period_us: lfl
                .signals()
                .iter()
                .map(|s| (1e6 / s.rate_hz).round() as i64)
                .collect(),
            next_slot_us: vec![0; n],
            air_ground: usize::from(id("air_ground").0),
            paused: id("sim.paused"),
            t0: None,
            last: None,
            step_us: DEFAULT_STEP_US,
            on_ground: None,
            window_until: None,
            buffer: VecDeque::new(),
        }
    }

    /// Samples one conditioned frame and appends what is recorded to `out`.
    /// Frames whose sim time has not advanced (pause, repeats) are dropped.
    /// Samples released from the pre-touchdown buffer are older than the
    /// current frame: the output is self-timestamped, not time-ordered.
    pub fn frame(&mut self, frame: &Conditioned, out: &mut Vec<Sample>) {
        let t0 = *self.t0.get_or_insert(frame.sim_time_s);
        let mut t = SimTime::from_secs_f64(frame.sim_time_s - t0);
        if let Some(last) = self.last {
            if last.us_since(t) > REBASE_BACK_US {
                t = last.add_us(self.step_us);
                self.t0 = Some(frame.sim_time_s - t.as_secs_f64());
            }
            if t <= last {
                return;
            }
            self.step_us = t.us_since(last).min(REBASE_BACK_US);
        }
        self.last = Some(t);

        let on_ground = frame.values[self.air_ground].map(|(v, _)| v >= 0.5);
        if self.on_ground == Some(false) && on_ground == Some(true) {
            self.window_until = Some(t.add_us(TOUCHDOWN_WINDOW_US));
            self.buffer.iter_mut().for_each(|held| held.release(out));
        }
        self.on_ground = on_ground.or(self.on_ground);

        let in_window = self.window_until.is_some_and(|until| t <= until);
        let mut recorded = vec![false; frame.values.len()];
        for (i, value) in frame.values.iter().enumerate() {
            let Some((value, ssm)) = *value else { continue };
            let slot = t.as_micros() >= self.next_slot_us[i];
            while self.next_slot_us[i] <= t.as_micros() {
                self.next_slot_us[i] += self.period_us[i];
            }
            if slot || in_window {
                emit(out, t, i, value, ssm);
                recorded[i] = true;
            }
        }

        self.buffer.push_back(Held {
            t,
            values: frame.values.clone(),
            recorded,
        });
        while self
            .buffer
            .front()
            .is_some_and(|h| t.us_since(h.t) >= TOUCHDOWN_WINDOW_US)
        {
            self.buffer.pop_front();
        }
    }

    /// Sim time of the last recorded frame.
    pub fn now(&self) -> Option<SimTime> {
        self.last
    }

    /// Records a pause transition as `sim.paused`, at the last frame's time.
    pub fn pause(&mut self, paused: bool, out: &mut Vec<Sample>) {
        if let Some(t) = self.last {
            out.push(Sample::new(t, self.paused, f64::from(u8::from(paused))));
        }
    }
}

impl Held {
    /// Records the values the regular schedule skipped, at this frame's time.
    fn release(&mut self, out: &mut Vec<Sample>) {
        for (i, (value, recorded)) in self.values.iter().zip(&mut self.recorded).enumerate() {
            if let (Some((value, ssm)), false) = (*value, *recorded) {
                emit(out, self.t, i, value, ssm);
                *recorded = true;
            }
        }
    }
}

/// Appends the sample of signal column `i`.
fn emit(out: &mut Vec<Sample>, t: SimTime, i: usize, value: f64, ssm: Ssm) {
    out.push(Sample {
        t,
        signal: SignalId(i as u16),
        value,
        ssm,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(secs: f64, on_ground: f64) -> Conditioned {
        let mut values = vec![None; Lfl::V1.signals().len()];
        for name in ["alt.pressure", "air_ground"] {
            let id = usize::from(Lfl::V1.by_name(name).unwrap().id.0);
            values[id] = Some((
                if name == "air_ground" {
                    on_ground
                } else {
                    1000.0
                },
                Ssm::Normal,
            ));
        }
        Conditioned {
            sim_time_s: secs,
            values,
        }
    }

    fn count(out: &[Sample], name: &str) -> usize {
        let id = Lfl::V1.by_name(name).unwrap().id;
        out.iter().filter(|s| s.signal == id).count()
    }

    #[test]
    fn records_each_parameter_at_its_rate() {
        let mut fdau = Fdau::new(&Lfl::V1);
        let mut out = Vec::new();
        for i in 0..1000 {
            fdau.frame(&frame(f64::from(i) / 50.0, 0.0), &mut out); // 20 s at 50 Hz, airborne
        }
        assert_eq!(count(&out, "alt.pressure"), 20); // 1 Hz
        assert_eq!(count(&out, "air_ground"), 80); // 4 Hz
    }

    #[test]
    fn drops_frames_whose_sim_time_did_not_advance() {
        let mut fdau = Fdau::new(&Lfl::V1);
        let mut out = Vec::new();
        for _ in 0..3 {
            fdau.frame(&frame(5.0, 0.0), &mut out);
        }
        assert_eq!(count(&out, "alt.pressure"), 1);
    }

    #[test]
    fn touchdown_records_every_frame_ten_seconds_either_side() {
        let mut fdau = Fdau::new(&Lfl::V1);
        let mut out = Vec::new();
        for i in 0..3000 {
            let secs = f64::from(i) / 50.0; // 60 s at 50 Hz, touchdown at 30 s
            fdau.frame(&frame(secs, if secs < 30.0 { 0.0 } else { 1.0 }), &mut out);
        }
        let id = Lfl::V1.by_name("alt.pressure").unwrap().id;
        let in_window = out
            .iter()
            .filter(|s| s.signal == id && (20.0..=40.0).contains(&s.t.as_secs_f64()))
            .count();
        assert_eq!(in_window, 1001); // every frame from 20.00 s to 40.00 s
        assert_eq!(count(&out, "alt.pressure"), 1001 + 20 + 19); // plus 1 Hz slots outside
    }

    #[test]
    fn a_backward_time_jump_is_rebased_not_silenced() {
        let mut fdau = Fdau::new(&Lfl::V1);
        let mut out = Vec::new();
        // 50 Hz: 0..10 s, then a flight reload jumps back to 2 s and runs to 6 s.
        for i in (0..=500).chain(100..=300) {
            fdau.frame(&frame(f64::from(i) / 50.0, 0.0), &mut out);
        }
        let id = Lfl::V1.by_name("alt.pressure").unwrap().id;
        let times: Vec<i64> = out
            .iter()
            .filter(|s| s.signal == id)
            .map(|s| s.t.as_micros())
            .collect();
        assert!(times.windows(2).all(|w| w[0] < w[1]), "{times:?}");
        assert!(times.windows(2).all(|w| w[1] - w[0] <= 1_000_000));
        assert_eq!(times.len(), 15); // 1 Hz over 10 s + 4 s rebased, as one timeline
    }

    #[test]
    fn a_small_backward_jitter_is_still_dropped() {
        let mut fdau = Fdau::new(&Lfl::V1);
        let mut out = Vec::new();
        for secs in [0.0, 5.0, 4.5, 5.02] {
            fdau.frame(&frame(secs, 0.0), &mut out);
        }
        assert_eq!(fdau.now(), Some(SimTime::from_secs_f64(5.02)));
    }
}
