//! The tokens of a turn from the agent's own records (`agent_records`): read while the turn runs
//! and once more when it ends, however it ends, and recorded as `session.usage` readings.

use super::*;

/// One turn's reading of its session's records. Dropping it takes the last reading and keeps the
/// reader for the session's next turn.
pub(super) struct RecordsTurn<'a> {
    app: &'a App,
    run: &'a Run,
    ctx: &'a Ctx<'a>,
    reader: Option<crate::agent_records::Reader>,
    next: tokio::time::Instant,
}

impl App {
    /// The records of session `sid` for the turn about to start, in the member's agent configuration
    /// directory.
    // @project-doc docs/interfaces/20261031-events.md#usage_records
    pub(super) fn records_turn<'a>(&'a self, run: &'a Run, ctx: &'a Ctx<'a>, sid: &str) -> RecordsTurn<'a> {
        let reader = run.records.lock().unwrap().remove(sid).or_else(|| {
            let member = run.member(ctx.agent)?;
            crate::agent_records::Reader::new(member.runtime, self.cfg.layout.agent_config(&member.connection), sid)
        });
        let next = tokio::time::Instant::now() + crate::agent_records::EVERY;
        RecordsTurn { app: self, run, ctx, reader, next }
    }
}

impl RecordsTurn<'_> {
    /// When the next reading is due while the turn runs.
    pub(super) fn due(&self) -> Option<tokio::time::Instant> {
        self.reader.as_ref().map(|_| self.next)
    }

    /// Record what changed since the last reading.
    pub(super) fn poll(&mut self) {
        self.next = tokio::time::Instant::now() + crate::agent_records::EVERY;
        self.read(false);
    }

    fn read(&mut self, ended: bool) {
        if let Some(data) = self.reader.as_mut().and_then(|r| r.reading(ended)) {
            self.app.show(self.run, (EventKind::SessionUsage, String::new(), data), self.ctx);
        }
    }
}

impl Drop for RecordsTurn<'_> {
    fn drop(&mut self) {
        self.read(true);
        if let Some(reader) = self.reader.take() {
            self.run.records.lock().unwrap().insert(reader.sid().to_owned(), reader);
        }
    }
}
