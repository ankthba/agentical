//! A toy multi-step agent that exercises the whole runtime.
//!
//! Its reasoning loop is the canonical "sample → branch → sample → commit effect":
//!
//! ```text
//! μX.  let route  ⇐ oracle("classify")  in        -- step 1: stochastic classify
//!      ( if route == "tool"                         -- step 2: deterministic branch
//!          then let arg ⇐ oracle("tool-arg") in     -- step 3a: stochastic arg
//!               do[commit] arg                       -- step 4a: DURABLE effect (WAL)
//!          else let ans ⇐ oracle("answer") in        -- step 3b: stochastic answer
//!               return ans )                          -- step 4b: pure halt
//! ```
//!
//! Every oracle draw and every durable effect goes through the [`Ports`] handle the
//! [`Runtime`](crate::runtime::Runtime) installs in the agent's [`State`]. The agent
//! itself contains **no** randomness and **no** journal access — it just asks its
//! ports to "draw" or "commit", and the runtime decides record-vs-replay. That is
//! what makes the observable trace a pure function of the journal on replay.

use crate::agent::{Agent, Step};
use crate::runtime::Ports;
use crate::types::Term;

/// Which reduction the toy agent will perform on its next `step`. Mirrors the
/// program-counter positions in the loop above; the program counter (which variant
/// we're in) is *data-independent*, so the journal cursor lands on the same entry on
/// record and on replay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pc {
    Classify,
    Branch,
    ToolArg,
    Commit,
    Answer,
    Halt,
}

/// The agent's private state `Σ`. Holds the program counter, the values drawn so far,
/// and the [`Ports`] handle the runtime uses to route effectful steps.
pub struct ToyState<'a> {
    pc: Pc,
    route: Option<Term>,
    arg: Option<Term>,
    answer: Option<Term>,
    /// The runtime-owned effect ports (oracle draw / durable effect), record-or-replay.
    pub ports: Ports<'a>,
}

impl<'a> ToyState<'a> {
    pub fn new(ports: Ports<'a>) -> Self {
        Self {
            pc: Pc::Classify,
            route: None,
            arg: None,
            answer: None,
            ports,
        }
    }
}

/// The toy reasoning agent. Stateless itself — all state lives in [`ToyState`].
pub struct ToyAgent;

impl Agent for ToyAgent {
    type State = ToyState<'static>;
    // Final result: a string-y term (the committed tool arg, or the direct answer).
    type Out = Term;

    fn step(&mut self, state: &mut Self::State) -> Step<Self::Out> {
        drive(state)
    }
}

// DESIGN-NOTE: `Agent::step(&mut self, state)` passes no `ctx`, so the agent cannot
// reach the Oracle / Journal / Channel that the calculus' `step` is supposed to hand
// it. We work around it by putting a `Ports` handle *inside* `State`. Feedback for
// formal-core: `step` should take a context handle (the `ctx` already mentioned in
// the doc comment), e.g. `fn step(&mut self, state, ctx: &mut Ctx) -> Step`, so the
// effect surface isn't smuggled through `State`. Also `type State` being an
// associated type forces a `'static` lifetime on the borrowed ports when used
// through the trait; the runtime therefore drives the agent via the free `drive`
// function below (generic over the ports' lifetime) rather than the trait method,
// and the trait impl is kept only to satisfy the contract.

/// The actual loop body, generic over the ports' lifetime so the runtime can lend it
/// a short-lived `Ports`. Returns a [`Step`] each call.
pub fn drive<'a>(state: &mut ToyState<'a>) -> Step<Term> {
    match state.pc {
        Pc::Classify => {
            // step 1: stochastic classify into a route.
            let route = state.ports.oracle_draw("classify");
            state.route = Some(route);
            state.pc = Pc::Branch;
            Step::Continue
        }
        Pc::Branch => {
            // step 2: deterministic branch on the drawn route.
            let route = state.route.as_ref().expect("route drawn in Classify");
            if route.0 == "tool" {
                state.pc = Pc::ToolArg;
            } else {
                state.pc = Pc::Answer;
            }
            Step::Continue
        }
        Pc::ToolArg => {
            // step 3a: draw the tool argument.
            let arg = state.ports.oracle_draw("tool-arg");
            state.arg = Some(arg);
            state.pc = Pc::Commit;
            Step::Continue
        }
        Pc::Commit => {
            // step 4a: durable effect, write-ahead logged by the runtime.
            let arg = state.arg.clone().expect("arg drawn in ToolArg");
            let committed = state.ports.durable_effect("commit", arg);
            state.pc = Pc::Halt;
            // The effect's recorded result becomes the agent's output.
            state.answer = Some(committed);
            Step::Continue
        }
        Pc::Answer => {
            // step 3b: draw a direct answer.
            let ans = state.ports.oracle_draw("answer");
            state.answer = Some(ans);
            state.pc = Pc::Halt;
            Step::Continue
        }
        Pc::Halt => {
            // step 4b / terminal: emit the final value.
            let out = state.answer.clone().unwrap_or_else(|| Term(String::new()));
            Step::Done(out)
        }
    }
}
