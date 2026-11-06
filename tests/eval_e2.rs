//! E2 — crash recovery at every journal position.
//!
//! For each trial we record a full run, then for EVERY crash point `k` (keep the
//! first `k` journal entries, discard the rest) we resume with a fresh oracle whose
//! seed differs from the recording seed, and check:
//!
//! - **prefix preservation**: the first `k` entries are unchanged, and the resumed
//!   trace reproduces every label whose journal entry survived;
//! - **completion**: the resumed run reaches `Done` (after recovery, if the crash
//!   landed inside an effect's write-ahead window);
//! - **validity**: every oracle value lies in the support of its prompt's
//!   distribution, and cursors stay dense;
//! - **no duplicate effects**: an effect whose `Committed` record survived is never
//!   performed again;
//! - **no orphans**: no dangling intent remains at the end.
//!
//! A second test checks Corollary C1's distributional content: resuming after the
//! prefix `classify = tool` re-samples `tool-arg` from its original distribution.

use agentical::journal::{Event, Lifecycle, Phase};
use agentical::runtime::{InMemoryJournal, Mode, Runtime, SeededOracle, TraceLabel};
use agentical::types::AgentId;

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
    AgentId("e2".to_string())
}

fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn in_support(term: &str) -> bool {
    dist_spec()
        .iter()
        .any(|(_, cs)| cs.iter().any(|(o, _)| *o == term))
}

/// Copy the first `k` entries of a journal into a fresh journal (a crash that kept
/// only the durable prefix).
fn crashed_copy(src: &InMemoryJournal, a: &AgentId, k: usize) -> InMemoryJournal {
    use agentical::journal::Journal;
    let mut j = InMemoryJournal::new();
    for e in &src.entries(a)[..k] {
        j.append(a, e.event.clone());
    }
    j
}

#[test]
fn e2_crash_at_every_point() {
    const TRIALS: usize = 1000;
    let a = agent();
    let mut rng = 0x0000_0000_0000_00E2u64;
    let (mut resumes, mut completed, mut in_window, mut prefix_ok) = (0usize, 0, 0, 0);
    let mut dup_effects = 0usize;

    for _ in 0..TRIALS {
        let rec_seed = splitmix64(&mut rng);
        let rec_oracle = oracle_with_seed(rec_seed);
        let mut rt = Runtime::new(a.clone(), &rec_oracle);
        let full = rt.run(Mode::Record, false);
        assert!(full.output.is_some());
        let n = rt.journal.len(&a) as usize;

        for k in 0..=n {
            resumes += 1;
            let j = crashed_copy(&rt.journal, &a, k);
            let committed_survived = j.entries(&a).iter().any(|e| {
                matches!(&e.event, Event::Effect(r)
                    if r.lifecycle == Lifecycle::Committed && r.phase == Phase::Forward)
            });

            let rep_seed = splitmix64(&mut rng);
            assert_ne!(rep_seed, rec_seed);
            let rep_oracle = oracle_with_seed(rep_seed);
            let mut rt2 = Runtime::with_journal(a.clone(), &rep_oracle, j);
            let mut resumed = rt2.run(Mode::Replay, false);
            let mut performed = resumed.effects_performed;

            if resumed.crashed_in_effect {
                // The crash point split an effect's Intent from its Committed record.
                in_window += 1;
                assert_eq!(rt2.journal.dangling_intents(&a), vec!["commit".to_string()]);
                rt2.recover_dangling_effects(/*idempotent=*/ true);
                resumed = rt2.run(Mode::Replay, false);
                performed += resumed.effects_performed;
            }

            // Completion and no orphans.
            if resumed.output.is_some()
                && matches!(resumed.trace.last(), Some(TraceLabel::Done { .. }))
                && rt2.journal.dangling_intents(&a).is_empty()
            {
                completed += 1;
            }

            // Prefix preservation: surviving entries unchanged ...
            let orig = rt.journal.entries(&a);
            let now = rt2.journal.entries(&a);
            let same_prefix = (0..k).all(|i| format!("{:?}", orig[i].event) == format!("{:?}", now[i].event));
            // ... and every trace label whose entry survived is reproduced.
            let labels_ok = full
                .trace
                .iter()
                .zip(resumed.trace.iter())
                .take_while(|(l, _)| match l {
                    TraceLabel::Oracle { key, .. } | TraceLabel::Effect { key, .. } => (*key as usize) < k,
                    TraceLabel::Done { .. } => false,
                })
                .all(|(x, y)| format!("{x:?}") == format!("{y:?}"));
            if same_prefix && labels_ok {
                prefix_ok += 1;
            }

            // Validity: oracle values in support, cursors dense.
            for (i, e) in now.iter().enumerate() {
                assert_eq!(e.at.0 as usize, i);
                if let Event::OracleDraw(s) = &e.event {
                    assert!(in_support(&s.term.0), "value {:?} outside the support", s.term);
                }
            }

            // No duplicate effects: a surviving Committed record means the effect
            // must not run again.
            if committed_survived && performed > 0 {
                dup_effects += 1;
            }
        }
    }

    println!(
        "\nE2 crash-at-every-point sweep\n\
         -----------------------------\n\
         trials                 : {TRIALS}\n\
         resumes (all k)        : {resumes}\n\
         completed              : {completed}/{resumes}\n\
         prefix reproduced      : {prefix_ok}/{resumes}\n\
         crashes inside WAL gap : {in_window}\n\
         duplicate effects      : {dup_effects}\n",
    );
    assert_eq!(completed, resumes);
    assert_eq!(prefix_ok, resumes);
    assert_eq!(dup_effects, 0);
}

#[test]
fn e2_resume_resamples_from_the_conditional_law() {
    // Prefix: classify = tool (one journal entry). Resume many times with fresh seeds;
    // the re-sampled tool-arg must follow its original distribution (arg-A w.p. 3/5).
    const N: usize = 20_000;
    let a = agent();
    let mut rng = 0x0000_0000_0000_0C01u64;

    // Find a recording whose first draw is `tool`.
    let mut base = None;
    while base.is_none() {
        let seed = splitmix64(&mut rng);
        let o = oracle_with_seed(seed);
        let mut rt = Runtime::new(a.clone(), &o);
        let r = rt.run(Mode::Record, false);
        if matches!(r.trace.first(), Some(TraceLabel::Oracle { term, .. }) if term.0 == "tool") {
            base = Some(crashed_copy(&rt.journal, &a, 1));
        }
    }
    let base = base.unwrap();

    let mut arg_a = 0usize;
    for _ in 0..N {
        let o = oracle_with_seed(splitmix64(&mut rng));
        let mut rt = Runtime::with_journal(a.clone(), &o, crashed_copy(&base, &a, 1));
        let r = rt.run(Mode::Replay, false);
        assert!(r.output.is_some());
        let arg = r.trace.iter().find_map(|l| match l {
            TraceLabel::Oracle { key: 1, term } => Some(term.0.clone()),
            _ => None,
        });
        if arg.as_deref() == Some("arg-A") {
            arg_a += 1;
        }
    }
    let freq = arg_a as f64 / N as f64;
    println!("\nE2 conditional law: P[arg-A | classify=tool] = {freq:.4} (expected 0.6000, N = {N})\n");
    assert!((freq - 0.6).abs() < 0.02, "empirical frequency {freq} far from 0.6");
}

#[test]
fn compensated_effect_replays_as_compensated_and_is_idempotent() {
    // Corollary C3 for compensation: after a crash inside the effect window and a
    // compensating recovery, replaying (twice) stops at the compensated effect, emits
    // no output, performs nothing, and appends nothing.
    let a = agent();
    let mut rng = 0x0000_0000_0000_0C03u64;
    loop {
        let o = oracle_with_seed(splitmix64(&mut rng));
        let mut rt = Runtime::new(a.clone(), &o);
        let crashed = rt.run(Mode::Record, true);
        if !crashed.crashed_in_effect {
            continue; // this seed took the answer route; try another
        }
        rt.recover_dangling_effects(/*idempotent=*/ false);
        let len_before = rt.journal.len(&a);

        let o2 = oracle_with_seed(splitmix64(&mut rng));
        let mut rt2 = Runtime::with_journal(a.clone(), &o2, rt.journal);
        let first = rt2.run(Mode::Replay, false);
        let second = rt2.run(Mode::Replay, false);
        for r in [&first, &second] {
            assert!(r.compensated);
            assert!(r.output.is_none());
            assert_eq!(r.effects_performed, 0);
        }
        assert_eq!(format!("{:?}", first.trace), format!("{:?}", second.trace));
        assert_eq!(rt2.journal.len(&a), len_before, "replay must not append");
        break;
    }
}
