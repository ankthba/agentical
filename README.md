# agentical

> Provably sound replay, crash recovery, and noninterference for nondeterministic LLM agents:
> the paper, a Lean 4 mechanization of its core results, and a dependency-free Rust research
> prototype.

**Paper:** [`paper/main.pdf`](paper/main.pdf) (source in [`paper/`](paper/)).

Every model call is treated as a draw from a probability measure, and every response is
recorded in an append-only journal. The paper proves that replaying a run from its journal
reproduces it exactly, so record and replay induce the same distribution over traces, and that
resuming after a crash continues the run with the distribution it would have had without the
crash. Both results extend to systems of concurrent agents that share memory and exchange
messages, under deterministic scheduling with one global journal. For such systems the paper
also proves a probabilistic noninterference theorem: under a floating-label discipline and a
priority scheduler that sees only public events, secret data cannot influence what public
observers see.

## Contents

| Path | What it is |
|------|------------|
| [`paper/`](paper/) | The paper (LaTeX). Build with `tectonic main.tex`; see [`paper/BUILD.md`](paper/BUILD.md). |
| [`lean/AgenticalT1.lean`](lean/AgenticalT1.lean) | Lean 4 mechanization of replay soundness and crash recovery, and their lifting to deterministic step systems (pointwise, `sorry`-free). |
| [`src/`](src/) | Traits for the agent, oracle and journal (plus interfaces for future multi-agent work), and [`src/runtime/`](src/runtime/): single-agent record, replay, crash resume, and write-ahead-logged effects. |
| [`tests/`](tests/) | Unit tests and the experiments E1–E3 from the paper's evaluation. |

## The six primitives

| Primitive | Trait | Role | Implemented in the prototype |
|-----------|-------|------|------------------------------|
| Agent | [`agent::Agent`](src/agent.rs) | unit of composition and failure | single agent |
| Oracle | [`oracle::Oracle`](src/oracle.rs) | an LLM call as a draw from a measure | seeded stand-in |
| Channel | [`channel::Channel`](src/channel.rs) | typed communication and delegation | interface only |
| Memory | [`memory::Memory`](src/memory.rs) | scoped, IFC-labelled state | interface only |
| Journal | [`journal::Journal`](src/journal.rs) | durable history and replay | in-memory backend |
| Supervisor | [`supervisor::Supervisor`](src/supervisor.rs) | failure propagation, compensation, escalation | interface only |

## Running

```
cargo test                                            # all tests
cargo test --release --test eval_e1 -- --nocapture    # E1: replay determinism
cargo test --release --test eval_e2 -- --nocapture    # E2: crash at every journal position
cargo test --release --test eval_e3 -- --nocapture    # E3: journaling overhead
cd lean && lean AgenticalT1.lean                      # check the Lean proof (Lean v4.30.0)
```

The crate has no dependencies beyond the Rust standard library.

## Status

- [x] Probabilistic semantics with a journal; replay soundness and crash recovery
- [x] Extension to concurrent agents (deterministic scheduling, global journal); noninterference
- [x] Lean 4 mechanization of the pointwise core and the lifting to systems
- [x] Single-agent runtime: record, replay, crash resume, write-ahead-logged effects
- [ ] Multi-agent runtime; mechanized noninterference; formalized side effects
- [ ] Persistent journal backend and experiments with real LLMs

## Use of AI tools

I developed this work with Anthropic's Claude as a collaborator on the proofs, code, and writing.
Any errors are my own.

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
