use std::fmt;
use thiserror::Error;

/// Identifies which phase of FSM processing produced an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorStage {
    OnTransition,
    OnEnter,
    OnExit,
    Persistency,
    NotAllowed,
    NotResponds,
    Validation,
    Unknown,
}

impl fmt::Display for ErrorStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OnTransition => write!(f, "on_transition"),
            Self::OnEnter => write!(f, "on_enter"),
            Self::OnExit => write!(f, "on_exit"),
            Self::Persistency => write!(f, "persistency"),
            Self::NotAllowed => write!(f, "not_allowed"),
            Self::NotResponds => write!(f, "not_responds"),
            Self::Validation => write!(f, "validation"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

/// The primary error type for FSM operations.
///
/// Contains the processing stage where the error occurred, a human-readable
/// reason, and optional context about which state/event was involved.
#[derive(Debug, Clone, Error)]
#[error("[{stage}] {reason}")]
pub struct FinitomataError {
    pub stage: ErrorStage,
    pub reason: String,
    pub state: Option<String>,
    pub event: Option<String>,
}

impl FinitomataError {
    /// The current state does not respond to the given event.
    pub fn not_responds(state: impl fmt::Display, event: impl fmt::Display) -> Self {
        Self {
            stage: ErrorStage::NotResponds,
            reason: format!("state `{state}` does not respond to event `{event}`"),
            state: Some(state.to_string()),
            event: Some(event.to_string()),
        }
    }

    /// The transition target returned by `on_transition` is not in the graph's allowed set.
    pub fn not_allowed(
        from: impl fmt::Display,
        to: impl fmt::Display,
        event: impl fmt::Display,
    ) -> Self {
        Self {
            stage: ErrorStage::NotAllowed,
            reason: format!("transition from `{from}` to `{to}` via `{event}` is not allowed"),
            state: Some(from.to_string()),
            event: Some(event.to_string()),
        }
    }

    /// Error during the `on_transition` callback.
    pub fn on_transition(reason: impl fmt::Display) -> Self {
        Self {
            stage: ErrorStage::OnTransition,
            reason: reason.to_string(),
            state: None,
            event: None,
        }
    }

    /// Error during the `on_enter` callback.
    pub fn on_enter(reason: impl fmt::Display) -> Self {
        Self {
            stage: ErrorStage::OnEnter,
            reason: reason.to_string(),
            state: None,
            event: None,
        }
    }

    /// Error during the `on_exit` callback.
    pub fn on_exit(reason: impl fmt::Display) -> Self {
        Self {
            stage: ErrorStage::OnExit,
            reason: reason.to_string(),
            state: None,
            event: None,
        }
    }

    /// Error from the persistence layer.
    pub fn persistency(reason: impl fmt::Display) -> Self {
        Self {
            stage: ErrorStage::Persistency,
            reason: reason.to_string(),
            state: None,
            event: None,
        }
    }

    /// Validation or general operational error.
    pub fn validation(reason: impl fmt::Display) -> Self {
        Self {
            stage: ErrorStage::Validation,
            reason: reason.to_string(),
            state: None,
            event: None,
        }
    }
}

/// Errors detected during compile-time or runtime graph validation.
#[derive(Debug, Clone, Error)]
pub enum ValidationError {
    #[error("no initial state defined")]
    NoInitialState,
    #[error("multiple initial states: {0:?}")]
    MultipleInitialStates(Vec<String>),
    #[error("no final state defined")]
    NoFinalState,
    #[error("unreachable state: {0}")]
    UnreachableState(String),
    #[error("orphan state with no incoming or outgoing transitions: {0}")]
    OrphanState(String),
}

/// Errors from the persistence layer.
#[derive(Debug, Clone, Error)]
pub enum PersistencyError {
    #[error("load failed: {0}")]
    LoadFailed(String),
    #[error("store failed: {0}")]
    StoreFailed(String),
    #[error("not found: {0}")]
    NotFound(String),
}
