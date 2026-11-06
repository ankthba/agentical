//! `InMemoryJournal` — a concrete [`Journal`] backend for the prototype.
//!
//! Append-only, **per-agent**, cursor-indexed log. The cursor *is* the key: each
//! agent gets a monotone position counter, so entries can never collide (no hash,
//! no key→value map, no collision assumption — exactly the ratified design in
//! `src/journal.rs`).
//!
//! Two cursors per agent:
//! - a **write** cursor (the length of the agent's log) — where the next
//!   [`append`](Journal::append) lands;
//! - a **read** cursor (the replay position) — what [`replay_next`](Journal::replay_next)
//!   consumes next. When the read cursor catches the write cursor, replay is
//!   exhausted and [`Outcome::Fresh`] is returned (the `[O-resume]` transition).
//!
//! Both modes (record / replay) share one log. Recording appends; replaying reads
//! and advances the read cursor; crash-resume = the read cursor walks the surviving
//! prefix and then naturally falls through to `Fresh`, after which the runtime
//! switches back to recording.

use std::cell::Cell;
use std::collections::HashMap;

use crate::journal::{Cursor, EffectRecord, Entry, Event, Journal, Lifecycle, Outcome, Phase};
use crate::types::AgentId;

// DESIGN-NOTE: `Journal::replay_next(&self, ...)` and `checkpoint(&self, ...)` take
// `&self`, but replay must *advance* a per-agent read cursor as it consumes entries.
// We therefore keep the read cursors behind `Cell` (interior mutability) so the
// trait can be satisfied verbatim without changing its signature. Worth feeding back
// to the formal-core agent: either (a) make `replay_next` take `&mut self`, or
// (b) make replay-position an explicit argument / return the next cursor so the
// caller threads it. Option (b) keeps the journal a pure value (nicer for proofs).
//
// DESIGN-NOTE: `replay_next` returns the `Event` but not its `Cursor`. The runtime
// needs the cursor too (to know *where* it is replaying, and to truncate). We expose
// `read_cursor(agent)` as an inherent (non-trait) helper for that. This is the same
// friction as above — the trait under-specifies what replay must hand back.

/// A concrete in-memory [`Journal`]. Per-agent append-only logs plus a per-agent
/// read cursor for replay. Zero external dependencies.
#[derive(Default)]
pub struct InMemoryJournal {
    /// Each agent's append-only log. Index in the `Vec` == the entry's [`Cursor`].
    logs: HashMap<AgentId, Vec<Entry>>,
    /// Per-agent replay read position. Interior-mutable so `replay_next(&self)` works.
    read: HashMap<AgentId, Cell<u64>>,
}

impl InMemoryJournal {
    pub fn new() -> Self {
        Self::default()
    }

    fn log(&self, agent: &AgentId) -> &[Entry] {
        self.logs.get(agent).map(|v| v.as_slice()).unwrap_or(&[])
    }

    fn read_pos(&self, agent: &AgentId) -> &Cell<u64> {
        // Safe to unwrap-or-insert lazily; callers go through methods that ensure it.
        self.read
            .get(agent)
            .expect("read cursor must be initialized via ensure_agent")
    }

    /// Make sure both the log and the read cursor exist for `agent`.
    fn ensure_agent(&mut self, agent: &AgentId) {
        self.logs.entry(agent.clone()).or_default();
        self.read.entry(agent.clone()).or_insert_with(|| Cell::new(0));
    }

    /// The current replay read cursor for `agent` (an inherent helper — see DESIGN-NOTE).
    pub fn read_cursor(&self, agent: &AgentId) -> Cursor {
        match self.read.get(agent) {
            Some(c) => Cursor(c.get()),
            None => Cursor(0),
        }
    }

    /// How many entries `agent` has logged (the write cursor / log length).
    pub fn len(&self, agent: &AgentId) -> u64 {
        self.log(agent).len() as u64
    }

    pub fn is_empty(&self, agent: &AgentId) -> bool {
        self.len(agent) == 0
    }

    /// All entries for `agent`, in order. Used by tests / supervisor recovery.
    pub fn entries(&self, agent: &AgentId) -> &[Entry] {
        self.log(agent)
    }

    /// Rewind the replay read cursor to the start so a fresh replay pass can run.
    /// (A resume re-enters the protected term at position 0 against the prefix.)
    pub fn rewind(&self, agent: &AgentId) {
        if let Some(c) = self.read.get(agent) {
            c.set(0);
        }
    }

    /// Simulate a **crash**: truncate `agent`'s log to its first `keep` entries,
    /// discarding everything after. Models "the disk only durably held a prefix".
    /// The read cursor is rewound to 0 so resume replays the surviving prefix.
    pub fn truncate(&mut self, agent: &AgentId, keep: u64) {
        if let Some(log) = self.logs.get_mut(agent) {
            log.truncate(keep as usize);
        }
        if let Some(c) = self.read.get(agent) {
            c.set(0);
        }
    }

    /// `no-orphaned-effect` detector. Scan `agent`'s surviving log for a durable
    /// effect whose `Intent` (write-ahead record) has **no** matching `Committed`
    /// record after it — a "dangling intent" left by a crash inside the
    /// pending→committed window. Returns the effect names that are dangling, in
    /// log order. Compensation / re-commit is the caller's (Supervisor's) job.
    pub fn dangling_intents(&self, agent: &AgentId) -> Vec<String> {
        self.dangling_intent_records(agent)
            .into_iter()
            .map(|r| r.name)
            .collect()
    }

    /// Like [`dangling_intents`](Self::dangling_intents), but returns the dangling
    /// `Intent` records themselves, so recovery can re-perform an idempotent effect
    /// with its journaled argument.
    pub fn dangling_intent_records(&self, agent: &AgentId) -> Vec<EffectRecord> {
        let mut pending: Vec<EffectRecord> = Vec::new();
        for entry in self.log(agent) {
            if let Event::Effect(rec) = &entry.event {
                match (rec.phase, rec.lifecycle) {
                    // An `Intent` opens a write-ahead window.
                    (Phase::Forward, Lifecycle::Intent) => pending.push(rec.clone()),
                    // A forward `Committed` closes it; so does a `Compensating` record
                    // (the effect was rolled back). Tagging the rollback
                    // `Compensating` is what stops a later replay from undoing twice.
                    (Phase::Forward, Lifecycle::Committed) | (Phase::Compensating, _) => {
                        if let Some(idx) = pending.iter().rposition(|p| p.name == rec.name) {
                            pending.remove(idx);
                        }
                    }
                }
            }
        }
        pending
    }
}

impl Journal for InMemoryJournal {
    fn append(&mut self, agent: &AgentId, event: Event) -> Cursor {
        self.ensure_agent(agent);
        let log = self.logs.get_mut(agent).expect("ensured above");
        let at = Cursor(log.len() as u64);
        log.push(Entry {
            agent: agent.clone(),
            at,
            event,
        });
        at
    }

    fn replay_next(&self, agent: &AgentId) -> Outcome {
        let log = self.log(agent);
        // If the agent was never recorded, there is nothing to replay.
        let pos = match self.read.get(agent) {
            Some(c) => c.get(),
            None => return Outcome::Fresh,
        };
        if (pos as usize) < log.len() {
            let event = log[pos as usize].event.clone();
            self.read_pos(agent).set(pos + 1);
            Outcome::Replay(event)
        } else {
            // Replay has caught up to the live tail: perform the step fresh.
            Outcome::Fresh
        }
    }

    fn checkpoint(&self, agent: &AgentId) -> Cursor {
        // The latest durable resume point is the end of the surviving log.
        Cursor(self.log(agent).len() as u64)
    }
}
