use async_trait::async_trait;
use finitomata::{
    Finitomata, FinitomataSupervisor, RestartStrategy, TransitionResult, finitomata,
    persistency::memory::InMemoryPersistency,
};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

static CRASH_COUNTER: AtomicU32 = AtomicU32::new(0);

#[finitomata(
    fsm = r#"
        [*] --> idle
        idle --> |process| working
        working --> |complete| idle
        working --> |fail| error
        error --> |retry| idle
        idle --> |shutdown| off
        off --> |confirm| [*]
    "#,
    syntax = "mermaid",
    auto_terminate = true
)]
#[derive(Debug, Clone, Default)]
struct Worker;

#[derive(Debug, Clone)]
struct WorkerPayload {
    task_name: String,
    attempts: u32,
    completed: u32,
}

#[async_trait]
impl Finitomata for Worker {
    type State = WorkerState;
    type Event = WorkerEvent;
    type Payload = WorkerPayload;

    async fn on_transition(
        &mut self,
        _from: &WorkerState,
        event: &WorkerEvent,
        _event_payload: &WorkerPayload,
        state_payload: &mut WorkerPayload,
    ) -> TransitionResult<WorkerState, WorkerPayload> {
        match event {
            WorkerEvent::Process => {
                state_payload.attempts += 1;
                println!(
                    "  [{}] Processing (attempt #{})",
                    state_payload.task_name, state_payload.attempts
                );

                // Simulate a crash on the 2nd attempt
                let crash_count = CRASH_COUNTER.fetch_add(1, Ordering::SeqCst);
                if crash_count == 1 {
                    println!("  [{}] CRASH! (simulated panic)", state_payload.task_name);
                    panic!("simulated worker crash");
                }

                TransitionResult::Ok(WorkerState::Working)
            }
            WorkerEvent::Complete => {
                state_payload.completed += 1;
                println!(
                    "  [{}] Completed (total: {})",
                    state_payload.task_name, state_payload.completed
                );
                TransitionResult::Ok(WorkerState::Idle)
            }
            WorkerEvent::Fail => {
                println!(
                    "  [{}] Failed — entering error state",
                    state_payload.task_name
                );
                TransitionResult::Ok(WorkerState::Error)
            }
            WorkerEvent::Retry => {
                println!("  [{}] Retrying from error", state_payload.task_name);
                TransitionResult::Ok(WorkerState::Idle)
            }
            WorkerEvent::Shutdown => {
                println!("  [{}] Shutting down", state_payload.task_name);
                TransitionResult::Ok(WorkerState::Off)
            }
            WorkerEvent::Confirm => TransitionResult::Ok(WorkerState::Off),
        }
    }

    async fn on_enter(&mut self, state: &WorkerState, payload: &mut WorkerPayload) {
        println!("  [{}] → entered: {state}", payload.task_name);
    }

    async fn on_start(&mut self, payload: &mut WorkerPayload) {
        println!("  [{}] on_start called", payload.task_name);
    }

    async fn on_terminate(&mut self, payload: &mut WorkerPayload) {
        println!(
            "  [{}] terminated (attempts: {}, completed: {})",
            payload.task_name, payload.attempts, payload.completed
        );
    }
}

#[tokio::main]
async fn main() {
    println!("=== Finitomata Supervised FSM Example ===\n");
    println!("This demonstrates automatic restart with state recovery after a crash.\n");

    let graph = Worker::build_graph();
    let persistency = InMemoryPersistency::<Worker>::new();

    let supervisor = FinitomataSupervisor::<Worker>::new("worker_sup", graph)
        .with_persistency(persistency)
        .with_auto_terminate(true);

    let payload = WorkerPayload {
        task_name: "data-pipeline".into(),
        attempts: 0,
        completed: 0,
    };

    // Spawn the FSM under a OneForOne supervisor
    println!("--- Spawning supervised FSM ---");
    supervisor
        .spawn_supervised("worker_1", Worker, payload, RestartStrategy::OneForOne)
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    // First transition: process (succeeds)
    println!("\n--- First task (will succeed) ---");
    let ep = WorkerPayload {
        task_name: String::new(),
        attempts: 0,
        completed: 0,
    };
    supervisor
        .transition("worker_1", WorkerEvent::Process, ep.clone())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Complete the first task
    supervisor
        .transition("worker_1", WorkerEvent::Complete, ep.clone())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    println!("\n--- Second task (will crash, then auto-recover) ---");
    // This will panic inside on_transition, causing the actor to crash.
    // The supervisor should restart it, recovering state from persistence.
    let _ = supervisor
        .transition("worker_1", WorkerEvent::Process, ep.clone())
        .await;
    // Give time for crash detection + restart + recovery
    tokio::time::sleep(Duration::from_millis(500)).await;

    // After recovery, the FSM should be back in its last persisted state
    if let Some(state) = supervisor.state("worker_1") {
        println!("\n--- State after recovery ---");
        println!("  Current state: {:?}", state.current);
        println!("  Payload: {:?}", state.payload);
        println!("  Lifecycle: {:?}", state.lifecycle);
    }

    // Send another transition to prove the FSM is alive after recovery
    println!("\n--- Third task (post-recovery, will succeed) ---");
    let result = supervisor
        .transition("worker_1", WorkerEvent::Process, ep.clone())
        .await;
    match result {
        Ok(()) => println!("  Transition sent successfully"),
        Err(e) => println!("  Transition failed: {e}"),
    }
    tokio::time::sleep(Duration::from_millis(100)).await;

    if let Some(state) = supervisor.state("worker_1") {
        println!("\n--- Final state ---");
        println!("  Current state: {:?}", state.current);
        println!("  Payload: {:?}", state.payload);
    }

    println!("\n=== Done ===");
}
