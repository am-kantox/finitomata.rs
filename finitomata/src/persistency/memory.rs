//! In-memory persistence backend backed by DashMap.

use async_trait::async_trait;
use dashmap::DashMap;

use crate::callbacks::Finitomata;
use crate::error::{FinitomataError, PersistencyError};
use crate::state::Lifecycle;

use super::Persistency;

/// In-memory persistence backend using a concurrent [`DashMap`].
///
/// Suitable for testing and single-process deployments where durability
/// across process restarts is not required. State is lost when the process exits.
pub struct InMemoryPersistency<F: Finitomata> {
    store: DashMap<String, (Lifecycle, F::State, F::Payload)>,
}

impl<F: Finitomata> InMemoryPersistency<F> {
    /// Creates a new empty in-memory persistence store.
    pub fn new() -> Self {
        Self {
            store: DashMap::new(),
        }
    }

    /// Returns the number of persisted FSM instances.
    pub fn len(&self) -> usize {
        self.store.len()
    }

    /// Returns `true` if no FSM instances are persisted.
    pub fn is_empty(&self) -> bool {
        self.store.is_empty()
    }

    /// Returns `true` if state is persisted for the given ID.
    pub fn contains(&self, id: &str) -> bool {
        self.store.contains_key(id)
    }

    /// Removes and returns the persisted state for the given ID.
    pub fn remove(&self, id: &str) -> Option<(Lifecycle, F::State, F::Payload)> {
        self.store.remove(id).map(|(_, v)| v)
    }
}

impl<F: Finitomata> Default for InMemoryPersistency<F> {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl<F: Finitomata> Persistency<F> for InMemoryPersistency<F>
where
    F::State: Clone + Send + Sync,
    F::Payload: Clone + Send + Sync,
{
    async fn load(
        &self,
        id: &str,
    ) -> Result<Option<(Lifecycle, F::State, F::Payload)>, PersistencyError> {
        Ok(self.store.get(id).map(|entry| entry.value().clone()))
    }

    async fn store(
        &self,
        id: &str,
        state: &F::State,
        payload: &F::Payload,
    ) -> Result<(), PersistencyError> {
        self.store.insert(
            id.to_string(),
            (Lifecycle::Running, state.clone(), payload.clone()),
        );
        Ok(())
    }

    async fn store_error(
        &self,
        _id: &str,
        _error: &FinitomataError,
    ) -> Result<(), PersistencyError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::callbacks::{Finitomata, TransitionResult};

    #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
    enum S {
        A,
        B,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
    #[allow(dead_code)]
    enum E {
        Go,
    }

    #[derive(Debug, Clone)]
    struct P(u32);

    struct TestFsm;

    #[async_trait]
    impl Finitomata for TestFsm {
        type State = S;
        type Event = E;
        type Payload = P;

        async fn on_transition(
            &mut self,
            _from: &S,
            _event: &E,
            _ep: &P,
            _sp: &mut P,
        ) -> TransitionResult<S, P> {
            TransitionResult::Ok(S::B)
        }
    }

    #[tokio::test]
    async fn test_store_and_load() {
        let persist = InMemoryPersistency::<TestFsm>::new();
        persist.store("fsm1", &S::A, &P(42)).await.unwrap();

        let loaded = persist.load("fsm1").await.unwrap().unwrap();
        assert_eq!(loaded.1, S::A);
        assert_eq!(loaded.2.0, 42);
    }

    #[tokio::test]
    async fn test_load_missing() {
        let persist = InMemoryPersistency::<TestFsm>::new();
        let loaded = persist.load("missing").await.unwrap();
        assert!(loaded.is_none());
    }
}
