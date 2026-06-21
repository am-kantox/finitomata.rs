//! Comprehensive integration tests for the finitomata public API.

use async_trait::async_trait;
use finitomata::{
    EventKind, Finitomata, FinitomataError, FinitomataSupervisor, FsmState, Lifecycle, Persistency,
    RestartStrategy, TransitionResult, finitomata, persistency::memory::InMemoryPersistency,
};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

// ============================================================================
// Test FSM definitions using the proc macro
// ============================================================================

#[finitomata(
    fsm = r#"
        [*] --> idle
        idle --> |start| running
        running --> |pause| paused
        running --> |stop| idle
        paused --> |resume| running
        paused --> |stop| idle
        idle --> |shutdown| off
        off --> |confirm| [*]
    "#,
    syntax = "mermaid",
    auto_terminate = true
)]
#[derive(Debug, Clone, Default)]
struct WorkflowFsm;

#[derive(Debug, Clone, Default)]
struct WorkflowPayload {
    started: bool,
    stopped: bool,
    transitions: Vec<String>,
}

#[async_trait]
impl Finitomata for WorkflowFsm {
    type State = WorkflowFsmState;
    type Event = WorkflowFsmEvent;
    type Payload = WorkflowPayload;

    async fn on_transition(
        &mut self,
        _from: &WorkflowFsmState,
        event: &WorkflowFsmEvent,
        _event_payload: &WorkflowPayload,
        state_payload: &mut WorkflowPayload,
    ) -> TransitionResult<WorkflowFsmState, WorkflowPayload> {
        state_payload.transitions.push(format!("{event:?}"));
        match event {
            WorkflowFsmEvent::Start => TransitionResult::Ok(WorkflowFsmState::Running),
            WorkflowFsmEvent::Pause => TransitionResult::Ok(WorkflowFsmState::Paused),
            WorkflowFsmEvent::Resume => TransitionResult::Ok(WorkflowFsmState::Running),
            WorkflowFsmEvent::Stop => TransitionResult::Ok(WorkflowFsmState::Idle),
            WorkflowFsmEvent::Shutdown => TransitionResult::Ok(WorkflowFsmState::Off),
            WorkflowFsmEvent::Confirm => TransitionResult::Ok(WorkflowFsmState::Off),
        }
    }

    async fn on_start(&mut self, payload: &mut WorkflowPayload) {
        payload.started = true;
    }

    async fn on_terminate(&mut self, payload: &mut WorkflowPayload) {
        payload.stopped = true;
    }
}

// FSM with hard events for testing auto-fire
#[finitomata(
    fsm = r#"
        [*] --> init
        init --> |boot!| ready
        ready --> |go| active
        active --> |done| finished
        finished --> |cleanup| [*]
    "#,
    syntax = "mermaid",
    auto_terminate = true
)]
#[derive(Debug, Clone, Default)]
struct HardEventFsm;

#[derive(Debug, Clone, Default)]
struct HardPayload {
    boot_count: u32,
}

#[async_trait]
impl Finitomata for HardEventFsm {
    type State = HardEventFsmState;
    type Event = HardEventFsmEvent;
    type Payload = HardPayload;

    async fn on_transition(
        &mut self,
        _from: &HardEventFsmState,
        event: &HardEventFsmEvent,
        _ep: &HardPayload,
        sp: &mut HardPayload,
    ) -> TransitionResult<HardEventFsmState, HardPayload> {
        match event {
            HardEventFsmEvent::Boot => {
                sp.boot_count += 1;
                TransitionResult::Ok(HardEventFsmState::Ready)
            }
            HardEventFsmEvent::Go => TransitionResult::Ok(HardEventFsmState::Active),
            HardEventFsmEvent::Done => TransitionResult::Ok(HardEventFsmState::Finished),
            HardEventFsmEvent::Cleanup => TransitionResult::Ok(HardEventFsmState::Finished),
        }
    }
}

// FSM with soft events
#[finitomata(
    fsm = r#"
        [*] --> idle
        idle --> |start| running
        running --> |tick?| running
        running --> |stop| idle
        idle --> |off| done
        done --> |confirm| [*]
    "#,
    syntax = "mermaid",
    auto_terminate = true
)]
#[derive(Debug, Clone, Default)]
struct SoftEventFsm;

#[derive(Debug, Clone, Default)]
struct SoftPayload {
    tick_count: u32,
}

#[async_trait]
impl Finitomata for SoftEventFsm {
    type State = SoftEventFsmState;
    type Event = SoftEventFsmEvent;
    type Payload = SoftPayload;

    async fn on_transition(
        &mut self,
        _from: &SoftEventFsmState,
        event: &SoftEventFsmEvent,
        _ep: &SoftPayload,
        sp: &mut SoftPayload,
    ) -> TransitionResult<SoftEventFsmState, SoftPayload> {
        match event {
            SoftEventFsmEvent::Start => TransitionResult::Ok(SoftEventFsmState::Running),
            SoftEventFsmEvent::Tick => {
                sp.tick_count += 1;
                TransitionResult::Ok(SoftEventFsmState::Running)
            }
            SoftEventFsmEvent::Stop => TransitionResult::Ok(SoftEventFsmState::Idle),
            SoftEventFsmEvent::Off => TransitionResult::Ok(SoftEventFsmState::Done),
            SoftEventFsmEvent::Confirm => TransitionResult::Ok(SoftEventFsmState::Done),
        }
    }
}

// FSM with timer
#[finitomata(
    fsm = r#"
        [*] --> waiting
        waiting --> |tick| counting
        counting --> |tick| counting
        counting --> |done| finished
        finished --> |end| [*]
    "#,
    syntax = "mermaid",
    auto_terminate = true
)]
#[derive(Debug, Clone, Default)]
struct TimerFsm;

#[derive(Debug, Clone, Default)]
struct TimerPayload {
    ticks: u32,
}

#[async_trait]
impl Finitomata for TimerFsm {
    type State = TimerFsmState;
    type Event = TimerFsmEvent;
    type Payload = TimerPayload;

    async fn on_transition(
        &mut self,
        _from: &TimerFsmState,
        event: &TimerFsmEvent,
        _ep: &TimerPayload,
        sp: &mut TimerPayload,
    ) -> TransitionResult<TimerFsmState, TimerPayload> {
        match event {
            TimerFsmEvent::Tick => {
                sp.ticks += 1;
                TransitionResult::Ok(TimerFsmState::Counting)
            }
            TimerFsmEvent::Done => TransitionResult::Ok(TimerFsmState::Finished),
            TimerFsmEvent::End => TransitionResult::Ok(TimerFsmState::Finished),
        }
    }

    async fn on_timer(
        &mut self,
        state: &TimerFsmState,
        payload: &mut TimerPayload,
    ) -> Option<(TimerFsmEvent, TimerPayload)> {
        match state {
            TimerFsmState::Waiting | TimerFsmState::Counting if payload.ticks < 3 => {
                Some((TimerFsmEvent::Tick, TimerPayload::default()))
            }
            TimerFsmState::Counting if payload.ticks >= 3 => {
                Some((TimerFsmEvent::Done, TimerPayload::default()))
            }
            _ => None,
        }
    }
}

// ============================================================================
// FinitomataSupervisor tests
// ============================================================================

#[tokio::test]
async fn test_start_fsm_basic() {
    let graph = WorkflowFsm::build_graph();
    let supervisor = FinitomataSupervisor::<WorkflowFsm>::new("test", graph);

    supervisor
        .start_fsm("fsm1", WorkflowFsm, WorkflowPayload::default())
        .await
        .unwrap();

    assert!(supervisor.alive("fsm1"));
    let state = supervisor.state("fsm1").unwrap();
    assert_eq!(state.current, WorkflowFsmState::Idle);
    assert!(state.payload.started);
    assert_eq!(state.lifecycle, Lifecycle::Running);
}

#[tokio::test]
async fn test_transition_basic() {
    let graph = WorkflowFsm::build_graph();
    let supervisor = FinitomataSupervisor::<WorkflowFsm>::new("test", graph);

    supervisor
        .start_fsm("fsm1", WorkflowFsm, WorkflowPayload::default())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(20)).await;

    supervisor
        .transition("fsm1", WorkflowFsmEvent::Start, WorkflowPayload::default())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    let state = supervisor.state("fsm1").unwrap();
    assert_eq!(state.current, WorkflowFsmState::Running);
    assert!(state.payload.transitions.contains(&"Start".to_string()));
}

#[tokio::test]
async fn test_transition_sequence() {
    let graph = WorkflowFsm::build_graph();
    let supervisor = FinitomataSupervisor::<WorkflowFsm>::new("test", graph);

    supervisor
        .start_fsm("fsm1", WorkflowFsm, WorkflowPayload::default())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(20)).await;

    let ep = WorkflowPayload::default();
    supervisor
        .transition("fsm1", WorkflowFsmEvent::Start, ep.clone())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(30)).await;

    supervisor
        .transition("fsm1", WorkflowFsmEvent::Pause, ep.clone())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(30)).await;

    supervisor
        .transition("fsm1", WorkflowFsmEvent::Resume, ep.clone())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(30)).await;

    supervisor
        .transition("fsm1", WorkflowFsmEvent::Stop, ep.clone())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(30)).await;

    let state = supervisor.state("fsm1").unwrap();
    assert_eq!(state.current, WorkflowFsmState::Idle);
    assert_eq!(state.payload.transitions.len(), 4);
}

#[tokio::test]
async fn test_transition_to_nonexistent_fsm() {
    let graph = WorkflowFsm::build_graph();
    let supervisor = FinitomataSupervisor::<WorkflowFsm>::new("test", graph);

    let result = supervisor
        .transition(
            "nonexistent",
            WorkflowFsmEvent::Start,
            WorkflowPayload::default(),
        )
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_current_state() {
    let graph = WorkflowFsm::build_graph();
    let supervisor = FinitomataSupervisor::<WorkflowFsm>::new("test", graph);

    supervisor
        .start_fsm("fsm1", WorkflowFsm, WorkflowPayload::default())
        .await
        .unwrap();

    assert_eq!(
        supervisor.current_state("fsm1"),
        Some(WorkflowFsmState::Idle)
    );
    assert_eq!(supervisor.current_state("missing"), None);
}

#[tokio::test]
async fn test_alive() {
    let graph = WorkflowFsm::build_graph();
    let supervisor = FinitomataSupervisor::<WorkflowFsm>::new("test", graph);

    assert!(!supervisor.alive("fsm1"));

    supervisor
        .start_fsm("fsm1", WorkflowFsm, WorkflowPayload::default())
        .await
        .unwrap();

    assert!(supervisor.alive("fsm1"));
    assert!(!supervisor.alive("fsm2"));
}

#[tokio::test]
async fn test_all() {
    let graph = WorkflowFsm::build_graph();
    let supervisor = FinitomataSupervisor::<WorkflowFsm>::new("test", graph);

    supervisor
        .start_fsm("fsm1", WorkflowFsm, WorkflowPayload::default())
        .await
        .unwrap();
    supervisor
        .start_fsm("fsm2", WorkflowFsm, WorkflowPayload::default())
        .await
        .unwrap();

    let all = supervisor.all();
    assert_eq!(all.len(), 2);
}

#[tokio::test]
async fn test_shutdown() {
    let graph = WorkflowFsm::build_graph();
    let supervisor = FinitomataSupervisor::<WorkflowFsm>::new("test", graph);

    supervisor
        .start_fsm("fsm1", WorkflowFsm, WorkflowPayload::default())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(20)).await;

    supervisor.shutdown("fsm1").await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    let state = supervisor.state("fsm1").unwrap();
    assert_eq!(state.lifecycle, Lifecycle::Terminated);
    assert!(state.payload.stopped);
}

#[tokio::test]
async fn test_shutdown_nonexistent() {
    let graph = WorkflowFsm::build_graph();
    let supervisor = FinitomataSupervisor::<WorkflowFsm>::new("test", graph);

    let result = supervisor.shutdown("nonexistent").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_auto_terminate_on_final_state() {
    let graph = WorkflowFsm::build_graph();
    let supervisor =
        FinitomataSupervisor::<WorkflowFsm>::new("test", graph).with_auto_terminate(true);

    supervisor
        .start_fsm("fsm1", WorkflowFsm, WorkflowPayload::default())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(20)).await;

    let ep = WorkflowPayload::default();
    supervisor
        .transition("fsm1", WorkflowFsmEvent::Shutdown, ep.clone())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    let state = supervisor.state("fsm1").unwrap();
    assert_eq!(state.current, WorkflowFsmState::Off);
    assert_eq!(state.lifecycle, Lifecycle::Terminated);
}

#[tokio::test]
async fn test_id_and_system() {
    let graph = WorkflowFsm::build_graph();
    let supervisor = FinitomataSupervisor::<WorkflowFsm>::new("my_sup", graph);

    assert_eq!(supervisor.id(), "my_sup");
    let _system = supervisor.system();
}

// ============================================================================
// Hard event tests
// ============================================================================

#[tokio::test]
async fn test_hard_event_auto_fires_on_start() {
    let graph = HardEventFsm::build_graph();
    let supervisor = FinitomataSupervisor::<HardEventFsm>::new("test", graph);

    supervisor
        .start_fsm("fsm1", HardEventFsm, HardPayload::default())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Hard event `boot!` should have auto-fired, moving from init → ready
    let state = supervisor.state("fsm1").unwrap();
    assert_eq!(state.current, HardEventFsmState::Ready);
    assert_eq!(state.payload.boot_count, 1);
}

// ============================================================================
// Soft event tests
// ============================================================================

#[tokio::test]
async fn test_soft_event_silently_skipped_in_wrong_state() {
    let graph = SoftEventFsm::build_graph();
    let supervisor = FinitomataSupervisor::<SoftEventFsm>::new("test", graph);

    supervisor
        .start_fsm("fsm1", SoftEventFsm, SoftPayload::default())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(20)).await;

    // `tick?` is a soft event — it should silently skip when in idle state
    supervisor
        .transition("fsm1", SoftEventFsmEvent::Tick, SoftPayload::default())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(30)).await;

    let state = supervisor.state("fsm1").unwrap();
    assert_eq!(state.current, SoftEventFsmState::Idle);
    assert_eq!(state.payload.tick_count, 0);
}

#[tokio::test]
async fn test_soft_event_works_in_correct_state() {
    let graph = SoftEventFsm::build_graph();
    let supervisor = FinitomataSupervisor::<SoftEventFsm>::new("test", graph);

    supervisor
        .start_fsm("fsm1", SoftEventFsm, SoftPayload::default())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(20)).await;

    let ep = SoftPayload::default();
    supervisor
        .transition("fsm1", SoftEventFsmEvent::Start, ep.clone())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(30)).await;

    supervisor
        .transition("fsm1", SoftEventFsmEvent::Tick, ep.clone())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(30)).await;

    let state = supervisor.state("fsm1").unwrap();
    assert_eq!(state.current, SoftEventFsmState::Running);
    assert_eq!(state.payload.tick_count, 1);
}

// ============================================================================
// Timer tests
// ============================================================================

#[tokio::test]
async fn test_timer_triggers_on_timer() {
    let graph = TimerFsm::build_graph();
    let supervisor =
        FinitomataSupervisor::<TimerFsm>::new("test", graph).with_timer(Duration::from_millis(50));

    supervisor
        .start_fsm("fsm1", TimerFsm, TimerPayload::default())
        .await
        .unwrap();

    // Wait for timer ticks to accumulate
    tokio::time::sleep(Duration::from_millis(400)).await;

    let state = supervisor.state("fsm1").unwrap();
    // Timer should have caused ticks and eventually done
    assert!(state.payload.ticks >= 3);
    assert_eq!(state.current, TimerFsmState::Finished);
}

// ============================================================================
// Persistence tests
// ============================================================================

#[tokio::test]
async fn test_persistence_stores_on_transition() {
    let graph = WorkflowFsm::build_graph();
    let persistency = InMemoryPersistency::<WorkflowFsm>::new();
    let persistency_clone = Arc::new(persistency);
    let persist_ref = persistency_clone.clone();

    let supervisor = FinitomataSupervisor::<WorkflowFsm>::new("test", graph)
        .with_persistency(PersistencyWrapper(persist_ref.clone()));

    supervisor
        .start_fsm("fsm1", WorkflowFsm, WorkflowPayload::default())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(20)).await;

    supervisor
        .transition("fsm1", WorkflowFsmEvent::Start, WorkflowPayload::default())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Check that persistence has the updated state
    let loaded = persist_ref.load("fsm1").await.unwrap().unwrap();
    assert_eq!(loaded.1, WorkflowFsmState::Running);
}

// Wrapper to allow Arc<InMemoryPersistency> to implement Persistency
struct PersistencyWrapper<F: Finitomata>(Arc<InMemoryPersistency<F>>);

#[async_trait]
impl Persistency<WorkflowFsm> for PersistencyWrapper<WorkflowFsm> {
    async fn load(
        &self,
        id: &str,
    ) -> Result<Option<(Lifecycle, WorkflowFsmState, WorkflowPayload)>, finitomata::PersistencyError>
    {
        self.0.load(id).await
    }

    async fn store(
        &self,
        id: &str,
        state: &WorkflowFsmState,
        payload: &WorkflowPayload,
    ) -> Result<(), finitomata::PersistencyError> {
        self.0.store(id, state, payload).await
    }

    async fn store_error(
        &self,
        id: &str,
        error: &FinitomataError,
    ) -> Result<(), finitomata::PersistencyError> {
        self.0.store_error(id, error).await
    }
}

#[tokio::test]
async fn test_persistence_recovery_on_start() {
    let graph = WorkflowFsm::build_graph();
    let persistency = InMemoryPersistency::<WorkflowFsm>::new();

    // Pre-store a state
    let payload = WorkflowPayload {
        started: false,
        stopped: false,
        transitions: vec!["Start".into(), "Pause".into()],
    };
    persistency
        .store("fsm1", &WorkflowFsmState::Paused, &payload)
        .await
        .unwrap();

    let supervisor =
        FinitomataSupervisor::<WorkflowFsm>::new("test", graph).with_persistency(persistency);

    // start_fsm should recover from persistence
    supervisor
        .start_fsm("fsm1", WorkflowFsm, WorkflowPayload::default())
        .await
        .unwrap();

    let state = supervisor.state("fsm1").unwrap();
    assert_eq!(state.current, WorkflowFsmState::Paused);
    assert_eq!(state.payload.transitions.len(), 2);
}

// ============================================================================
// Listener tests
// ============================================================================

#[tokio::test]
async fn test_listener_receives_transitions() {
    let graph = WorkflowFsm::build_graph();
    let listener = RecordingListener::new();
    let listener_clone = listener.clone();

    let supervisor =
        FinitomataSupervisor::<WorkflowFsm>::new("test", graph).with_listener(listener);

    supervisor
        .start_fsm("fsm1", WorkflowFsm, WorkflowPayload::default())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(20)).await;

    supervisor
        .transition("fsm1", WorkflowFsmEvent::Start, WorkflowPayload::default())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    let events = listener_clone.events.lock().await;
    assert!(!events.is_empty());
    assert_eq!(events[0], "fsm1: Idle -> Running via Start");
}

#[derive(Clone)]
struct RecordingListener {
    events: Arc<Mutex<Vec<String>>>,
}

impl RecordingListener {
    fn new() -> Self {
        Self {
            events: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

#[async_trait]
impl finitomata::Listener<WorkflowFsm> for RecordingListener {
    async fn on_transition(
        &self,
        name: &str,
        from: &WorkflowFsmState,
        to: &WorkflowFsmState,
        event: &WorkflowFsmEvent,
    ) {
        let msg = format!("{name}: {from:?} -> {to:?} via {event:?}");
        self.events.lock().await.push(msg);
    }
}

// ============================================================================
// spawn_supervised tests
// ============================================================================

#[tokio::test]
async fn test_spawn_supervised_basic() {
    let graph = WorkflowFsm::build_graph();
    let supervisor = FinitomataSupervisor::<WorkflowFsm>::new("test", graph)
        .with_persistency(InMemoryPersistency::new());

    supervisor
        .spawn_supervised(
            "fsm1",
            WorkflowFsm,
            WorkflowPayload::default(),
            RestartStrategy::OneForOne,
        )
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    assert!(supervisor.alive("fsm1"));
    let state = supervisor.state("fsm1").unwrap();
    assert_eq!(state.current, WorkflowFsmState::Idle);
}

#[tokio::test]
async fn test_spawn_supervised_transition() {
    let graph = WorkflowFsm::build_graph();
    let supervisor = FinitomataSupervisor::<WorkflowFsm>::new("test", graph)
        .with_persistency(InMemoryPersistency::new());

    supervisor
        .spawn_supervised(
            "fsm1",
            WorkflowFsm,
            WorkflowPayload::default(),
            RestartStrategy::OneForOne,
        )
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    supervisor
        .transition("fsm1", WorkflowFsmEvent::Start, WorkflowPayload::default())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    let state = supervisor.state("fsm1").unwrap();
    assert_eq!(state.current, WorkflowFsmState::Running);
}

// ============================================================================
// Graph API tests (build_graph macro output)
// ============================================================================

#[test]
fn test_build_graph_initial_state() {
    let graph = WorkflowFsm::build_graph();
    assert_eq!(*graph.initial_state(), WorkflowFsmState::Idle);
}

#[test]
fn test_build_graph_final_states() {
    let graph = WorkflowFsm::build_graph();
    assert!(graph.is_final(&WorkflowFsmState::Off));
    assert!(!graph.is_final(&WorkflowFsmState::Running));
}

#[test]
fn test_build_graph_responds() {
    let graph = WorkflowFsm::build_graph();
    assert!(graph.responds(&WorkflowFsmState::Idle, &WorkflowFsmEvent::Start));
    assert!(graph.responds(&WorkflowFsmState::Running, &WorkflowFsmEvent::Pause));
    assert!(!graph.responds(&WorkflowFsmState::Idle, &WorkflowFsmEvent::Pause));
}

#[test]
fn test_build_graph_allowed() {
    let graph = WorkflowFsm::build_graph();
    let (targets, kind) = graph
        .allowed(&WorkflowFsmState::Idle, &WorkflowFsmEvent::Start)
        .unwrap();
    assert!(targets.contains(&WorkflowFsmState::Running));
    assert_eq!(kind, EventKind::Normal);
}

#[test]
fn test_build_graph_events_for() {
    let graph = WorkflowFsm::build_graph();
    let events = graph.events_for(&WorkflowFsmState::Running);
    let event_names: Vec<_> = events.iter().map(|(e, _)| format!("{e:?}")).collect();
    assert!(event_names.contains(&"Pause".to_string()));
    assert!(event_names.contains(&"Stop".to_string()));
}

#[test]
fn test_build_graph_all_states() {
    let graph = WorkflowFsm::build_graph();
    let states = graph.all_states();
    assert!(states.contains(&WorkflowFsmState::Idle));
    assert!(states.contains(&WorkflowFsmState::Running));
    assert!(states.contains(&WorkflowFsmState::Paused));
    assert!(states.contains(&WorkflowFsmState::Off));
}

#[test]
fn test_build_graph_all_events() {
    let graph = WorkflowFsm::build_graph();
    let events = graph.all_events();
    assert!(events.contains(&&WorkflowFsmEvent::Start));
    assert!(events.contains(&&WorkflowFsmEvent::Stop));
}

#[test]
fn test_build_graph_shortest_path() {
    let graph = WorkflowFsm::build_graph();
    let path = graph
        .shortest_path(&WorkflowFsmState::Idle, &WorkflowFsmState::Paused)
        .unwrap();
    assert_eq!(path.len(), 2); // idle->start->running, running->pause->paused
}

#[test]
fn test_build_graph_validate() {
    let graph = WorkflowFsm::build_graph();
    assert!(graph.validate().is_ok());
}

// ============================================================================
// Error type tests
// ============================================================================

#[test]
fn test_error_not_responds() {
    let err = FinitomataError::not_responds("idle", "finish");
    assert_eq!(err.stage, finitomata::error::ErrorStage::NotResponds);
    assert!(err.reason.contains("idle"));
    assert!(err.reason.contains("finish"));
    assert_eq!(err.state, Some("idle".to_string()));
    assert_eq!(err.event, Some("finish".to_string()));
}

#[test]
fn test_error_not_allowed() {
    let err = FinitomataError::not_allowed("idle", "done", "jump");
    assert_eq!(err.stage, finitomata::error::ErrorStage::NotAllowed);
    assert!(err.reason.contains("idle"));
    assert!(err.reason.contains("done"));
}

#[test]
fn test_error_display() {
    let err = FinitomataError::validation("something went wrong");
    let display = format!("{err}");
    assert!(display.contains("validation"));
    assert!(display.contains("something went wrong"));
}

// ============================================================================
// Cache tests
// ============================================================================

#[test]
fn test_cache_operations() {
    use finitomata::cache::StateCache;

    let cache: StateCache<&str, u32> = StateCache::new();
    assert!(cache.is_empty());
    assert_eq!(cache.len(), 0);

    let state = FsmState::new("test1", "idle", 42);
    cache.update(&state);

    assert!(!cache.is_empty());
    assert_eq!(cache.len(), 1);
    assert!(cache.contains("test1"));
    assert!(!cache.contains("test2"));

    assert_eq!(cache.get_state("test1"), Some("idle"));
    assert_eq!(cache.get_payload("test1"), Some(42));

    let full = cache.get("test1").unwrap();
    assert_eq!(full.current, "idle");
    assert_eq!(full.payload, 42);

    cache.remove("test1");
    assert!(cache.is_empty());
    assert!(cache.get("test1").is_none());
}

#[test]
fn test_cache_all() {
    use finitomata::cache::StateCache;

    let cache: StateCache<&str, u32> = StateCache::new();
    cache.update(&FsmState::new("a", "idle", 1));
    cache.update(&FsmState::new("b", "running", 2));

    let all = cache.all();
    assert_eq!(all.len(), 2);
}

// ============================================================================
// FsmState tests
// ============================================================================

#[test]
fn test_fsm_state_transition_to() {
    let mut state: FsmState<&str, u32> = FsmState::new("test", "idle", 0);
    state.transition_to("running");
    assert_eq!(state.current, "running");
    assert_eq!(state.history.len(), 2);
}

#[test]
fn test_fsm_state_set_payload() {
    let mut state: FsmState<&str, u32> = FsmState::new("test", "idle", 0);
    state.set_payload(99);
    assert_eq!(state.payload, 99);
}

#[test]
fn test_fsm_state_error_lifecycle() {
    let mut state: FsmState<&str, u32> = FsmState::new("test", "idle", 0);
    assert!(state.last_error.is_none());

    state.set_error(FinitomataError::validation("test error"));
    assert!(state.last_error.is_some());

    state.clear_error();
    assert!(state.last_error.is_none());
}

// ============================================================================
// Lifecycle enum tests
// ============================================================================

#[test]
fn test_lifecycle_variants() {
    assert_ne!(Lifecycle::Created, Lifecycle::Running);
    assert_ne!(Lifecycle::Running, Lifecycle::Terminated);
    assert_eq!(Lifecycle::Created, Lifecycle::Created);
}

// ============================================================================
// Parser tests (runtime parsing)
// ============================================================================

#[test]
fn test_mermaid_parser_runtime() {
    use finitomata::FsmParser;
    use finitomata::parser::mermaid::MermaidParser;

    let input = r#"
        [*] --> idle
        idle --> |work| busy
        busy --> |rest| idle
        idle --> |quit| [*]
    "#;

    let fsm = MermaidParser::parse(input).unwrap();
    assert_eq!(fsm.initial, "idle");
    assert!(fsm.finals.contains(&"idle".to_string()));
    assert_eq!(fsm.transitions.len(), 3);
}

#[test]
fn test_plantuml_parser_runtime() {
    use finitomata::FsmParser;
    use finitomata::parser::plantuml::PlantUmlParser;

    let input = r#"
        [*] --> idle
        idle --> busy : work
        busy --> idle : rest
        idle --> [*] : quit
    "#;

    let fsm = PlantUmlParser::parse(input).unwrap();
    assert_eq!(fsm.initial, "idle");
    assert!(fsm.finals.contains(&"idle".to_string()));
    assert_eq!(fsm.transitions.len(), 3);
}

// ============================================================================
// InMemoryPersistency tests
// ============================================================================

#[tokio::test]
async fn test_in_memory_persistency_operations() {
    let persist = InMemoryPersistency::<WorkflowFsm>::new();
    assert!(persist.is_empty());
    assert_eq!(persist.len(), 0);

    persist
        .store(
            "test1",
            &WorkflowFsmState::Running,
            &WorkflowPayload::default(),
        )
        .await
        .unwrap();

    assert!(!persist.is_empty());
    assert_eq!(persist.len(), 1);
    assert!(persist.contains("test1"));

    let loaded = persist.load("test1").await.unwrap().unwrap();
    assert_eq!(loaded.0, Lifecycle::Running);
    assert_eq!(loaded.1, WorkflowFsmState::Running);

    let removed = persist.remove("test1").unwrap();
    assert_eq!(removed.1, WorkflowFsmState::Running);
    assert!(persist.is_empty());
}

// ============================================================================
// Multiple FSMs on the same supervisor
// ============================================================================

#[tokio::test]
async fn test_multiple_fsms_independent() {
    let graph = WorkflowFsm::build_graph();
    let supervisor = FinitomataSupervisor::<WorkflowFsm>::new("test", graph);

    supervisor
        .start_fsm("a", WorkflowFsm, WorkflowPayload::default())
        .await
        .unwrap();
    supervisor
        .start_fsm("b", WorkflowFsm, WorkflowPayload::default())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(20)).await;

    let ep = WorkflowPayload::default();
    supervisor
        .transition("a", WorkflowFsmEvent::Start, ep.clone())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(30)).await;

    // a should be running, b should still be idle
    assert_eq!(
        supervisor.current_state("a"),
        Some(WorkflowFsmState::Running)
    );
    assert_eq!(supervisor.current_state("b"), Some(WorkflowFsmState::Idle));
}
