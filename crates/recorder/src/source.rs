use std::io;

use fdm_core::{AircraftId, Airport};

/// SimConnect system events a source subscribes to, in subscription order.
pub const SYSTEM_EVENTS: &[&str] = &[
    "SimStart",
    "SimStop",
    "Pause_EX1",
    "Crashed",
    "FlightLoaded",
];

/// What a telemetry source delivers, before any conditioning.
#[derive(Debug, Clone, PartialEq)]
pub enum Telemetry {
    /// One sim frame: raw values in `Acquisition::columns()` order, `NaN`
    /// where a column was not captured.
    Frame(Vec<f64>),
    Pause(bool),
    Event(SimEvent),
}

impl Telemetry {
    /// From a SimConnect system event, with its `data` and file name.
    pub fn from_system_event(name: &str, data: u32, file: &str) -> Option<Self> {
        Some(match name {
            "Pause_EX1" => Self::Pause(data != 0),
            "SimStart" => Self::Event(SimEvent::SimStart),
            "SimStop" => Self::Event(SimEvent::SimStop),
            "Crashed" => Self::Event(SimEvent::Crashed),
            "FlightLoaded" => Self::Event(SimEvent::FlightLoaded(file.to_owned())),
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SimEvent {
    SimStart,
    SimStop,
    Crashed,
    FlightLoaded(String),
    Flow(FlowEvent),
    /// The sim's own runway data for an airport, captured in flight: the
    /// scenery actually flown, which can differ from the real world.
    Airport(Airport),
    /// The user aircraft's identity, at flight start and whenever it
    /// changes: tells the ground segment which type profile applies.
    Aircraft(AircraftId),
    /// A moment the pilot marked from the in-sim panel, with an optional
    /// note: a pilot report for the debrief.
    PilotReport(String),
    /// Where an attempt at a landing challenge begins or ends.
    Attempt(AttemptMarker),
}

/// The start or end of an attempt, tied to it by an opaque `reference`
/// the caller chose (docs/challenge-protocol.md A7). The recorder gives it
/// no meaning; ground tools grade and slice by it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttemptMarker {
    pub phase: AttemptPhase,
    pub challenge: String,
    pub version: u32,
    /// `^[A-Za-z0-9_-]{8,64}$` (`is_reference`), or `None` for a local attempt.
    pub reference: Option<String>,
    /// Why it ended (`stopped`, `abandoned`, `timeout`, `replaced`); `None` at the begin.
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttemptPhase {
    Begin,
    End,
}

/// Whether `s` can be an attempt marker's reference: 8 to 64 of
/// `A-Z a-z 0-9 _ -`.
pub fn is_reference(s: &str) -> bool {
    (8..=64).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// MSFS 2024 flow events (`SIMCONNECT_FLOW_EVENT`, same numbering). They
/// bound a flight and mark discontinuities analysis must not score across.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowEvent {
    FltLoad = 1,
    FltLoaded,
    TeleportStart,
    TeleportDone,
    BackOnTrackStart,
    BackOnTrackDone,
    SkipStart,
    SkipDone,
    BackToMainMenu,
    RtcStart,
    RtcEnd,
    ReplayStart,
    ReplayEnd,
    FlightStart,
    FlightEnd,
    PlaneCrash,
}

impl FlowEvent {
    const ALL: [Self; 16] = [
        Self::FltLoad,
        Self::FltLoaded,
        Self::TeleportStart,
        Self::TeleportDone,
        Self::BackOnTrackStart,
        Self::BackOnTrackDone,
        Self::SkipStart,
        Self::SkipDone,
        Self::BackToMainMenu,
        Self::RtcStart,
        Self::RtcEnd,
        Self::ReplayStart,
        Self::ReplayEnd,
        Self::FlightStart,
        Self::FlightEnd,
        Self::PlaneCrash,
    ];

    pub fn from_code(code: u32) -> Option<Self> {
        Self::ALL.into_iter().find(|e| e.code() == code)
    }

    pub const fn code(self) -> u32 {
        self as u32
    }
}

/// A stream of telemetry: the live simulator or a replayed capture.
pub trait TelemetrySource {
    /// Blocks until the next item; `Ok(None)` once the source has ended.
    fn recv(&mut self) -> io::Result<Option<Telemetry>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_codes_match_the_simconnect_enum() {
        assert_eq!(FlowEvent::from_code(14), Some(FlowEvent::FlightStart));
        assert_eq!(FlowEvent::from_code(16), Some(FlowEvent::PlaneCrash));
        assert_eq!(FlowEvent::from_code(0), None);
        assert!(
            FlowEvent::ALL
                .iter()
                .all(|&e| FlowEvent::from_code(e.code()) == Some(e))
        );
    }
}
