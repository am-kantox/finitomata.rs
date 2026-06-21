//! Persistence trait and backends for durable FSM state.

pub mod memory;

use async_trait::async_trait;

use crate::callbacks::Finitomata;
use crate::error::PersistencyError;
use crate::state::Lifecycle;

/// Trait for persisting FSM state across restarts.
///
/// Implementations store and retrieve the FSM's current state and payload,
/// enabling recovery after crashes when used with supervised FSMs.
///
/// # Provided Implementations
///
/// - [`memory::InMemoryPersistency`] — in-process `DashMap`-backed store (for testing / single-process)
///
/// # Implementing Custom Backends
///
/// Implement this trait for databases, Redis, files, etc. The `store` method is
/// called after every successful transition; `load` is called on FSM startup to
/// recover prior state.
#[async_trait]
pub trait Persistency<F: Finitomata>: Send + Sync {
    /// Loads the persisted state for the given FSM instance.
    /// Returns `Ok(None)` if no prior state exists.
    async fn load(
        &self,
        id: &str,
    ) -> Result<Option<(Lifecycle, F::State, F::Payload)>, PersistencyError>;

    /// Persists the current state and payload after a successful transition.
    async fn store(
        &self,
        id: &str,
        state: &F::State,
        payload: &F::Payload,
    ) -> Result<(), PersistencyError>;

    /// Persists an error that occurred during a transition (optional hook).
    async fn store_error(
        &self,
        id: &str,
        error: &crate::error::FinitomataError,
    ) -> Result<(), PersistencyError>;
}
