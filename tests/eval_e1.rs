//! **E1 — Replay-determinism sweep** (empirical correlate of Theorem T1a).
//!
//! For each of `N` trials we (1) record a run under a random oracle seed, then
//! (2) replay it against its own journal under an *independent, different* oracle
//! seed, and check the observable trace and final output are byte-identical.
//!
//! T1a predicts a 100% match rate: replay is Dirac because every journaled draw
//! is consumed from `J*`, so the replay RNG is irrelevant. Any divergence would
//! be a soundness failure, not a flake. The per-trial seeds come from a fixed
//! SplitMix64 stream, so the whole sweep is reproducible.
//!
//! Run with output:  `cargo test --test eval_e1 -- --nocapture`

use agentical::runtime::{InMemoryJournal, Mode, Runtime, SeededOracle, TraceLabel};
use agentical::types::AgentId;

/// Prompt-indexed distributions `μ_p` (same toy spec as `tests/replay.rs`): the
/// `classify` route is weighted toward `tool`, so a typical seed exercises the
/// durable-effect branch. Every draw is RNG-determined, hence genuinely stochastic.
fn dist_spec() -> Vec<(&'static str, Vec<(&'static str, f64)>)> {
    vec![
        ("classify", vec![("tool", 5.0), ("answer", 1.0)]),
        ("tool-arg", vec![("arg-A", 3.0), ("arg-B", 2.0)]),
        ("answer", vec![("direct-answer", 1.0)]),
    ]
}

fn oracle_with_seed(seed: u64) -> SeededOracle {
    let spec = dist_spec();
    let borrowed: Vec<(&str, Vec<(&str, f64)>)> = spec
        .iter()
        .map(|(p, cs)| (*p, cs.iter().map(|(o, w)| (*o, *w)).collect()))
        .collect();
    let refs: Vec<(&str, &[(&str, f64)])> =
        borrowed.iter().map(|(p, cs)| (*p, cs.as_slice())).collect();
    SeededOracle::new(seed, &refs)
}

fn agent() -> AgentId {
    AgentId("e1".to_string())
}

fn fmt_trace(trace: &[TraceLabel]) -> Vec<String> {
    trace.iter().map(|l| format!("{l:?}")).collect()
}

/// Reproducible 64-bit seed stream (so the sweep is deterministic run-to-run).
fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

#[test]
fn e1_replay_determinism_sweep() {
    const N: usize = 1000;
    let mut rng = 0x0000_0000_0000_00E1u64; // fixed start => reproducible sweep
    let mut matched = 0usize;
    let mut with_oracle = 0usize; // trials that exercised >=1 oracle draw
    let mut tool_path = 0usize; // trials that hit the durable-effect branch
    let mut distinct_witness = 0usize; // record & replay seeds differed

    for _ in 0..N {
        let rec_seed = splitmix64(&mut rng);
        let rep_seed = splitmix64(&mut rng); // independent draw
        if rec_seed != rep_seed {
            distinct_witness += 1;
        }

        // (1) RECORD under rec_seed.
        let rec_oracle = oracle_with_seed(rec_seed);
        let mut rt = Runtime::new(agent(), &rec_oracle);
        let recorded = rt.run(Mode::Record, /*crash_in_effect_window=*/ false);
        let journal: InMemoryJournal = rt.journal;

        // (2) REPLAY the journal under a DIFFERENT seed: equality can only come
        //     from J*, never from RNG luck.
        let rep_oracle = oracle_with_seed(rep_seed);
        let mut rt2 = Runtime::with_journal(agent(), &rep_oracle, journal);
        let replayed = rt2.run(Mode::Replay, false);

        if fmt_trace(&recorded.trace) == fmt_trace(&replayed.trace)
            && recorded.output == replayed.output
        {
            matched += 1;
        }
        if recorded
            .trace
            .iter()
            .any(|l| matches!(l, TraceLabel::Oracle { .. }))
        {
            with_oracle += 1;
        }
        if recorded.crashed_in_effect || rt2_effect_committed(&replayed.trace) {
            tool_path += 1;
        }
    }

    let rate = matched as f64 / N as f64;
    println!(
        "\nE1 replay-determinism sweep\n\
         ---------------------------\n\
         trials                : {N}\n\
         trace+output matches  : {matched}/{N}  (rate = {rate:.4})\n\
         exercised oracle draw : {with_oracle}/{N}\n\
         hit durable-effect    : {tool_path}/{N}\n\
         record!=replay seed   : {distinct_witness}/{N}\n",
    );

    assert_eq!(
        with_oracle, N,
        "every trial must exercise at least one stochastic oracle draw, else the test is vacuous"
    );
    assert_eq!(
        distinct_witness, N,
        "record and replay seeds must differ so equality cannot be RNG coincidence"
    );
    assert_eq!(
        matched, N,
        "T1a predicts byte-identical replay for EVERY trial; a miss is a soundness failure"
    );
}

/// True if the trace shows a committed durable effect (the tool branch).
fn rt2_effect_committed(trace: &[TraceLabel]) -> bool {
    trace
        .iter()
        .any(|l| format!("{l:?}").contains("Effect"))
}
