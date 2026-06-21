//! Core transition engine implementing the Finitomata callback sequence.

use crate::callbacks::{Finitomata, TransitionResult};
use crate::error::FinitomataError;
use crate::listener::Listener;
use crate::persistency::Persistency;
use crate::state::{FsmState, Lifecycle};
use crate::transition::{EventKind, TransitionGraph};

/// Context passed to [`transit`] containing optional infrastructure services.
pub struct TransitContext<'a, F: Finitomata> {
    pub persistency: Option<&'a dyn Persistency<F>>,
    pub listener: Option<&'a dyn Listener<F>>,
    pub auto_terminate: bool,
}

/// The outcome of a successful transition.
pub enum TransitOutcome {
    /// Normal transition completed.
    Transitioned,
    /// Soft event was not applicable to current state; silently skipped.
    SoftSkipped,
    /// Transition reached a final state and the FSM has been terminated.
    AutoTerminate,
    /// A hard event completed; the caller should immediately fire the next hard event.
    HardContinue,
}

/// Executes a single state transition with the full Finitomata callback sequence.
///
/// # Callback sequence
///
/// 1. Validate event responds to current state (soft events silently skip)
/// 2. `on_exit(current_state, payload)`
/// 3. `on_transition(from, event, event_payload, payload)` → target state
/// 4. Validate target is in the graph's allowed set
/// 5. Persist (if configured)
/// 6. Update state + history
/// 7. Notify listener
/// 8. `on_enter(new_state, payload)`
/// 9. If auto_terminate and target is final → terminate
/// 10. If event kind is Hard → return `HardContinue`
///
/// On failure at step 3 or 4: `on_failure()` is called, the FSM re-enters the
/// original state, and no persistence or listener notification occurs.
pub async fn transit<F: Finitomata>(
    fsm: &mut F,
    graph: &TransitionGraph<F::State, F::Event>,
    state: &mut FsmState<F::State, F::Payload>,
    event: F::Event,
    event_payload: F::Payload,
    ctx: &TransitContext<'_, F>,
) -> Result<TransitOutcome, FinitomataError> {
    let (allowed_targets, kind) = match graph.allowed(&state.current, &event) {
        Some(result) => result,
        None => {
            let is_soft = graph
                .transitions()
                .iter()
                .any(|t| t.event == event && t.kind == EventKind::Soft);
            if is_soft {
                return Ok(TransitOutcome::SoftSkipped);
            }
            return Err(FinitomataError::not_responds(
                format!("{:?}", state.current),
                format!("{:?}", event),
            ));
        }
    };

    // 1. on_exit(current_state)
    fsm.on_exit(&state.current, &mut state.payload).await;

    // 2. on_transition → determine target state
    let from = state.current.clone();
    let result = fsm
        .on_transition(&from, &event, &event_payload, &mut state.payload)
        .await;

    let target = match result {
        TransitionResult::Ok(target) => target,
        TransitionResult::OkWithPayload(target, new_payload) => {
            state.payload = new_payload;
            target
        }
        TransitionResult::Error(err) => {
            fsm.on_failure(&event, &err, &mut state.payload).await;
            state.set_error(err.clone());
            fsm.on_enter(&from, &mut state.payload).await;
            return Err(err);
        }
    };

    // 3. Validate target is in the allowed set from the transition graph
    if !allowed_targets.contains(&target) {
        let err = FinitomataError::not_allowed(
            format!("{:?}", from),
            format!("{:?}", target),
            format!("{:?}", event),
        );
        fsm.on_failure(&event, &err, &mut state.payload).await;
        state.set_error(err.clone());
        fsm.on_enter(&from, &mut state.payload).await;
        return Err(err);
    }

    // 4. Persist
    if let Some(persist) = ctx.persistency
        && let Err(e) = persist.store(&state.name, &target, &state.payload).await
    {
        let err = FinitomataError::persistency(e);
        state.set_error(err.clone());
        fsm.on_enter(&from, &mut state.payload).await;
        return Err(err);
    }

    // 5. Update state + history
    state.clear_error();
    state.transition_to(target.clone());
    state.lifecycle = Lifecycle::Running;

    // 6. Notify listener
    if let Some(listener) = ctx.listener {
        listener
            .on_transition(&state.name, &from, &target, &event)
            .await;
    }

    // 7. on_enter(new_state)
    fsm.on_enter(&target, &mut state.payload).await;

    // 8. Check auto-terminate
    if ctx.auto_terminate && graph.is_final(&target) {
        state.lifecycle = Lifecycle::Terminating;
        fsm.on_terminate(&mut state.payload).await;
        state.lifecycle = Lifecycle::Terminated;
        return Ok(TransitOutcome::AutoTerminate);
    }

    // 9. Hard events trigger immediate re-transition
    if kind == EventKind::Hard {
        return Ok(TransitOutcome::HardContinue);
    }

    Ok(TransitOutcome::Transitioned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::callbacks::Finitomata;
    use crate::transition::{Transition, TransitionGraph};
    use std::collections::BTreeSet;

    #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
    enum State {
        Idle,
        Running,
        Done,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
    enum Event {
        Start,
        Finish,
    }

    #[derive(Debug, Clone)]
    struct Payload {
        count: u32,
    }

    struct TestFsm;

    #[async_trait::async_trait]
    impl Finitomata for TestFsm {
        type State = State;
        type Event = Event;
        type Payload = Payload;

        async fn on_transition(
            &mut self,
            _from: &State,
            event: &Event,
            _event_payload: &Payload,
            state_payload: &mut Payload,
        ) -> TransitionResult<State, Payload> {
            match event {
                Event::Start => {
                    state_payload.count += 1;
                    TransitionResult::Ok(State::Running)
                }
                Event::Finish => TransitionResult::Ok(State::Done),
            }
        }
    }

    fn test_graph() -> TransitionGraph<State, Event> {
        TransitionGraph::new(
            State::Idle,
            BTreeSet::from([State::Done]),
            vec![
                Transition {
                    from: State::Idle,
                    to: vec![State::Running],
                    event: Event::Start,
                    kind: EventKind::Normal,
                },
                Transition {
                    from: State::Running,
                    to: vec![State::Done],
                    event: Event::Finish,
                    kind: EventKind::Normal,
                },
            ],
        )
    }

    #[tokio::test]
    async fn test_basic_transition() {
        let graph = test_graph();
        let mut fsm = TestFsm;
        let mut state = FsmState::new("test", State::Idle, Payload { count: 0 });
        let ctx = TransitContext {
            persistency: None,
            listener: None,
            auto_terminate: false,
        };

        let result = transit(
            &mut fsm,
            &graph,
            &mut state,
            Event::Start,
            Payload { count: 0 },
            &ctx,
        )
        .await;

        assert!(result.is_ok());
        assert_eq!(state.current, State::Running);
        assert_eq!(state.payload.count, 1);
    }

    #[tokio::test]
    async fn test_invalid_event() {
        let graph = test_graph();
        let mut fsm = TestFsm;
        let mut state = FsmState::new("test", State::Idle, Payload { count: 0 });
        let ctx = TransitContext {
            persistency: None,
            listener: None,
            auto_terminate: false,
        };

        let result = transit(
            &mut fsm,
            &graph,
            &mut state,
            Event::Finish,
            Payload { count: 0 },
            &ctx,
        )
        .await;

        assert!(result.is_err());
        assert_eq!(state.current, State::Idle);
    }

    #[tokio::test]
    async fn test_auto_terminate() {
        let graph = test_graph();
        let mut fsm = TestFsm;
        let mut state = FsmState::new("test", State::Running, Payload { count: 0 });
        let ctx = TransitContext {
            persistency: None,
            listener: None,
            auto_terminate: true,
        };

        let result = transit(
            &mut fsm,
            &graph,
            &mut state,
            Event::Finish,
            Payload { count: 0 },
            &ctx,
        )
        .await;

        assert!(matches!(result, Ok(TransitOutcome::AutoTerminate)));
        assert_eq!(state.lifecycle, Lifecycle::Terminated);
    }
}
