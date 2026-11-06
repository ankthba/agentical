//! # agentical
//!
//! A research prototype of provably sound record, replay, and crash recovery for
//! nondeterministic LLM agents. The [`runtime`] module implements **single-agent**
//! record, replay, crash resume, and write-ahead-logged durable effects on top of
//! [`journal::Journal`] and [`oracle::Oracle`]. `Channel`, `Memory`, and `Supervisor`
//! are interfaces for future multi-agent work, with no implementation.
//! See `paper/` for the paper.
//!
//! | primitive  | this crate                  | role                              |
//! |------------|-----------------------------|-----------------------------------|
//! | Agent      | [`agent::Agent`]            | unit of composition & failure     |
//! | Oracle     | [`oracle::Oracle`]          | probabilistic call (**novel**)    |
//! | Channel    | [`channel::Channel`]        | typed communication & delegation  |
//! | Memory     | [`memory::Memory`]          | scoped, IFC-labeled state         |
//! | Journal    | [`journal::Journal`]        | durable history & replay (**novel**) |
//! | Supervisor | [`supervisor::Supervisor`]  | failure propagation               |
//!
//! ## Two kinds of nondeterminism, kept strictly separate
//! - *probabilistic* (has a measure): funneled entirely through [`oracle`].
//! - *adversarial* (scheduler/ordering, demonic): lives entirely in [`channel`].
//!
//! Method signatures are synchronous to keep the traits executor-agnostic; an async
//! runtime layer can wrap them.

// Design scaffold: variants/fields exist to define the model before it has callers.
#![allow(dead_code)]

pub mod agent;
pub mod channel;
pub mod journal;
pub mod memory;
pub mod oracle;
pub mod runtime;
pub mod supervisor;
pub mod types;

/// Everything you need to talk about the model in one import.
pub mod prelude {
    pub use crate::agent::{Agent, Step};
    pub use crate::channel::Channel;
    pub use crate::journal::{Event, Journal, Outcome};
    pub use crate::memory::{Label, Labeled, Memory, Retention, Scope};
    pub use crate::oracle::{Dist, Oracle, Prompt, Sampled};
    pub use crate::supervisor::{Decision, Fault, Supervisor};
    pub use crate::types::{AgentId, Term};
}
