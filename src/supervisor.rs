//! Primitive #6 — **Supervisor**: failure propagation, compensation, escalation.
//!
//! A supervision relation over agents (Erlang/actor-style trees). When a child
//! faults, the supervisor chooses to **restart** (replay from the journal),
//! **compensate** (run saga-style inverse effects for durable actions already
//! taken), or **escalate**. Guarantees every fault resolves — no agent left in a
//! partial durable state (the *no-orphaned-effect* property, provable against the
//! [`crate::journal::Journal`]).

use crate::types::AgentId;

/// A fault that can arise mid-reduction.
#[derive(Clone, Debug)]
pub enum Fault {
    /// The Oracle (LLM/tool sampling) errored.
    OracleError(String),
    /// A tool/effect failed.
    ToolError(String),
    /// A step exceeded its deadline.
    Timeout,
    /// Cost/step budget exhausted — turns a runaway reasoning loop into a
    /// well-typed fault rather than an unbounded burn.
    BudgetExhausted,
    /// A stated contract `φ` was violated.
    ContractViolation(String),
}

/// An ordered list of compensating actions (a saga) for already-committed durable
/// effects, run in reverse. Placeholder shape; each step is a compensating agent
/// action in the full design.
#[derive(Default)]
pub struct Saga {
    pub steps: Vec<String>,
}

/// What a supervisor decides when a child faults.
pub enum Decision {
    /// Replay the child from its [`crate::journal::Journal`].
    Restart,
    /// Run inverse effects for durable actions already taken.
    Compensate(Saga),
    /// Propagate the fault upward.
    Escalate,
    /// Stop the child.
    Stop,
}

/// A cost/step budget capability; exhaustion surfaces as [`Fault::BudgetExhausted`].
#[derive(Clone, Copy, Debug)]
pub struct Budget {
    pub max_steps: u64,
    pub max_cost_millis: u64,
}

/// The supervision relation: given a faulting child, decide how to recover.
pub trait Supervisor {
    fn on_fault(&self, child: &AgentId, fault: Fault) -> Decision;
}
