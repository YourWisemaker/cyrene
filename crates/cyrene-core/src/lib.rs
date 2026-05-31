//! `cyrene-core`: core domain types, traits, and the Agent_Loop contract for Cyrene.
//!
//! This crate sits at the bottom of the dependency graph: it depends on nothing
//! app-specific, and every adapter crate depends on it. Task 2.1 establishes the
//! domain types — [`Session`], [`Plan`], [`Step`], [`StepKind`], [`Risk`],
//! [`Budget`], [`Money`], stable id newtypes, and the crate error model
//! ([`CoreError`] + [`Recoverability`]). Task 2.2 adds the three plugin traits —
//! [`Channel`], [`Memory`], and [`Model`] — plus their request/response and
//! descriptor types, each with a `thiserror` error enum implementing
//! [`Recoverable`] so adapter crates return concrete errors the loop can
//! classify.
//!
//! All domain and message types derive `serde::{Serialize, Deserialize}`
//! (round-trips are unit-tested in task 2.3) plus `Debug`/`Clone`/`PartialEq`
//! where reasonable. The traits themselves are not serializable.

mod budget;
mod channel;
mod error;
mod ids;
mod memory;
mod model;
mod money;
mod plan;
mod risk;
mod session;

pub use budget::{Budget, BudgetLimit};
pub use channel::{
    Channel, ChannelError, ChannelHealth, ChannelId, InboundMessage, OutboundMessage,
};
pub use error::{CoreError, Recoverability, Recoverable};
pub use ids::{BranchId, ChannelOrigin, NodeId, PlanId, SessionId, UserId};
pub use memory::{Fact, Memory, MemoryError, MemoryHit, MemoryQuery, Relation};
pub use model::{
    ChatMessage, FinishReason, Model, ModelDescriptor, ModelError, ModelRequest, ModelResponse,
    Role, Tier, TokenUsage,
};
pub use money::Money;
pub use plan::{Plan, Step, StepKind, ToolCall};
pub use risk::Risk;
pub use session::Session;

/// Returns the stable identifier of this subsystem crate.
#[must_use]
pub fn subsystem() -> &'static str {
    "cyrene-core"
}

#[cfg(test)]
mod tests {
    use super::subsystem;

    #[test]
    fn subsystem_id_is_nonempty() {
        assert!(!subsystem().is_empty());
    }
}
