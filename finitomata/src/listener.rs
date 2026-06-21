//! Transition observer trait and built-in implementations.

use async_trait::async_trait;

use crate::callbacks::Finitomata;

/// Observes state transitions for external telemetry, logging, or event propagation.
///
/// Listeners are notified after each successful transition. They cannot influence
/// the transition outcome — use [`Finitomata::on_transition`] for that.
#[async_trait]
pub trait Listener<F: Finitomata>: Send + Sync {
    /// Called after a successful state transition.
    async fn on_transition(&self, name: &str, from: &F::State, to: &F::State, event: &F::Event);
}

/// A no-op listener that discards all notifications.
pub struct NoopListener;

#[async_trait]
impl<F: Finitomata> Listener<F> for NoopListener {
    async fn on_transition(
        &self,
        _name: &str,
        _from: &F::State,
        _to: &F::State,
        _event: &F::Event,
    ) {
    }
}

/// A listener that emits structured `tracing` info events for each transition.
pub struct TracingListener;

#[async_trait]
impl<F: Finitomata> Listener<F> for TracingListener {
    async fn on_transition(&self, name: &str, from: &F::State, to: &F::State, event: &F::Event) {
        tracing::info!(
            fsm = name,
            from = ?from,
            to = ?to,
            event = ?event,
            "FSM transition"
        );
    }
}
