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
}

/// Splits the telemetry stream into flights using MSFS flow events. A flight
/// opens on the first frame outside the menu and closes at flight end or on
/// return to the main menu. Replay frames are dropped: a recorder does not
/// live the same flight twice.
#[derive(Debug, Default)]
pub struct Session {
    open: bool,
    in_menu: bool,
    replaying: bool,
}

impl Session {
    pub fn route(&mut self, item: &Telemetry) -> Action {
        match item {
            Telemetry::Event(SimEvent::Flow(FlowEvent::FlightEnd | FlowEvent::BackToMainMenu)) => {
                self.end()
            }
            Telemetry::Event(SimEvent::Flow(FlowEvent::FlightStart)) => {
                self.in_menu = false;
                self.held_or_recorded()
            }
            Telemetry::Event(SimEvent::Flow(
                flow @ (FlowEvent::ReplayStart | FlowEvent::ReplayEnd),
            )) => {
                self.replaying = *flow == FlowEvent::ReplayStart;
                self.held_or_recorded()
            }
            Telemetry::Frame(_) if self.replaying || (self.in_menu && !self.open) => Action::Skip,
            Telemetry::Frame(_) if !self.open => {
                self.open = true;
                Action::Open
            }
            _ => self.held_or_recorded(),
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
}
