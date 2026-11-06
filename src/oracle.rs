//! Primitive #2 — **Oracle**: the probabilistic call. *Novel core (1 of 2).*
//!
//! An LLM call (or any stochastic action) is modeled as a *draw* `x ~ μ_e` from a
//! measurable distribution, **not** a function `f(e)`. This is the primitive that
//! LLMbda/FIDES collapse to a deterministic function and that the MSC calculus
//! treats as an opaque black box. Making it a distribution is what lets the paper
//! state quantitative properties (in expectation / with probability ≥ p).

use crate::types::Term;

/// A prompt / conversation prefix handed to an [`Oracle`].
#[derive(Clone, Debug)]
pub struct Prompt(pub String);

/// The realized result of one Oracle draw, plus what the runtime needs to journal
/// it for sound replay (see [`crate::journal`]).
#[derive(Clone, Debug)]
pub struct Sampled {
    /// The drawn result term.
    pub term: Term,
    /// Provenance journaled alongside the draw (model id, seed, logprobs, latency).
    /// Opaque bytes for now; the runtime crate gives it structure.
    pub provenance: Vec<u8>,
}

/// An **abstract** probability measure hint over result terms. Proofs are either
/// distribution-agnostic (hold for all `Dist`) or *parametric* in this (hold given
/// assumptions such as a temperature/top-p setting or a calibration/tail bound).
///
/// Deliberately opaque: we never need the LLM's true token distribution, only
/// assumptions on it. This keeps the proofs tractable and honest.
#[derive(Clone, Debug, Default)]
pub struct Dist;

/// A stochastic step with an associated (possibly unknown) probability measure.
pub trait Oracle {
    /// Draw one sample `x ~ μ_prompt`.
    ///
    /// In *record* mode the runtime journals the outcome; in *replay* mode (see
    /// [`crate::journal`]) the journaled outcome is returned **instead of**
    /// re-sampling — the move that makes durability sound under nondeterminism.
    fn sample(&self, prompt: Prompt) -> Sampled;

    /// Optional assumptions on the underlying measure that proofs may be parametric
    /// in. `None` ⇒ reason distribution-agnostically.
    fn measure_hint(&self) -> Option<Dist> {
        None
    }
}
