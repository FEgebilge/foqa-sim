use fdm_core::AircraftId;
use recorder::{FlowEvent, SimEvent, Telemetry};

/// What to do with one telemetry item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Skip,
    /// Keep until a flight opens (events that precede the first frame).
    Hold,
    /// Open a flight with the held items, then record this one.
    Open,
    Record,
    /// Record this item, then close the flight.
    Close,
    /// Close the flight and hold this item: it begins the next one, which
    /// opens at the next frame.
    Split,
}

/// Splits the telemetry stream into flights using MSFS flow events. A flight
/// opens on the first frame outside the menu and closes at flight end, on
/// return to the main menu, when a flight file loads, and before a jump
/// (teleport, time skip, back on track), so no flight spans two runs or a
/// jump; the next one opens when the load or jump is done. An aircraft swap
/// splits it too: one flight, one type. Replay frames are dropped: a
/// recorder does not live the same flight twice.
#[derive(Debug, Default)]
pub struct Session {
    open: bool,
    in_menu: bool,
    replaying: bool,
    /// No flight opens until `resume`: its files could not be written.
    blocked: bool,
    /// The app is placing the aircraft (`placing`): its teleport is a
    /// challenge start within the flight, recorded, not split.
    placing: bool,
    /// The sim sends flow events (MSFS 2024). MSFS 2020 has none, so there
    /// a flight file loaded mid-flight is what splits it.
    flows: bool,
    /// The aircraft of the open flight, or of the next one.
    aircraft: Option<AircraftId>,
}

impl Session {
    pub fn route(&mut self, item: &Telemetry) -> Action {
        match item {
            Telemetry::Event(SimEvent::Flow(flow)) => self.flow(*flow),
            Telemetry::Event(SimEvent::Aircraft(id)) => {
                // The type and variant; a new call sign is the same aircraft.
                let swapped = (self.aircraft.replace(id.clone()))
                    .is_some_and(|a| (&a.title, &a.atc_model) != (&id.title, &id.atc_model));
                self.split_if(swapped)
            }
            Telemetry::Event(SimEvent::FlightLoaded(_)) => self.split_if(!self.flows),
            Telemetry::Frame(_)
                if self.replaying || (!self.open && (self.in_menu || self.blocked)) =>
            {
                Action::Skip
            }
            Telemetry::Frame(_) if !self.open => {
                self.open = true;
                Action::Open
            }
            _ => self.held_or_recorded(),
        }
    }

    fn flow(&mut self, flow: FlowEvent) -> Action {
        use FlowEvent::*;
        self.flows = true;
        match flow {
            TeleportStart | TeleportDone if self.placing => {
                self.placing = flow == TeleportStart;
                self.held_or_recorded()
            }
            FlightEnd | BackToMainMenu | FltLoad | TeleportStart | SkipStart | BackOnTrackStart => {
                self.end()
            }
            FlightStart | FltLoaded | TeleportDone | SkipDone | BackOnTrackDone => {
                self.in_menu = false;
                self.held_or_recorded()
            }
            ReplayStart | ReplayEnd => {
                self.replaying = flow == ReplayStart;
                self.held_or_recorded()
            }
            RtcStart | RtcEnd | PlaneCrash => self.held_or_recorded(),
        }
    }

    /// Ends the open flight now, as the return to the main menu does: no
    /// flight opens until the next flight start. `Close` (with no item to
    /// record) if one was open.
    pub fn end(&mut self) -> Action {
        self.in_menu = true;
        self.replaying = false;
        if std::mem::take(&mut self.open) {
            Action::Close
        } else {
            Action::Skip
        }
    }

    /// Leaves the menu state as a flight start does: the next frame opens
    /// a flight.
    pub fn start(&mut self) {
        self.in_menu = false;
    }

    /// The open flight's files failed (the caller closes it): none opens
    /// until `resume`.
    pub fn fail(&mut self) {
        self.open = false;
        self.blocked = true;
    }

    pub fn resume(&mut self) {
        self.blocked = false;
    }

    /// The app placed the aircraft (or, `false`, its teleport is over).
    pub fn placing(&mut self, on: bool) {
        self.placing = on;
    }

    fn split_if(&mut self, split: bool) -> Action {
        if split && std::mem::take(&mut self.open) {
            Action::Split
        } else {
            self.held_or_recorded()
        }
    }

    fn held_or_recorded(&self) -> Action {
        if self.open {
            Action::Record
        } else {
            Action::Hold
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flow(e: FlowEvent) -> Telemetry {
        Telemetry::Event(SimEvent::Flow(e))
    }

    fn frame() -> Telemetry {
        Telemetry::Frame(vec![0.0])
    }

    fn routes(items: &[Telemetry]) -> Vec<Action> {
        let mut session = Session::default();
        items.iter().map(|i| session.route(i)).collect()
    }

    #[test]
    fn flight_runs_from_first_frame_to_flight_end() {
        use Action::*;
        let items = [
            flow(FlowEvent::FlightStart),
            frame(),
            frame(),
            flow(FlowEvent::FlightEnd),
            frame(),
        ];
        assert_eq!(routes(&items), [Hold, Open, Record, Close, Skip]);
    }

    #[test]
    fn replay_frames_are_dropped_but_the_markers_kept() {
        use Action::*;
        let items = [
            frame(),
            flow(FlowEvent::ReplayStart),
            frame(),
            flow(FlowEvent::ReplayEnd),
            frame(),
        ];
        assert_eq!(routes(&items), [Open, Record, Skip, Record, Record]);
    }

    #[test]
    fn next_flight_opens_after_returning_from_the_menu() {
        use Action::*;
        let items = [
            frame(),
            flow(FlowEvent::BackToMainMenu),
            frame(),
            Telemetry::Pause(true),
            flow(FlowEvent::FlightStart),
            frame(),
        ];
        assert_eq!(routes(&items), [Open, Close, Skip, Hold, Hold, Open]);
    }

    #[test]
    fn ending_a_flight_waits_for_the_next_flight_start() {
        use Action::*;
        let mut session = Session::default();
        assert_eq!(session.route(&frame()), Open);
        assert_eq!(session.end(), Close);
        assert_eq!(session.end(), Skip);
        assert_eq!(session.route(&frame()), Skip);
        assert_eq!(session.route(&flow(FlowEvent::FlightStart)), Hold);
        assert_eq!(session.route(&frame()), Open);
    }

    #[test]
    fn starting_after_an_end_opens_at_the_next_frame() {
        use Action::*;
        let mut session = Session::default();
        assert_eq!(session.route(&frame()), Open);
        assert_eq!(session.end(), Close);
        assert_eq!(session.route(&frame()), Skip);
        session.start();
        assert_eq!(session.route(&frame()), Open);
    }

    fn aircraft(title: &str) -> Telemetry {
        Telemetry::Event(SimEvent::Aircraft(AircraftId {
            title: title.into(),
            atc_model: "ATCCOM.AC_MODEL C172.0.text".into(),
            atc_type: String::new(),
            atc_id: "BILGE".into(),
        }))
    }

    #[test]
    fn an_aircraft_swap_starts_a_new_flight() {
        use Action::*;
        let items = [
            aircraft("C172"),
            frame(),
            aircraft("C172"),
            aircraft("A320"),
            frame(),
            aircraft("A320"),
        ];
        assert_eq!(routes(&items), [Hold, Open, Record, Split, Open, Record]);
    }

    #[test]
    fn a_load_or_a_jump_ends_the_flight_until_it_is_done() {
        use Action::*;
        for (start, done) in [
            (FlowEvent::FltLoad, FlowEvent::FltLoaded),
            (FlowEvent::TeleportStart, FlowEvent::TeleportDone),
            (FlowEvent::SkipStart, FlowEvent::SkipDone),
            (FlowEvent::BackOnTrackStart, FlowEvent::BackOnTrackDone),
        ] {
            let items = [frame(), flow(start), frame(), flow(done), frame()];
            assert_eq!(routes(&items), [Open, Close, Skip, Hold, Open], "{start:?}");
        }
    }

    #[test]
    fn the_apps_own_teleport_stays_in_the_flight() {
        use Action::*;
        let mut session = Session::default();
        assert_eq!(session.route(&frame()), Open);
        session.placing(true);
        assert_eq!(session.route(&flow(FlowEvent::TeleportStart)), Record);
        assert_eq!(session.route(&flow(FlowEvent::TeleportDone)), Record);
        // The next one is the pilot's.
        assert_eq!(session.route(&flow(FlowEvent::TeleportStart)), Close);
    }

    #[test]
    fn without_flow_events_a_flight_file_load_splits() {
        use Action::*;
        let loaded = || Telemetry::Event(SimEvent::FlightLoaded("restart.FLT".into()));
        // MSFS 2020: no flow events.
        assert_eq!(routes(&[frame(), loaded(), frame()]), [Open, Split, Open]);
        // MSFS 2024 splits on its flow events instead.
        let items = [flow(FlowEvent::FlightStart), frame(), loaded(), frame()];
        assert_eq!(routes(&items), [Hold, Open, Record, Record]);
    }

    #[test]
    fn a_failed_flight_reopens_only_once_resumed() {
        use Action::*;
        let mut session = Session::default();
        assert_eq!(session.route(&frame()), Open);
        session.fail();
        assert_eq!(session.route(&frame()), Skip);
        session.resume();
        assert_eq!(session.route(&frame()), Open);
    }
}
