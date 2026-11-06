//! Cross-cutting newtypes shared by the primitives.
//!
//! Representations are deliberately opaque placeholders — the paper fixes the
//! object-language syntax (Phase 2); the crate only needs their *roles* to be stable.

/// Stable identity of an agent in a running system.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AgentId(pub String);

/// A term in the object language: the thing an [`crate::oracle::Oracle`] draw
/// produces and an [`crate::agent::Agent`] reasons over. Opaque for now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Term(pub String);
