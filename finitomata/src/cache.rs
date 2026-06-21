//! Concurrent in-memory state cache for fast reads without actor round-trips.

use std::fmt::Debug;

use dashmap::DashMap;

use crate::state::FsmState;

/// A concurrent state cache backed by [`DashMap`].
///
/// Updated after every successful transition, this cache allows reading
/// FSM state without sending a message to the actor (no round-trip latency).
/// This is the Rust equivalent of Finitomata's ETS-backed state cache.
pub struct StateCache<S: Clone + Debug, P: Clone + Debug> {
    cache: DashMap<String, FsmState<S, P>>,
}

impl<S: Clone + Debug + Send + Sync, P: Clone + Debug + Send + Sync> StateCache<S, P> {
    /// Creates a new empty state cache.
    pub fn new() -> Self {
        Self {
            cache: DashMap::new(),
        }
    }

    /// Returns the full FSM state for the given name, or `None` if not present.
    pub fn get(&self, name: &str) -> Option<FsmState<S, P>> {
        self.cache.get(name).map(|entry| entry.value().clone())
    }

    /// Returns just the current state for the given name.
    pub fn get_state(&self, name: &str) -> Option<S> {
        self.cache
            .get(name)
            .map(|entry| entry.value().current.clone())
    }

    /// Returns just the payload for the given name.
    pub fn get_payload(&self, name: &str) -> Option<P> {
        self.cache
            .get(name)
            .map(|entry| entry.value().payload.clone())
    }

    /// Inserts or updates the cached state for an FSM instance.
    pub fn update(&self, state: &FsmState<S, P>) {
        self.cache.insert(state.name.clone(), state.clone());
    }

    /// Removes the cached state for the given name.
    pub fn remove(&self, name: &str) {
        self.cache.remove(name);
    }

    /// Returns `true` if the cache contains an entry for the given name.
    pub fn contains(&self, name: &str) -> bool {
        self.cache.contains_key(name)
    }

    /// Returns all cached entries as `(name, state)` pairs.
    pub fn all(&self) -> Vec<(String, FsmState<S, P>)> {
        self.cache
            .iter()
            .map(|entry| (entry.key().clone(), entry.value().clone()))
            .collect()
    }

    /// Returns the number of cached entries.
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /// Returns `true` if the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }
}

impl<S: Clone + Debug + Send + Sync, P: Clone + Debug + Send + Sync> Default for StateCache<S, P> {
    fn default() -> Self {
        Self::new()
    }
}
