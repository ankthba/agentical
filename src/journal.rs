//! Primitive #5 — **Journal**: durable event history & replay. *Novel core (2 of 2).*
//!
//! An append-only, **per-agent**, position-indexed log of the realized outcomes of
//! stochastic/effectful steps. The same agent code runs in two modes:
//! - *record*: a step samples for real and [`append`](Journal::append)s its outcome.
//! - *replay*: a step consumes its recorded outcome via
//!   [`replay_next`](Journal::replay_next) **instead of** re-sampling.
//!
//! ## Replay soundness (Theorem 1 of the paper), as realized here
//! - Replay reproduces the recorded observable trace with probability 1: the journal
//!   is authoritative and `[O-rep]` is Dirac.
//! - Record and replay are a coupling of the same oracle randomness, conditioned on
//!   the journal (see the proof of T1 in `paper/sections/metatheory.tex`).
//!
//! ## Design decisions
//! - **The cursor *is* the key** (a monotone position counter per agent): no key→value
//!   map, no hash, hence no collision assumption.
//! - **Write-ahead logging** for durable effects: an `Intent` record (with the effect's
//!   argument) is appended *before* the effect executes and a `Committed` record (with
//!   its result) *after*. A crash in between is detected on resume and recovered.
//! - **Phase tags** [`Phase::Forward`] vs [`Phase::Compensating`] stop a crash in the
//!   middle of a saga from compensating twice on replay.

use crate::oracle::Sampled;
use crate::types::{AgentId, Term};

/// A position in an agent's append-only log. The cursor *is* the key: positions are a
/// monotone counter, so entries can never collide.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Cursor(pub u64);

/// The realized outcome a journal entry records. Anything *externally determined* — an
/// Oracle draw, a channel receive, a committed durable effect — must be journaled for
/// replay to be sound. Position/owner live in [`Entry`], not here.
#[derive(Clone, Debug)]
pub enum Event {
    /// An Oracle draw and the term it produced (the only stochastic step).
    OracleDraw(Sampled),
    /// A value received over a channel (externally determined ⇒ journaled).
    ChannelRecv(Vec<u8>),
    /// A durable, possibly-irreversible external effect, write-ahead logged.
    Effect(EffectRecord),
}

/// A write-ahead-logged durable effect. `Intent` is journaled *before* the effect runs,
/// `Committed` *after* it succeeds; the gap is the crash window resume must reconcile.
#[derive(Clone, Debug)]
pub struct EffectRecord {
    pub name: String,
    pub lifecycle: Lifecycle,
    pub phase: Phase,
    /// The value the record carries: the effect's *argument* on an `Intent` record
    /// and on a `Compensating` record, and the effect's *result* on a forward
    /// `Committed` record. Replay reads the result from here instead of
    /// re-performing the effect.
    pub value: Term,
}

/// Commit lifecycle of a durable effect (the write-ahead-logging discipline).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lifecycle {
    /// Effect intended but not yet confirmed (WAL record written before execution).
    Intent,
    /// Effect confirmed committed to the world.
    Committed,
}

/// Whether an entry is a normal forward effect or a saga rollback. Journaling
/// compensations as `Compensating` makes a mid-saga crash replay-safe (no double undo).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Forward,
    Compensating,
}

/// One positioned, agent-scoped entry in the append-only log.
#[derive(Clone, Debug)]
pub struct Entry {
    pub agent: AgentId,
    pub at: Cursor,
    pub event: Event,
}

/// What replay yields at an agent's current position.
#[derive(Clone, Debug)]
pub enum Outcome {
    /// Reuse this recorded event instead of performing the step (replay mode).
    Replay(Event),
    /// Replay has caught up to the live tail — perform the step fresh and append it.
    Fresh,
}

/// Per-agent append-only log that makes an otherwise-stochastic system replayable, and
/// therefore durable. Backends are pluggable (file, embedded KV, in-memory for tests)
/// and require **no cloud dependency** — works offline / at the edge.
pub trait Journal {
    /// Append an event at the next position in `agent`'s log; returns its cursor.
    /// Durable-effect discipline: append `Intent`, execute the effect, append `Committed`.
    fn append(&mut self, agent: &AgentId, event: Event) -> Cursor;

    /// The recorded event at `agent`'s current replay position, or `Fresh` once replay
    /// has caught up to the tail.
    fn replay_next(&self, agent: &AgentId) -> Outcome;

    /// Latest durable resume point for `agent`.
    fn checkpoint(&self, agent: &AgentId) -> Cursor;
}
