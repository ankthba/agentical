//! Primitive #1 — **Agent**: the unit of composition and of failure.
//!
//! Calculus: a located, named process `⟨id, Σ, B⟩` (identity, private state `Σ`,
//! behaviour `B`). Agents compose by parallel composition and by delegation
//! (spawn a sub-agent, await its result over a [`crate::channel::Channel`]).

/// The decision produced by one reasoning step of an [`Agent`]. The runtime drives
/// `step` in a loop; each variant corresponds to a reduction in the calculus.
#[derive(Debug)]
pub enum Step<Out> {
    /// Pure local progress — keep stepping.
    Continue,
    /// Emit a final result and halt.
    Done(Out),
    /// Suspend until an external event (a channel receive, a sub-agent result).
    /// In the calculus this is the point where adversarial scheduling resolves.
    Await,
}

/// A process and the granularity at which **failure is contained** and at which a
/// probabilistic **contract** (`with probability ≥ p, the output satisfies φ`) is stated.
///
/// `step` is where the runtime hands the agent access (via a future `ctx` handle) to
/// the other primitives: draw an [`crate::oracle::Oracle`], talk over a
/// [`crate::channel::Channel`], touch [`crate::memory::Memory`]. Each effect is
/// recorded in the [`crate::journal::Journal`] so the agent is replayable.
pub trait Agent {
    /// Private state `Σ`.
    type State;
    /// The agent's final result type.
    type Out;

    /// One reduction of the behaviour `B`.
    fn step(&mut self, state: &mut Self::State) -> Step<Self::Out>;
}
