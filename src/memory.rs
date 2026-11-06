//! Primitive #4 — **Memory**: scoped, IFC-labeled, retained state.
//!
//! More than a variable environment: every cell has a **scope** (who can see it), a
//! **retention** (how long it lives), and an information-flow **label** (lifted from
//! LLMbda's lattice). This is where the brief's "what agents remember, for how long,
//! with what guarantees" is made precise.

/// Information-flow label, lifted from LLMbda's security lattice. Flow is permitted
/// only "upward"; a high-labeled value must never reach a low-labeled sink.
/// (Two points here; the real lattice is a parameter of the model.)
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Label {
    Low,
    High,
}

/// Where a memory cell is visible.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// Visible only to the owning agent.
    Private,
    /// Shared within a delegation group.
    Group,
    /// Visible system-wide.
    Global,
}

/// How long a memory cell survives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Retention {
    /// Discarded at end of the current turn.
    Turn,
    /// Lives for the session, then provably unreadable.
    Session,
    /// Persisted; survives restart (journaled).
    Durable,
}

/// A value tagged with its information-flow label.
#[derive(Clone, Debug)]
pub struct Labeled<V> {
    pub label: Label,
    pub value: V,
}

/// An unforgeable capability gating a read/write (a token in the real runtime).
#[derive(Clone, Copy, Debug)]
pub struct Cap;

/// Scoped, labeled store. Reads/writes check label flow and scope/retention.
/// Distinct from the bounded working [`Context`].
pub trait Memory {
    type Key;
    type Val;

    /// Read a cell if the capability and label policy permit.
    fn read(&self, key: &Self::Key, cap: Cap) -> Option<Labeled<Self::Val>>;

    /// Write a cell with an explicit scope, retention, and (via `val`) label.
    fn write(
        &mut self,
        key: Self::Key,
        val: Labeled<Self::Val>,
        scope: Scope,
        retention: Retention,
        cap: Cap,
    );
}

/// The bounded, ephemeral working window (the "context window"). Truncation is an
/// **explicit, modeled** operation — never an invisible side effect — so reasoning
/// about what an agent can still know stays sound.
pub trait Context {
    type Item;

    fn push(&mut self, item: Self::Item);

    /// Lossy truncation to fit a bound. Modeled explicitly on purpose.
    fn truncate_to(&mut self, budget: usize);
}
