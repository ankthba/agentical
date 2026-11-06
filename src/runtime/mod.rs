//! # `agentical::runtime` — a minimal but real working runtime
//!
//! This module turns the design-stage traits in `src/*.rs` into a running system
//! that **empirically demonstrates the flagship theorem T1 (replay soundness under
//! nondeterminism)** from the paper (`paper/`), for a single agent.
//!
//! It contains, with **zero external dependencies** (std only):
//! - [`InMemoryJournal`] — the concrete [`Journal`](crate::journal::Journal):
//!   append-only, per-agent, the cursor *is* the key, with write-ahead
//!   [`Lifecycle`](crate::journal::Lifecycle)/[`Phase`](crate::journal::Phase) tags.
//! - [`SeededOracle`] — the concrete [`Oracle`](crate::oracle::Oracle): a real
//!   seeded categorical sampler in record mode; its draw is read from the journal in
//!   replay mode.
//! - [`Runtime`] — a driver that steps a [`ToyAgent`] in [`Mode::Record`] or
//!   [`Mode::Replay`], journaling oracle draws and durable effects under the
//!   `[O-rec]`/`[O-rep]` and `[Eff-rec]`/`[Eff-rep]` rules, and detecting dangling
//!   effect intents on resume (the *no-orphaned-effect* property).
//! - [`ToyAgent`] — a 3-stochastic-step "sample → branch → sample → commit effect"
//!   reasoning loop that exercises all of the above.
//!
//! The end-to-end T1 demonstration lives in `tests/replay.rs`.

pub mod agent;
pub mod journal;
pub mod oracle;
pub mod rng;

pub use agent::{drive, ToyAgent, ToyState};
pub use journal::InMemoryJournal;
pub use oracle::SeededOracle;

use crate::agent::Step;
use crate::journal::{
    Cursor, EffectRecord, Event, Journal, Lifecycle, Outcome, Phase,
};
use crate::oracle::{Oracle, Prompt, Sampled};
use crate::types::{AgentId, Term};

/// Record vs. replay — which semantics the runtime gives the shared term rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// `[O-rec]` / `[Eff-rec]`: sample/perform for real and append to the journal.
    Record,
    /// `[O-rep]` / `[Eff-rep]`: consume the journaled outcome instead. Falls through
    /// to record at any step where the journal is exhausted (`[O-resume]`).
    Replay,
}

/// One observable transition label, in the spirit of the paper's transition labels. The label
/// **carries the realized value** here (not just the key) because T1a is about the
/// *observable trace* being byte-identical between record and replay — the realized
/// values are exactly what we assert equal. (The formal `a ::= o(key) | eff(key,f)`
/// deliberately hides the value so two record runs with different draws share a
/// label; for the empirical equality test we want the values in the trace.)
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TraceLabel {
    /// An oracle draw at `key` resolved to `term`.
    Oracle { key: u64, term: Term },
    /// A durable effect `name` committed with result `term`.
    Effect { key: u64, name: String, term: Term },
    /// The agent halted with `out`.
    Done { out: Term },
}

/// The effect surface the agent reaches through its `State`. The runtime owns the
/// oracle and journal; `Ports` lends the agent record-or-replay-aware `oracle_draw`
/// and `durable_effect` operations. (See the DESIGN-NOTE in `agent.rs` about why the
/// effect surface is threaded through `State` rather than a `ctx` argument.)
pub struct Ports<'a> {
    agent: AgentId,
    mode: Mode,
    oracle: &'a dyn Oracle,
    journal: &'a mut InMemoryJournal,
    /// Accumulating observable trace for this run.
    trace: &'a mut Vec<TraceLabel>,
    /// Set if a durable effect was journaled `Intent` but a crash (or this turn's
    /// end) prevented the matching `Committed` — surfaced for the supervisor.
    pub crash_after_intent: bool,
    /// If `true`, simulate a crash *between* an effect's Intent and Committed
    /// records: write Intent, perform, but DO NOT write Committed (then stop).
    pub crash_in_effect_window: bool,
    /// Set when replay reaches an effect that recovery rolled back (an `Intent`
    /// followed by a `Compensating` record): the supervisor abandoned the protected
    /// term, so the run stops without an output.
    pub compensated: bool,
    /// Number of times this run actually performed an effect in the world (not
    /// counting effects whose committed result was replayed from the journal).
    pub effects_performed: u32,
}

impl<'a> Ports<'a> {
    /// `let x ⇐ oracle(e)` — [`O-rec`]/[`O-rep`]. In replay mode, read the journaled
    /// draw; if replay is exhausted, fall through to a fresh sample and record it.
    pub fn oracle_draw(&mut self, prompt: &str) -> Term {
        // `key` is the journal cursor of the entry that produced this draw. On
        // replay it is the read cursor *after* `replay_next` advanced past the
        // consumed entry (so `read_cursor - 1`); on a fresh record it is the
        // append position (`checkpoint - 1`). Computing it from whichever path ran
        // keeps the label's key identical between record and replay (the trace
        // alphabet is replay-invariant).
        let (sampled, key): (Sampled, u64) = match self.mode {
            Mode::Replay => match self.journal.replay_next(&self.agent) {
                Outcome::Replay(Event::OracleDraw(s)) => {
                    let k = self.journal.read_cursor(&self.agent).0.saturating_sub(1);
                    (s, k)
                }
                Outcome::Replay(other) => {
                    panic!("journal desync: expected OracleDraw, found {other:?}");
                }
                Outcome::Fresh => {
                    // [O-resume]: the replayed prefix is exhausted. Switch this run to
                    // record mode for good, so later steps append fresh entries
                    // instead of re-reading the entry we are about to append.
                    self.mode = Mode::Record;
                    let s = self.record_oracle(prompt);
                    let k = self.journal.checkpoint(&self.agent).0.saturating_sub(1);
                    (s, k)
                }
            },
            Mode::Record => {
                let s = self.record_oracle(prompt);
                let k = self.journal.checkpoint(&self.agent).0.saturating_sub(1);
                (s, k)
            }
        };
        self.trace.push(TraceLabel::Oracle {
            key,
            term: sampled.term.clone(),
        });
        sampled.term
    }

    /// Fresh sample + journal append (`[O-rec]`). The cursor it lands at is the key.
    fn record_oracle(&mut self, prompt: &str) -> Sampled {
        let sampled = self.oracle.sample(Prompt(prompt.to_string()));
        self.journal
            .append(&self.agent, Event::OracleDraw(sampled.clone()));
        sampled
    }

    /// `do[f] e` — [`Eff-rec`]/[`Eff-rep`] with the write-ahead discipline:
    /// append `Intent`, perform, append `Committed`. In replay, reuse the recorded
    /// committed result and DO NOT re-perform. If `crash_in_effect_window` is set,
    /// we append `Intent`, "perform", and then stop *without* the `Committed`
    /// record — leaving a dangling intent for resume to detect.
    pub fn durable_effect(&mut self, name: &str, arg: Term) -> Term {
        if self.mode == Mode::Replay {
            match self.journal.replay_next(&self.agent) {
                Outcome::Replay(Event::Effect(rec)) => {
                    if rec.lifecycle != Lifecycle::Intent || rec.phase != Phase::Forward {
                        panic!("journal desync: expected a forward Intent, found {rec:?}");
                    }
                    // The Intent opens the write-ahead window; the next entry (if any)
                    // closes it.
                    match self.journal.replay_next(&self.agent) {
                        Outcome::Replay(Event::Effect(c))
                            if c.lifecycle == Lifecycle::Committed && c.phase == Phase::Forward =>
                        {
                            // [Eff-rep]: reuse the journaled result; do NOT re-perform.
                            let key = self.journal.read_cursor(&self.agent).0.saturating_sub(1);
                            self.trace.push(TraceLabel::Effect {
                                key,
                                name: name.to_string(),
                                term: c.value.clone(),
                            });
                            return c.value;
                        }
                        Outcome::Replay(Event::Effect(c)) if c.phase == Phase::Compensating => {
                            // Recovery rolled this effect back: the supervisor abandoned
                            // the protected term, so the run stops here, again.
                            self.compensated = true;
                            return Term(String::new());
                        }
                        Outcome::Replay(other) => {
                            panic!("journal desync: expected the effect's completion, found {other:?}");
                        }
                        Outcome::Fresh => {
                            // Intent without completion: the crash landed inside the
                            // write-ahead window. Stop and let recovery reconcile it.
                            self.crash_after_intent = true;
                            return Term(String::new());
                        }
                    }
                }
                Outcome::Replay(other) => {
                    panic!("journal desync: expected Effect, found {other:?}");
                }
                Outcome::Fresh => {
                    // [O-resume] for effects: the replayed prefix is exhausted.
                    self.mode = Mode::Record;
                }
            }
        }

        // [Eff-intent]: write-ahead log the intent (with its argument) BEFORE performing.
        self.journal.append(
            &self.agent,
            Event::Effect(EffectRecord {
                name: name.to_string(),
                lifecycle: Lifecycle::Intent,
                phase: Phase::Forward,
                value: arg.clone(),
            }),
        );
        // [Eff-perform]: a pure deterministic transform stands in for a real side
        // effect (a tool call, a write to the world).
        let result = effect_result(name, &arg);
        self.effects_performed += 1;

        if self.crash_in_effect_window {
            // Simulated crash: intent written, effect performed, but the process
            // dies before the Committed record is durably written.
            self.crash_after_intent = true;
            return result;
        }

        // [Eff-commit]: journal the committed result AFTER the effect succeeds.
        self.journal.append(
            &self.agent,
            Event::Effect(EffectRecord {
                name: name.to_string(),
                lifecycle: Lifecycle::Committed,
                phase: Phase::Forward,
                value: result.clone(),
            }),
        );
        let key = self.journal.checkpoint(&self.agent).0.saturating_sub(1);
        self.trace.push(TraceLabel::Effect {
            key,
            name: name.to_string(),
            term: result.clone(),
        });
        result
    }
}

/// The deterministic "world effect" the toy `commit` performs: prefix the argument.
/// (Idempotent in the sense that re-running it on the same arg yields the same
/// result — which is exactly what makes `[Eff-rep]` reuse / re-commit sound.)
fn effect_result(name: &str, arg: &Term) -> Term {
    Term(format!("{name}({})", arg.0))
}

/// The driver. Owns the journal and an oracle; steps the toy agent to completion in a
/// chosen [`Mode`], producing the observable [`TraceLabel`] sequence.
pub struct Runtime<'o> {
    agent_id: AgentId,
    oracle: &'o dyn Oracle,
    pub journal: InMemoryJournal,
}

/// The result of driving one run.
#[derive(Debug)]
pub struct RunResult {
    /// The observable trace (the object T1a asserts is byte-identical on replay).
    pub trace: Vec<TraceLabel>,
    /// The agent's final output, if it halted this turn.
    pub output: Option<Term>,
    /// True if the run stopped mid-effect (Intent written, no Committed) — a
    /// simulated crash inside the write-ahead window.
    pub crashed_in_effect: bool,
    /// True if replay reached an effect that recovery compensated, so the run
    /// stopped without an output.
    pub compensated: bool,
    /// How many effects this run performed in the world (replayed effects excluded).
    pub effects_performed: u32,
}

impl<'o> Runtime<'o> {
    pub fn new(agent_id: AgentId, oracle: &'o dyn Oracle) -> Self {
        Self {
            agent_id,
            oracle,
            journal: InMemoryJournal::new(),
        }
    }

    /// Adopt an existing (possibly crash-truncated) journal — used for resume.
    pub fn with_journal(agent_id: AgentId, oracle: &'o dyn Oracle, journal: InMemoryJournal) -> Self {
        Self {
            agent_id,
            oracle,
            journal,
        }
    }

    /// Drive the toy agent to completion (or to a simulated crash) in `mode`.
    ///
    /// `crash_in_effect_window`: if set, the durable effect writes its `Intent`,
    /// performs, then the run STOPS without the `Committed` record (simulating a
    /// crash inside the WAL window). The journal is left with a dangling intent.
    pub fn run(&mut self, mode: Mode, crash_in_effect_window: bool) -> RunResult {
        // On replay we re-enter from journal position 0 against the surviving prefix.
        if mode == Mode::Replay {
            self.journal.rewind(&self.agent_id);
        }
        let mut trace: Vec<TraceLabel> = Vec::new();
        let mut crashed = false;
        let mut output = None;

        // Build the agent's state with a freshly-lent Ports each run.
        let mut state = {
            let ports = Ports {
                agent: self.agent_id.clone(),
                mode,
                oracle: self.oracle,
                journal: &mut self.journal,
                trace: &mut trace,
                crash_after_intent: false,
                crash_in_effect_window,
                compensated: false,
                effects_performed: 0,
            };
            ToyState::new(ports)
        };

        // Step the loop to a halt (or until a simulated crash flips the flag).
        loop {
            match drive(&mut state) {
                Step::Continue => {
                    if state.ports.crash_after_intent {
                        crashed = true;
                        break;
                    }
                    if state.ports.compensated {
                        break;
                    }
                }
                Step::Done(out) => {
                    trace_push_done(state.ports.trace, &out);
                    output = Some(out);
                    break;
                }
                Step::Await => {
                    // The toy agent never awaits (no channels in this demo).
                    break;
                }
            }
        }

        let compensated = state.ports.compensated;
        let effects_performed = state.ports.effects_performed;
        RunResult {
            trace,
            output,
            crashed_in_effect: crashed,
            compensated,
            effects_performed,
        }
    }

    /// `no-orphaned-effect` recovery: scan the (post-crash) journal for a dangling
    /// forward `Intent`. For an **idempotent** effect (class I) we re-perform it with
    /// the journaled argument and append the missing `Committed` record with its
    /// result; otherwise we compensate (class C) and append a `Compensating` record.
    /// The toy effect has no external state, so compensation here only records the
    /// rollback. Returns the names that were reconciled.
    pub fn recover_dangling_effects(&mut self, idempotent: bool) -> Vec<String> {
        let dangling = self.journal.dangling_intent_records(&self.agent_id);
        for rec in &dangling {
            let (phase, value) = if idempotent {
                (Phase::Forward, effect_result(&rec.name, &rec.value))
            } else {
                (Phase::Compensating, rec.value.clone())
            };
            self.journal.append(
                &self.agent_id,
                Event::Effect(EffectRecord {
                    name: rec.name.clone(),
                    lifecycle: Lifecycle::Committed,
                    phase,
                    value,
                }),
            );
        }
        dangling.into_iter().map(|r| r.name).collect()
    }

    /// Borrow the journal (for inspection / truncation in tests).
    pub fn journal_mut(&mut self) -> &mut InMemoryJournal {
        &mut self.journal
    }

    /// The agent this runtime drives.
    pub fn agent_id(&self) -> &AgentId {
        &self.agent_id
    }

    pub fn checkpoint(&self) -> Cursor {
        self.journal.checkpoint(&self.agent_id)
    }
}

/// Push the terminal `Done` label onto a trace.
fn trace_push_done(trace: &mut Vec<TraceLabel>, out: &Term) {
    trace.push(TraceLabel::Done { out: out.clone() });
}
