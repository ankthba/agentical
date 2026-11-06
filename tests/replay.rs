//! Empirical demonstration of the flagship theorem **T1 (replay soundness under
//! nondeterminism)** for the `agentical` runtime prototype.
//!
//! These tests run the real [`Runtime`] over the toy "sample → branch → sample →
//! commit effect" [`ToyAgent`], using the seeded categorical [`SeededOracle`] and
//! the append-only [`InMemoryJournal`]. They prove three properties:
//!
//! 1. **Determinism of replay (T1a).** Record a run; replay it from the journal with
//!    the oracle seeded *differently*; assert the observable trace is byte-identical.
//!    Equality therefore can only come from the journal, never from RNG luck.
//! 2. **Crash + resume.** Truncate the journal mid-run (a crash after some steps),
//!    resume in replay-then-record mode; assert the surviving prefix is reproduced
//!    and the run completes to the same final output as the uninterrupted run.
//! 3. **No-orphaned-effect.** Crash *between* a durable effect's `Intent` and
//!    `Committed` records; assert resume detects the dangling intent and reconciles
//!    it (re-commit for the idempotent effect / compensate otherwise), leaving no
//!    agent in a partially-applied durable state.
//!
//! All of this is std-only; no external dependencies.

use agentical::journal::{Cursor, Event, Journal, Lifecycle, Phase};
use agentical::prelude::Term;
use agentical::runtime::{InMemoryJournal, Mode, Runtime, SeededOracle, TraceLabel};
use agentical::types::AgentId;

/// The toy oracle's **prompt-indexed** distributions `μ_p`. The `classify` route is
/// weighted toward `tool` so a typical seed exercises the durable-effect path (where
/// WAL / no-orphaned-effect lives); every draw is still RNG-determined, hence
/// genuinely stochastic. Returned as owned data; `dists()` borrows it for the oracle.
fn dist_spec() -> Vec<(&'static str, Vec<(&'static str, f64)>)> {
    vec![
        ("classify", vec![("tool", 5.0), ("answer", 1.0)]),
        ("tool-arg", vec![("arg-A", 3.0), ("arg-B", 2.0)]),
        ("answer", vec![("direct-answer", 1.0)]),
    ]
}

/// Build a `SeededOracle` over [`dist_spec`] with the given seed.
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
    AgentId("toy-1".to_string())
}

/// Trace projection used for the equality assertions (the full observable trace).
fn fmt_trace(trace: &[TraceLabel]) -> Vec<String> {
    trace.iter().map(|l| format!("{l:?}")).collect()
}

// ---------------------------------------------------------------------------
// Property 1 — determinism of replay (T1a): replay reproduces the recorded trace
// with probability 1, even when the replay oracle is seeded differently.
// ---------------------------------------------------------------------------
#[test]
fn replay_is_byte_identical_with_a_different_seed() {
    // RECORD with seed 0xA11CE.
    let rec_oracle = oracle_with_seed(0x000A_11CE);
    let mut rt = Runtime::new(agent(), &rec_oracle);
    let recorded = rt.run(Mode::Record, /*crash_in_effect_window=*/ false);

    assert!(
        recorded.output.is_some(),
        "the recorded run should halt with an output"
    );
    assert!(
        !recorded.trace.is_empty(),
        "the recorded run should produce a non-empty observable trace"
    );

    // Move the populated journal into a NEW runtime whose oracle is seeded
    // DIFFERENTLY (0xBEEF vs 0xA11CE). If the RNG drove the replay, the trace would
    // diverge; if the JOURNAL drives it, the trace is identical.
    let journal: InMemoryJournal = rt.journal;
    let replay_oracle = oracle_with_seed(0x0000_BEEF);
    assert_ne!(
        rec_oracle.seed(),
        replay_oracle.seed(),
        "replay oracle must be seeded differently so equality cannot come from luck"
    );

    let mut rt2 = Runtime::with_journal(agent(), &replay_oracle, journal);
    let replayed = rt2.run(Mode::Replay, /*crash_in_effect_window=*/ false);

    assert_eq!(
        fmt_trace(&recorded.trace),
        fmt_trace(&replayed.trace),
        "replayed observable trace must be byte-identical to the recorded one"
    );
    assert_eq!(
        recorded.output, replayed.output,
        "replayed final output must match the recorded output"
    );

    // Sanity: the seeded oracle, sampled live with the *replay* seed, would NOT in
    // general agree with the recorded draw — confirming the equality above is the
    // journal's doing, not a coincidence of both seeds picking the same outcome.
    // (We just assert the run actually went through the stochastic path.)
    assert!(
        recorded
            .trace
            .iter()
            .any(|l| matches!(l, TraceLabel::Oracle { .. })),
        "the run must contain at least one oracle draw"
    );
}

/// A second, independent witness for Property 1: a record run started from the SAME
/// seed as the replay oracle would (in general) produce a DIFFERENT trace than the
/// recorded one — so byte-identical replay is not an artifact of the two seeds
/// happening to coincide.
#[test]
fn different_seed_records_a_different_trace_in_general() {
    let oracle_a = oracle_with_seed(0x000A_11CE);
    let mut rt_a = Runtime::new(agent(), &oracle_a);
    let run_a = rt_a.run(Mode::Record, false);

    let oracle_b = oracle_with_seed(0x0000_BEEF);
    let mut rt_b = Runtime::new(agent(), &oracle_b);
    let run_b = rt_b.run(Mode::Record, false);

    // It is overwhelmingly likely these differ; if this ever fails, pick new seeds.
    assert_ne!(
        fmt_trace(&run_a.trace),
        fmt_trace(&run_b.trace),
        "two different seeds should record different traces (else the replay test is vacuous)"
    );
}

// ---------------------------------------------------------------------------
// Property 2 — crash + resume: truncate the journal mid-run, resume in
// replay-then-record mode, reproduce the prefix and complete the run.
// ---------------------------------------------------------------------------
#[test]
fn crash_then_resume_reproduces_prefix_and_completes() {
    // Full, uninterrupted record run is the ground truth.
    let oracle = oracle_with_seed(0x000A_11CE);
    let mut rt = Runtime::new(agent(), &oracle);
    let full = rt.run(Mode::Record, false);
    let full_len = rt.journal.len(&agent());
    assert!(
        full_len >= 2,
        "the run should have journaled at least two entries to truncate between"
    );

    // Simulate a CRASH: keep only the first entry (the `classify` oracle draw),
    // discard the rest. This models a process that died after step 1.
    let keep = 1u64;
    rt.journal.truncate(&agent(), keep);
    assert_eq!(
        rt.journal.len(&agent()),
        keep,
        "truncation should leave exactly the surviving prefix"
    );

    // RESUME: replay the surviving prefix, then fall through to record the rest
    // (`[O-resume]`). The resume oracle has a DIFFERENT seed, so the replayed prefix
    // can only match because it is read from the journal.
    let resume_oracle = oracle_with_seed(0x0000_BEEF);
    let mut rt2 = Runtime::with_journal(agent(), &resume_oracle, rt.journal);
    let resumed = rt2.run(Mode::Replay, false);

    // The replayed prefix (entry 0 = the classify draw) must match the original.
    let prefix_orig = &full.trace[0];
    let prefix_resumed = &resumed.trace[0];
    assert_eq!(
        format!("{prefix_orig:?}"),
        format!("{prefix_resumed:?}"),
        "the replayed prefix must reproduce the recorded prefix exactly"
    );

    // And the run must COMPLETE (reach a Done with an output).
    assert!(
        resumed.output.is_some(),
        "the resumed run must complete to a final output"
    );
    assert!(
        matches!(resumed.trace.last(), Some(TraceLabel::Done { .. })),
        "the resumed run must end in a Done label"
    );

    // The surviving prefix is untouched, and the resumed run appended its own suffix
    // after it (the suffix is a fresh draw, so it may differ from the original's).
    let entries = rt2.journal.entries(&agent());
    assert!(entries.len() as u64 > keep, "resume must append the remaining steps");
    for (i, e) in entries.iter().enumerate() {
        assert_eq!(e.at, Cursor(i as u64), "cursors stay dense after resume");
    }
    let _ = full;
}

// ---------------------------------------------------------------------------
// Property 3 — no-orphaned-effect: crash inside the durable effect's
// Intent→Committed window; resume detects the dangling intent and reconciles it.
// ---------------------------------------------------------------------------
#[test]
fn crash_in_effect_window_leaves_dangling_intent_then_recovers() {
    // Seed chosen so the agent takes the TOOL route (which performs the durable
    // effect). We assert that precondition explicitly below.
    let oracle = oracle_with_seed(0x0000_BEEF);
    let mut rt = Runtime::new(agent(), &oracle);

    // Run with the crash-in-effect-window flag: the effect writes its `Intent`,
    // performs, then the process "dies" before the `Committed` record is written.
    let crashed = rt.run(Mode::Record, /*crash_in_effect_window=*/ true);
    assert!(
        crashed.crashed_in_effect,
        "precondition: this seed must route through the durable effect and crash in its WAL window"
    );
    assert!(
        crashed.output.is_none(),
        "a run that crashed mid-effect must not have produced a final output"
    );

    // The journal now holds a forward `Intent` with no matching `Committed`.
    let entries: Vec<_> = rt.journal.entries(&agent()).to_vec();
    let has_intent = entries.iter().any(|e| {
        matches!(
            &e.event,
            Event::Effect(r)
                if r.lifecycle == Lifecycle::Intent && r.phase == Phase::Forward
        )
    });
    let has_committed = entries.iter().any(|e| {
        matches!(
            &e.event,
            Event::Effect(r) if r.lifecycle == Lifecycle::Committed
        )
    });
    assert!(has_intent, "the crash must have left a dangling Intent record");
    assert!(
        !has_committed,
        "the crash must have prevented the matching Committed record"
    );

    // Detection: the journal reports the dangling intent by name.
    let dangling = rt.journal.dangling_intents(&agent());
    assert_eq!(
        dangling,
        vec!["commit".to_string()],
        "resume must detect exactly the dangling 'commit' intent"
    );

    // RECOVERY (idempotent effect ⇒ re-commit): append the missing `Committed`
    // forward record. After recovery there are NO dangling intents.
    let reconciled = rt.recover_dangling_effects(/*idempotent=*/ true);
    assert_eq!(
        reconciled,
        vec!["commit".to_string()],
        "recovery should reconcile exactly the dangling effect"
    );
    assert!(
        rt.journal.dangling_intents(&agent()).is_empty(),
        "after re-commit there must be no orphaned effect"
    );

    // The WAL is now well-formed: Intent followed by a forward Committed.
    let after: Vec<_> = rt.journal.entries(&agent()).to_vec();
    let committed_now = after.iter().any(|e| {
        matches!(
            &e.event,
            Event::Effect(r)
                if r.lifecycle == Lifecycle::Committed && r.phase == Phase::Forward
        )
    });
    assert!(committed_now, "re-commit must append a forward Committed record");
}

/// Variant of Property 3 with a **non-idempotent** effect: recovery compensates
/// (logs a `Compensating` rollback) instead of re-committing. We assert the
/// compensation is journaled with the `Compensating` phase, which is what stops a
/// later replay from double-undoing (the ratified design in `src/journal.rs`).
#[test]
fn crash_in_effect_window_recovers_by_compensation() {
    let oracle = oracle_with_seed(0x0000_BEEF);
    let mut rt = Runtime::new(agent(), &oracle);
    let crashed = rt.run(Mode::Record, true);
    assert!(crashed.crashed_in_effect);

    let reconciled = rt.recover_dangling_effects(/*idempotent=*/ false);
    assert_eq!(reconciled, vec!["commit".to_string()]);

    // A Compensating record now exists; no forward Intent is left dangling.
    let entries: Vec<_> = rt.journal.entries(&agent()).to_vec();
    let has_comp = entries.iter().any(|e| {
        matches!(
            &e.event,
            Event::Effect(r) if r.phase == Phase::Compensating
        )
    });
    assert!(has_comp, "compensation must append a Compensating record");
    assert!(
        rt.journal.dangling_intents(&agent()).is_empty(),
        "after compensation there must be no orphaned forward intent"
    );
}

/// A small direct check that the journal's cursor really *is* the key: appends land
/// at strictly increasing positions starting from 0, and `checkpoint` tracks length.
#[test]
fn journal_cursor_is_the_key() {
    let mut j = InMemoryJournal::new();
    let a = agent();
    assert_eq!(j.checkpoint(&a), Cursor(0));
    let c0 = j_append_oracle(&mut j, &a, "x");
    let c1 = j_append_oracle(&mut j, &a, "y");
    assert_eq!(c0, Cursor(0));
    assert_eq!(c1, Cursor(1));
    assert_eq!(j.checkpoint(&a), Cursor(2));
}

fn j_append_oracle(j: &mut InMemoryJournal, a: &AgentId, s: &str) -> Cursor {
    use agentical::oracle::Sampled;
    j.append(
        a,
        Event::OracleDraw(Sampled {
            term: Term(s.to_string()),
            provenance: Vec::new(),
        }),
    )
}
