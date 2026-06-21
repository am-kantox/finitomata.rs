use std::fmt::Debug;
use std::hash::Hash;

use async_trait::async_trait;

use crate::error::FinitomataError;

/// Result of a state transition attempt, returned by [`Finitomata::on_transition`].
#[derive(Debug, Clone)]
pub enum TransitionResult<S, P> {
    /// Transition succeeded; move to the given target state.
    Ok(S),
    /// Transition succeeded; move to the given state and replace the payload.
    OkWithPayload(S, P),
    /// Transition failed; triggers [`Finitomata::on_failure`] and rolls back.
    Error(FinitomataError),
}

/// Core trait for defining a finite state machine's behavior.
///
/// Implementors provide the transition logic and optional lifecycle hooks.
/// The FSM engine calls these methods in a well-defined sequence during each
/// transition (see [`crate::engine::transit`] for the full callback order).
///
/// # Associated Types
///
/// - `State` — the set of states the FSM can be in (typically an enum)
/// - `Event` — the set of events that trigger transitions (typically an enum)
/// - `Payload` — the mutable data carried through the FSM's lifetime
///
/// # Lifecycle Hooks (all optional)
///
/// | Hook | When called |
/// |------|-------------|
/// | `on_start` | Once, when the FSM actor is first spawned |
/// | `on_enter` | After transitioning into a new state |
/// | `on_exit` | Before leaving the current state |
/// | `on_failure` | When `on_transition` returns `Error` or target is invalid |
/// | `on_timer` | On each timer tick (if timer is configured) |
/// | `on_terminate` | When the FSM is shutting down |
#[async_trait]
pub trait Finitomata: Send + Sync + 'static {
    type State: Clone + Send + Sync + Eq + Hash + Debug + Ord;
    type Event: Clone + Send + Sync + Eq + Hash + Debug + Ord;
    type Payload: Clone + Send + Sync + Debug;

    /// Core transition logic. Given the current state, event, and payloads,
    /// returns which state to move to (or an error to abort the transition).
    async fn on_transition(
        &mut self,
        from: &Self::State,
        event: &Self::Event,
        event_payload: &Self::Payload,
        state_payload: &mut Self::Payload,
    ) -> TransitionResult<Self::State, Self::Payload>;

    /// Called once when the FSM actor is first started.
    async fn on_start(&mut self, _payload: &mut Self::Payload) {}

    /// Called after the FSM has entered a new state.
    async fn on_enter(&mut self, _state: &Self::State, _payload: &mut Self::Payload) {}

    /// Called before the FSM leaves its current state.
    async fn on_exit(&mut self, _state: &Self::State, _payload: &mut Self::Payload) {}

    /// Called when a transition fails (either from `TransitionResult::Error`
    /// or because the target state was not in the graph's allowed set).
    async fn on_failure(
        &mut self,
        _event: &Self::Event,
        _reason: &FinitomataError,
        _payload: &mut Self::Payload,
    ) {
    }

    /// Called on each timer tick. Return `Some((event, payload))` to trigger
    /// a transition, or `None` to do nothing.
    async fn on_timer(
        &mut self,
        _state: &Self::State,
        _payload: &mut Self::Payload,
    ) -> Option<(Self::Event, Self::Payload)> {
        None
    }

    /// Called when the FSM is shutting down (either via auto-terminate on
    /// reaching a final state, or via explicit shutdown).
    async fn on_terminate(&mut self, _payload: &mut Self::Payload) {}
}
