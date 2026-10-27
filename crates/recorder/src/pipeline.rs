use std::io::{self, Write};

use fdm_core::{Lfl, Sample, SimTime};

use crate::{Acquisition, Fdau, JournalWriter, SimEvent, Telemetry, TelemetrySource};

/// The on-board chain in one piece: wiring, FDAU and journal.
pub struct Recorder<W: Write> {
    acquisition: Acquisition,
    fdau: Fdau,
    journal: JournalWriter<W>,
    batch: Vec<Sample>,
}

impl<W: Write> Recorder<W> {
    pub fn new(lfl: &Lfl, out: W) -> io::Result<Self> {
        let mut journal = JournalWriter::new(out, lfl)?;
        journal.event(SimTime::ZERO, &SimEvent::GearScaledV1)?;
        Ok(Self {
            acquisition: Acquisition::new(lfl),
            fdau: Fdau::new(lfl),
            journal,
            batch: Vec::new(),
        })
    }

    /// The raw columns a source must deliver.
    pub fn acquisition(&self) -> &Acquisition {
        &self.acquisition
    }

    /// Records one item and flushes, so a killed process loses at most the
    /// item in flight. Events are stamped with the last frame's time.
    pub fn record(&mut self, item: &Telemetry) -> io::Result<()> {
        match item {
            Telemetry::Frame(raw) => {
                if let Some(frame) = self.acquisition.condition(raw) {
                    self.fdau.frame(&frame, &mut self.batch);
                }
            }
            Telemetry::Pause(paused) => self.fdau.pause(*paused, &mut self.batch),
            Telemetry::Event(event) => self
                .journal
                .event(self.fdau.now().unwrap_or(SimTime::ZERO), event)?,
        }
        for sample in self.batch.drain(..) {
            self.journal.sample(&sample)?;
        }
        self.journal.flush()
    }

    /// Records everything a source delivers until it ends.
    pub fn run(&mut self, source: &mut impl TelemetrySource) -> io::Result<()> {
        while let Some(item) = source.recv()? {
            self.record(&item)?;
        }
        Ok(())
    }

    pub fn into_inner(self) -> W {
        self.journal.into_inner()
    }
}
