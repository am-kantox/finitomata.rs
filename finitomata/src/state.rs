use std::collections::VecDeque;
use std::fmt::Debug;

use crate::error::FinitomataError;

/// Lifecycle phase of an FSM instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Lifecycle {
    /// Just created, not yet started.
    Created,
    /// Recovered from persistence.
    Loaded,
    /// Actively processing events.
    Running,
    /// Shutdown initiated, executing on_terminate.
    Terminating,
    /// Fully stopped.
    Terminated,
}

/// A single entry in the state history, tracking consecutive visits.
#[derive(Debug, Clone)]
pub struct HistoryEntry<S> {
    pub state: S,
    pub count: usize,
}

/// A bounded history of state transitions that collapses consecutive repeats.
///
/// When the same state appears multiple times in a row, only one entry is
/// stored with an incremented `count`. When the history exceeds `max_size`,
/// the oldest entry is evicted.
#[derive(Debug, Clone)]
pub struct BoundedHistory<S> {
    entries: VecDeque<HistoryEntry<S>>,
    max_size: usize,
}

impl<S: PartialEq + Clone + Debug> BoundedHistory<S> {
    /// Creates a new bounded history with the given maximum number of entries.
    pub fn new(max_size: usize) -> Self {
        Self {
            entries: VecDeque::with_capacity(max_size),
            max_size,
        }
    }

    /// Records a state visit. If it matches the most recent entry, increments
    /// the count; otherwise appends a new entry (evicting the oldest if full).
    pub fn push(&mut self, state: S) {
        if let Some(last) = self.entries.back_mut()
            && last.state == state
        {
            last.count += 1;
            return;
        }
        if self.entries.len() >= self.max_size {
            self.entries.pop_front();
        }
        self.entries.push_back(HistoryEntry { state, count: 1 });
    }

    /// Returns the full history as a slice of entries.
    pub fn entries(&self) -> &VecDeque<HistoryEntry<S>> {
        &self.entries
    }

    /// Returns the most recent history entry, if any.
    pub fn last(&self) -> Option<&HistoryEntry<S>> {
        self.entries.back()
    }

    /// Returns the number of distinct entries in the history.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if the history is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// The full runtime state of an FSM instance.
///
/// Tracks the current state, payload, transition history, lifecycle phase,
/// and the last error (if any).
#[derive(Debug, Clone)]
pub struct FsmState<S: Clone + Debug, P: Clone + Debug> {
    /// The registered name of this FSM instance.
    pub name: String,
    /// The current state.
    pub current: S,
    /// The mutable payload carried through the FSM's lifetime.
    pub payload: P,
    /// Bounded history of state transitions (collapses consecutive repeats).
    pub history: BoundedHistory<S>,
    /// Current lifecycle phase.
    pub lifecycle: Lifecycle,
    /// The most recent error, if any.
    pub last_error: Option<FinitomataError>,
}

impl<S: Clone + Debug + PartialEq, P: Clone + Debug> FsmState<S, P> {
    /// Creates a new FSM state with the given name, initial state, and payload.
    pub fn new(name: impl Into<String>, initial_state: S, payload: P) -> Self {
        let name = name.into();
        let mut history = BoundedHistory::new(32);
        history.push(initial_state.clone());
        Self {
            name,
            current: initial_state,
            payload,
            history,
            lifecycle: Lifecycle::Created,
            last_error: None,
        }
    }

    /// Transitions to a new state, updating both `current` and the history.
    pub fn transition_to(&mut self, new_state: S) {
        self.current = new_state.clone();
        self.history.push(new_state);
    }

    /// Replaces the payload.
    pub fn set_payload(&mut self, payload: P) {
        self.payload = payload;
    }

    /// Records an error.
    pub fn set_error(&mut self, error: FinitomataError) {
        self.last_error = Some(error);
    }

    /// Clears any recorded error.
    pub fn clear_error(&mut self) {
        self.last_error = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bounded_history_collapses_repeats() {
        let mut history = BoundedHistory::new(10);
        history.push("idle");
        history.push("idle");
        history.push("idle");
        assert_eq!(history.len(), 1);
        assert_eq!(history.last().unwrap().count, 3);
    }

    #[test]
    fn test_bounded_history_distinct_entries() {
        let mut history = BoundedHistory::new(10);
        history.push("idle");
        history.push("running");
        history.push("idle");
        assert_eq!(history.len(), 3);
    }

    #[test]
    fn test_bounded_history_eviction() {
        let mut history = BoundedHistory::new(3);
        history.push("a");
        history.push("b");
        history.push("c");
        history.push("d");
        assert_eq!(history.len(), 3);
        assert_eq!(history.entries()[0].state, "b");
    }

    #[test]
    fn test_fsm_state_new() {
        let state: FsmState<&str, u32> = FsmState::new("test", "idle", 0);
        assert_eq!(state.current, "idle");
        assert_eq!(state.payload, 0);
        assert_eq!(state.lifecycle, Lifecycle::Created);
        assert!(state.last_error.is_none());
    }
}
