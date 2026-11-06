//! `SeededOracle` — a concrete [`Oracle`] for record/replay.
//!
//! This is the prototype stand-in for an LLM call. It is **prompt-indexed**: for
//! each prompt `p` it carries a small **categorical distribution** `μ_p` over
//! candidate result terms (`(outcome, weight)` pairs), plus one seeded
//! [`SplitMix64`] RNG shared across draws. In *record* mode
//! [`sample`](Oracle::sample) genuinely draws `x ~ μ_prompt` and advances the RNG;
//! the runtime journals the outcome. In *replay* mode the runtime reads the
//! journaled outcome **instead of** calling the oracle, which is why we can seed the
//! replay oracle *differently* and still reproduce the recorded trace — the equality
//! can only come from the journal.
//!
//! Indexing the distribution by the prompt mirrors the calculus exactly: `oracle(e)`
//! denotes the measure `μ_p` where `e ⇓ p` (paper, Section 4). A prompt with no
//! registered distribution falls back to a single fixed outcome.
//!
//! The provenance bytes journaled alongside each draw record the seed and the RNG
//! word consumed — the "model id / seed / logprobs" metadata a real backend would
//! persist (provenance is not observable but is kept).

use std::cell::RefCell;
use std::collections::HashMap;

use crate::oracle::{Dist, Oracle, Prompt, Sampled};
use crate::runtime::rng::SplitMix64;
use crate::types::Term;

// DESIGN-NOTE: `Oracle::sample(&self, ...)` takes `&self`, but a real sampler must
// advance RNG state, which is `&mut`. We keep the RNG behind a `RefCell`. Feedback
// for formal-core: either `sample(&mut self, ...)` or thread the RNG state through
// the runtime explicitly. `&self` reads as "pure" but sampling is by definition
// stateful, so the signature is slightly misleading.

/// A categorical distribution `μ_p`: outcomes with positive weights.
#[derive(Clone)]
struct Categorical {
    outcomes: Vec<String>,
    weights: Vec<f64>,
}

/// A seeded, **prompt-indexed** categorical [`Oracle`].
pub struct SeededOracle {
    /// `prompt ⇒ μ_prompt`.
    dists: HashMap<String, Categorical>,
    seed: u64,
    rng: RefCell<SplitMix64>,
}

impl SeededOracle {
    /// Build an oracle from `(prompt, &[(outcome, weight)])` distributions.
    ///
    /// Example:
    /// ```ignore
    /// SeededOracle::new(0xA11CE, &[
    ///     ("classify",  &[("tool", 5.0), ("answer", 1.0)]),
    ///     ("tool-arg",  &[("arg-A", 3.0), ("arg-B", 2.0)]),
    ///     ("answer",    &[("direct-answer", 1.0)]),
    /// ]);
    /// ```
    pub fn new(seed: u64, dists: &[(&str, &[(&str, f64)])]) -> Self {
        let mut map = HashMap::new();
        for (prompt, choices) in dists {
            let outcomes = choices.iter().map(|(o, _)| o.to_string()).collect();
            let weights = choices.iter().map(|(_, w)| *w).collect();
            map.insert(
                (*prompt).to_string(),
                Categorical { outcomes, weights },
            );
        }
        Self {
            dists: map,
            seed,
            rng: RefCell::new(SplitMix64::new(seed)),
        }
    }

    /// The seed this oracle was constructed with (recorded into provenance).
    pub fn seed(&self) -> u64 {
        self.seed
    }
}

impl Oracle for SeededOracle {
    fn sample(&self, prompt: Prompt) -> Sampled {
        let mut rng = self.rng.borrow_mut();
        // Consume exactly one RNG word per draw; keep it for provenance.
        let word = rng.next_u64();

        let term = match self.dists.get(&prompt.0) {
            Some(cat) => {
                // Re-derive the categorical pick from `word` so the draw is a pure
                // function of the consumed word (provenance ⇔ outcome stay consistent).
                let total: f64 = cat.weights.iter().sum();
                let unit = ((word >> 11) as f64) * (1.0 / ((1u64 << 53) as f64));
                let mut target = unit * total;
                let mut idx = cat.weights.len() - 1;
                for (i, w) in cat.weights.iter().enumerate() {
                    target -= *w;
                    if target < 0.0 {
                        idx = i;
                        break;
                    }
                }
                Term(cat.outcomes[idx].clone())
            }
            // Unknown prompt ⇒ a fixed, deterministic fallback outcome.
            None => Term(format!("<no-dist:{}>", prompt.0)),
        };

        // Provenance: seed (8 bytes) ++ the consumed RNG word (8 bytes).
        let mut provenance = Vec::with_capacity(16);
        provenance.extend_from_slice(&self.seed.to_le_bytes());
        provenance.extend_from_slice(&word.to_le_bytes());
        Sampled { term, provenance }
    }

    fn measure_hint(&self) -> Option<Dist> {
        // We *do* know each μ_p here, but `Dist` is an opaque hint in the scaffold;
        // we surface its presence and let proofs stay distribution-agnostic.
        Some(Dist)
    }
}
