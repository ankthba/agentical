//! Primitive #3 — **Channel**: typed communication, delegation, sharing.
//!
//! π-calculus-style names carrying typed messages between agents. **All
//! *adversarial* (scheduler/ordering) nondeterminism lives here**; all
//! *probabilistic* nondeterminism is funneled through [`crate::oracle`]. Keeping
//! the two apart is the standard probabilistic-automata discipline and keeps the
//! proofs honest.

use core::marker::PhantomData;

/// A channel over which agents send/receive typed messages, delegate sub-tasks,
/// and coordinate. `recv` is a point where externally-determined values enter, so
/// the runtime journals them (see [`crate::journal::Event::ChannelRecv`]).
pub trait Channel {
    type Msg;

    /// `c!v` — send a message.
    fn send(&self, msg: Self::Msg);

    /// `c?x` — receive a message. Journaled for replay.
    fn recv(&self) -> Self::Msg;
}

/// A compile-time interaction contract (session type), sketched as a typestate.
///
/// `P` is the current protocol state; in the full design, method availability is
/// gated by `P` so out-of-protocol sends/receives don't type-check. This is the
/// basis for the progress / deadlock-freedom result on well-typed compositions.
pub struct Session<P> {
    _protocol: PhantomData<P>,
}
