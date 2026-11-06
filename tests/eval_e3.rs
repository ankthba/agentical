//! **E3 — Journaling overhead.**
//!
//! * `e3_journaling_overhead` — H3a (per-step append latency) + H3d (size $\Theta(n)$).
//! * `e3_replay_speedup` — H3c: with a *latency-injected* oracle (modelling an LLM
//!   network call), replay is dramatically faster than re-execution because replay
//!   performs **zero** oracle calls (it reads the journal). This is the systems-level
//!   payoff of T1, measured without a real model.
//!
//! Run with:  `cargo test --release --test eval_e3 -- --nocapture`

use agentical::journal::{Event, Journal};
use agentical::oracle::{Oracle, Prompt, Sampled};
use agentical::prelude::Term;
use agentical::runtime::{InMemoryJournal, Mode, Runtime, SeededOracle};
use agentical::types::AgentId;
use std::time::{Duration, Instant};

fn agent() -> AgentId {
    AgentId("e3".to_string())
}

fn mk_event(i: usize) -> Event {
    Event::OracleDraw(Sampled {
        term: Term(format!("v{i}")),
        provenance: Vec::new(),
    })
}

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

/// An oracle that injects a fixed latency per draw (modelling a network LLM call),
/// then delegates to a seeded oracle for the realized outcome. Replay never calls it.
struct LatencyOracle {
    inner: SeededOracle,
    delay: Duration,
}
impl Oracle for LatencyOracle {
    fn sample(&self, prompt: Prompt) -> Sampled {
        std::thread::sleep(self.delay);
        self.inner.sample(prompt)
    }
}

#[test]
fn e3_journaling_overhead() {
    let a = agent();

    // Warm up the allocator / caches.
    {
        let mut j = InMemoryJournal::new();
        for i in 0..1_000 {
            j.append(&a, mk_event(i));
        }
    }

    println!("\nE3 journaling overhead (H3a, H3d)\n---------------------------------");
    let scales = [1_000usize, 10_000, 100_000, 1_000_000];
    let mut last_ns = 0.0;
    for &n in &scales {
        let mut j = InMemoryJournal::new();
        let t = Instant::now();
        for i in 0..n {
            j.append(&a, mk_event(i));
        }
        let ns_per = t.elapsed().as_nanos() as f64 / n as f64;
        last_ns = ns_per;

        // H3d: exactly one journal entry per append => size is Theta(n).
        assert_eq!(
            j.len(&a) as usize,
            n,
            "journal must grow exactly one entry per append (Theta(n))"
        );
        println!(
            "  n = {:>9}   append = {:>6.1} ns/step   journal len = {}",
            n,
            ns_per,
            j.len(&a)
        );
    }

    // H3a: per-step append is O(1) amortized and far under the 5 us budget.
    assert!(
        last_ns < 5_000.0,
        "H3a: per-step journal append must stay under 5us (measured {last_ns:.1} ns)"
    );
    println!("  => O(1) amortized at ~{last_ns:.0} ns/step (<< 5 us, H3a); size Theta(n) (H3d)\n");
}

#[test]
fn e3_replay_speedup() {
    let a = agent();
    // Model an LLM call as 1 ms of latency per oracle draw.
    let slow = LatencyOracle {
        inner: oracle_with_seed(0x000A_11CE),
        delay: Duration::from_millis(1),
    };
    let reps = 80usize;

    // RECORD: every draw pays the oracle latency.
    let t0 = Instant::now();
    for _ in 0..reps {
        let mut rt = Runtime::new(a.clone(), &slow);
        let _ = rt.run(Mode::Record, false);
    }
    let rec = t0.elapsed();

    // Record one journal, then REPLAY it repeatedly: replay reads the journal and
    // performs zero oracle calls, so it pays none of the latency.
    let mut rt0 = Runtime::new(a.clone(), &slow);
    let _ = rt0.run(Mode::Record, false);
    let mut j = rt0.journal;
    let t1 = Instant::now();
    for _ in 0..reps {
        j.rewind(&a);
        let mut rt = Runtime::with_journal(a.clone(), &slow, j);
        let _ = rt.run(Mode::Replay, false);
        j = rt.journal;
    }
    let rep = t1.elapsed();

    let rec_us = rec.as_secs_f64() * 1e6 / reps as f64;
    let rep_us = rep.as_secs_f64() * 1e6 / reps as f64;
    let speedup = rec_us / rep_us.max(1e-9);

    println!("\nE3 replay speedup (H3c)\n-----------------------");
    println!("  injected oracle latency: 1 ms / draw (models an LLM call)");
    println!(
        "  record: {:>8.1} us/run    replay: {:>6.1} us/run    speedup: {:.0}x",
        rec_us, rep_us, speedup
    );
    println!("  (replay performs ZERO oracle calls; it reads the journal)\n");

    assert!(
        speedup >= 5.0,
        "H3c: with oracle latency, replay must be >= 5x faster than re-execution (got {speedup:.0}x)"
    );
}
