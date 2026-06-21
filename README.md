# Finitomata for Rust — Documentation

A Rust port of the [Finitomata](https://github.com/am-kantox/finitomata) Elixir library, providing compile-time validated finite state machines with rich lifecycle callbacks, persistence, and actor-based supervision via [joerl](https://crates.io/crates/joerl).

## Table of Contents

- [Overview](#overview)
- [Architecture](#architecture)
- [Quick Start](#quick-start)
- [The `#[finitomata]` Proc Macro](#the-finitomata-proc-macro)
- [The `Finitomata` Trait](#the-finitomata-trait)
- [Supervisor API](#supervisor-api)
- [Transition Graph](#transition-graph)
- [Event Kinds](#event-kinds)
- [Persistence](#persistence)
- [Listeners](#listeners)
- [Timers](#timers)
- [Supervised FSMs (Fault Tolerance)](#supervised-fsms-fault-tolerance)
- [State Cache](#state-cache)
- [Error Handling](#error-handling)
- [Mermaid Syntax](#mermaid-syntax)
- [PlantUML Syntax](#plantuml-syntax)
- [API Reference](#api-reference)

---

## Overview

Finitomata provides:

- **Compile-time validation** — FSM definitions are parsed and validated by a proc macro. Invalid graphs (unreachable states, missing initial/final states) produce compile errors.
- **Rich lifecycle callbacks** — `on_start`, `on_enter`, `on_exit`, `on_transition`, `on_failure`, `on_timer`, `on_terminate`.
- **Actor-based execution** — Each FSM instance runs as a joerl actor with its own mailbox, enabling concurrent independent operation.
- **Fault-tolerant supervision** — FSMs can be spawned under a joerl supervisor with automatic restart and state recovery from persistence.
- **Persistence** — Trait-based pluggable storage. Ships with an in-memory backend; implement `Persistency` for databases, Redis, etc.
- **Timers** — Configurable recurring timers that invoke `on_timer` for periodic state-driven logic.
- **Listeners** — Observer pattern for external telemetry/logging of transitions.

---

## Architecture

```
finitomata.rust/
├── finitomata/               # Main library crate
│   ├── src/
│   │   ├── lib.rs            # Public re-exports
│   │   ├── callbacks.rs      # Finitomata trait (user implements)
│   │   ├── engine.rs         # Transition orchestration
│   │   ├── supervisor.rs     # FinitomataSupervisor (joerl-backed)
│   │   ├── transition.rs     # TransitionGraph, validation, pathfinding
│   │   ├── state.rs          # FsmState, Lifecycle, BoundedHistory
│   │   ├── error.rs          # Error types
│   │   ├── cache.rs          # DashMap-backed state cache
│   │   ├── timer.rs          # FsmTimer (tokio interval)
│   │   ├── listener.rs       # Listener trait + implementations
│   │   ├── persistency/      # Persistency trait + InMemoryPersistency
│   │   ├── parser/           # Mermaid + PlantUML parsers
│   │   └── examples/         # Runnable examples
└── finitomata_macro/         # Proc macro crate (#[finitomata])
```

---

## Quick Start

```rust
use async_trait::async_trait;
use finitomata::{finitomata, Finitomata, FinitomataSupervisor, TransitionResult};
use std::time::Duration;

#[finitomata(
    fsm = r#"
        [*] --> idle
        idle --> |start| running
        running --> |stop| idle
        idle --> |shutdown| off
        off --> |confirm| [*]
    "#,
    syntax = "mermaid",
    auto_terminate = true
)]
#[derive(Debug, Clone, Default)]
struct MyFsm;

#[derive(Debug, Clone)]
struct Payload { counter: u32 }

#[async_trait]
impl Finitomata for MyFsm {
    type State = MyFsmState;
    type Event = MyFsmEvent;
    type Payload = Payload;

    async fn on_transition(
        &mut self,
        _from: &MyFsmState,
        event: &MyFsmEvent,
        _event_payload: &Payload,
        state_payload: &mut Payload,
    ) -> TransitionResult<MyFsmState, Payload> {
        match event {
            MyFsmEvent::Start => {
                state_payload.counter += 1;
                TransitionResult::Ok(MyFsmState::Running)
            }
            MyFsmEvent::Stop => TransitionResult::Ok(MyFsmState::Idle),
            MyFsmEvent::Shutdown => TransitionResult::Ok(MyFsmState::Off),
            MyFsmEvent::Confirm => TransitionResult::Ok(MyFsmState::Off),
        }
    }
}

#[tokio::main]
async fn main() {
    let graph = MyFsm::build_graph();
    let supervisor = FinitomataSupervisor::<MyFsm>::new("my_sup", graph)
        .with_auto_terminate(true);

    supervisor.start_fsm("instance_1", MyFsm, Payload { counter: 0 }).await.unwrap();

    // Send events
    supervisor.transition("instance_1", MyFsmEvent::Start, Payload { counter: 0 }).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Query state (from cache, no actor round-trip)
    let state = supervisor.state("instance_1").unwrap();
    println!("Current: {:?}, Counter: {}", state.current, state.payload.counter);
}
```

---

## The `#[finitomata]` Proc Macro

The `#[finitomata]` attribute macro parses an FSM definition at compile time, validates the graph, and generates:

1. A **State enum** (e.g., `MyFsmState`) with variants for each state
2. An **Event enum** (e.g., `MyFsmEvent`) with variants for each event
3. A `build_graph()` associated function that returns a `TransitionGraph`

### Attributes

| Attribute | Type | Default | Description |
|-----------|------|---------|-------------|
| `fsm` | string (required) | — | The FSM definition in Mermaid or PlantUML syntax |
| `syntax` | `"mermaid"` or `"plantuml"` | `"mermaid"` | Which parser to use |
| `auto_terminate` | `bool` | `false` | Auto-terminate on reaching a final state |
| `timer` | integer (ms) | — | Timer interval in milliseconds |

### Compile-Time Validation

The macro rejects:
- Definitions with no initial state (`[*] --> ...`)
- Definitions with no final state (`... --> [*]`)
- Unreachable states
- Empty definitions

---

## The `Finitomata` Trait

The core trait that users implement to define FSM behavior.

```rust
#[async_trait]
pub trait Finitomata: Send + Sync + 'static {
    type State: Clone + Send + Sync + Eq + Hash + Debug + Ord;
    type Event: Clone + Send + Sync + Eq + Hash + Debug + Ord;
    type Payload: Clone + Send + Sync + Debug;

    // Required
    async fn on_transition(
        &mut self, from: &Self::State, event: &Self::Event,
        event_payload: &Self::Payload, state_payload: &mut Self::Payload,
    ) -> TransitionResult<Self::State, Self::Payload>;

    // Optional lifecycle hooks (all have default no-op implementations)
    async fn on_start(&mut self, payload: &mut Self::Payload) {}
    async fn on_enter(&mut self, state: &Self::State, payload: &mut Self::Payload) {}
    async fn on_exit(&mut self, state: &Self::State, payload: &mut Self::Payload) {}
    async fn on_failure(&mut self, event: &Self::Event, reason: &FinitomataError, payload: &mut Self::Payload) {}
    async fn on_timer(&mut self, state: &Self::State, payload: &mut Self::Payload) -> Option<(Self::Event, Self::Payload)> { None }
    async fn on_terminate(&mut self, payload: &mut Self::Payload) {}
}
```

### `TransitionResult`

```rust
pub enum TransitionResult<S, P> {
    Ok(S),                   // Move to state S, keep current payload
    OkWithPayload(S, P),     // Move to state S with a new payload
    Error(FinitomataError),  // Abort — triggers on_failure, no state change
}
```

### Callback Sequence

For every transition, the engine executes this sequence:

1. Validate: does current state respond to event? (soft events skip silently)
2. `on_exit(current_state, payload)`
3. `on_transition(from, event, event_payload, payload)` → target state
4. Validate: is target in the graph's allowed set?
5. Persist (if configured)
6. Update state + history
7. Notify listener
8. `on_enter(new_state, payload)`
9. If `auto_terminate` and target is final → `on_terminate` and stop
10. If event is Hard → immediately fire next hard event

---

## Supervisor API

`FinitomataSupervisor<F>` is the high-level API for managing FSM instances.

### Construction (Builder Pattern)

```rust
let supervisor = FinitomataSupervisor::<MyFsm>::new("my_sup", graph)
    .with_persistency(InMemoryPersistency::new())  // Enable persistence
    .with_listener(TracingListener)                 // Log transitions
    .with_auto_terminate(true)                      // Stop on final state
    .with_timer(Duration::from_secs(5));            // Recurring timer
```

### Methods

| Method | Description |
|--------|-------------|
| `start_fsm(name, fsm, payload)` | Spawn an unsupervised FSM actor |
| `spawn_supervised(name, fsm, payload, strategy)` | Spawn under a fault-tolerant supervisor |
| `spawn_supervised_with_intensity(name, fsm, payload, strategy, intensity)` | Same with custom restart limits |
| `transition(name, event, payload)` | Send an event to trigger a transition |
| `state(name)` | Get full state snapshot (from cache) |
| `current_state(name)` | Get just the current state enum value |
| `alive(name)` | Check if FSM is in Running lifecycle |
| `all()` | List all managed FSMs |
| `shutdown(name)` | Graceful shutdown (calls on_terminate) |
| `system()` | Access the underlying joerl ActorSystem |
| `id()` | Get the supervisor's ID |

---

## Transition Graph

`TransitionGraph<S, E>` holds the validated FSM structure.

### Key Methods

```rust
graph.initial_state()           // → &S
graph.final_states()            // → &BTreeSet<S>
graph.is_final(&state)          // → bool
graph.responds(&state, &event)  // → bool
graph.allowed(&state, &event)   // → Option<(&[S], EventKind)>
graph.events_for(&state)        // → Vec<(&E, EventKind)>
graph.all_states()              // → BTreeSet<S>
graph.all_events()              // → BTreeSet<&E>
graph.shortest_path(&from, &to) // → Option<Vec<(E, S)>>
graph.validate()                // → Result<(), Vec<ValidationError>>
```

---

## Event Kinds

Events can have special suffixes that modify their behavior:

| Suffix | Kind | Behavior |
|--------|------|----------|
| (none) | `Normal` | Standard event — must be explicitly triggered |
| `!` | `Hard` | Auto-fires immediately when entering the source state |
| `?` | `Soft` | Silently ignored if the current state doesn't respond |

### Hard Events

```
[*] --> init
init --> |boot!| ready    ← fires automatically when entering init
ready --> |go| active
```

After entering `init`, the `boot!` event fires immediately without any external trigger. This is equivalent to Elixir Finitomata's "determined transitions."

### Soft Events

```
running --> |tick?| running   ← no error if sent while in idle
```

Sending `tick?` to an FSM in `idle` state silently succeeds (no-op) instead of returning an error.

---

## Persistence

### Trait

```rust
#[async_trait]
pub trait Persistency<F: Finitomata>: Send + Sync {
    async fn load(&self, id: &str) -> Result<Option<(Lifecycle, F::State, F::Payload)>, PersistencyError>;
    async fn store(&self, id: &str, state: &F::State, payload: &F::Payload) -> Result<(), PersistencyError>;
    async fn store_error(&self, id: &str, error: &FinitomataError) -> Result<(), PersistencyError>;
}
```

### In-Memory Backend

```rust
use finitomata::persistency::memory::InMemoryPersistency;

let persist = InMemoryPersistency::<MyFsm>::new();
let supervisor = FinitomataSupervisor::new("sup", graph)
    .with_persistency(persist);
```

### Recovery

When `start_fsm` or `spawn_supervised` is called with persistence configured:
1. The system checks `persist.load(name)` for existing state
2. If found, the FSM starts from the persisted state (not the graph's initial state)
3. Every successful transition calls `persist.store(name, state, payload)`

---

## Listeners

Listeners observe transitions for telemetry, logging, or event propagation.

```rust
#[async_trait]
pub trait Listener<F: Finitomata>: Send + Sync {
    async fn on_transition(&self, name: &str, from: &F::State, to: &F::State, event: &F::Event);
}
```

### Built-in Listeners

- `NoopListener` — discards all notifications
- `TracingListener` — emits `tracing::info!` events

### Custom Listener Example

```rust
struct MetricsListener;

#[async_trait]
impl Listener<MyFsm> for MetricsListener {
    async fn on_transition(&self, name: &str, from: &MyFsmState, to: &MyFsmState, event: &MyFsmEvent) {
        metrics::counter!("fsm_transitions", "fsm" => name, "event" => format!("{event:?}")).increment(1);
    }
}
```

---

## Timers

Configure a recurring timer to invoke `on_timer`:

```rust
let supervisor = FinitomataSupervisor::new("sup", graph)
    .with_timer(Duration::from_secs(5));
```

The `on_timer` callback can return `Some((event, payload))` to trigger a transition, or `None` to do nothing:

```rust
async fn on_timer(&mut self, state: &MyState, payload: &mut MyPayload) -> Option<(MyEvent, MyPayload)> {
    if *state == MyState::Waiting && payload.elapsed > 30 {
        Some((MyEvent::Timeout, MyPayload::default()))
    } else {
        None
    }
}
```

For supervised FSMs, timers use joerl's `send_after` mechanism (self-rescheduling on each tick). For unsupervised FSMs, the standalone `FsmTimer` is available.

---

## Supervised FSMs (Fault Tolerance)

`spawn_supervised` wraps an FSM under a joerl supervisor that automatically restarts it on crash.

```rust
use finitomata::RestartStrategy;

supervisor
    .spawn_supervised("worker_1", MyFsm::default(), payload, RestartStrategy::OneForOne)
    .await
    .unwrap();
```

### Restart Strategies

| Strategy | Behavior |
|----------|----------|
| `OneForOne` | Only the crashed FSM is restarted |
| `OneForAll` | All children of the supervisor are restarted |
| `RestForOne` | The crashed FSM and all started after it are restarted |

### Recovery Flow

1. FSM crashes (panic, unrecoverable error)
2. joerl supervisor detects the exit
3. Supervisor calls the factory to create a fresh FSM actor
4. New actor's `started()` hook calls `Persistency::load(name)`
5. If state is found, the FSM resumes from the last persisted state
6. `on_start` and `on_enter` are called on the recovered state
7. The actor re-registers its PID in the joerl registry
8. Subsequent `transition()` calls route to the new PID

### Custom Restart Intensity

```rust
use finitomata::RestartIntensity;

supervisor
    .spawn_supervised_with_intensity(
        "worker_1",
        MyFsm::default(),
        payload,
        RestartStrategy::OneForOne,
        RestartIntensity { max_restarts: 5, within_seconds: 60 },
    )
    .await
    .unwrap();
```

If restarts exceed the limit, the supervisor itself terminates.

---

## State Cache

The `StateCache` stores the latest FSM state in a concurrent `DashMap`, updated after every transition. This enables `supervisor.state(name)` to return immediately without an actor round-trip.

```rust
// These are instant (cache reads):
let state = supervisor.state("my_fsm");
let current = supervisor.current_state("my_fsm");
let is_alive = supervisor.alive("my_fsm");
let all = supervisor.all();
```

---

## Error Handling

### `FinitomataError`

The primary error type with a `stage` field indicating where the error occurred:

| Stage | When |
|-------|------|
| `NotResponds` | Event sent to a state that doesn't handle it |
| `NotAllowed` | `on_transition` returned a target not in the graph |
| `OnTransition` | User code returned `TransitionResult::Error` |
| `OnEnter` | Error during entry callback |
| `OnExit` | Error during exit callback |
| `Persistency` | Storage backend failed |
| `Validation` | General validation/operational error |

### `ValidationError`

Compile-time or runtime graph validation errors:
- `NoInitialState`
- `NoFinalState`
- `UnreachableState(name)`
- `MultipleInitialStates(names)`
- `OrphanState(name)`

---

## Mermaid Syntax

Two flavors are supported:

### Flowchart Style

```
[*] --> idle
idle --> |start| running
running --> |pause| paused
running --> |stop| idle
paused --> |resume| running
idle --> |shutdown| [*]
```

### State Diagram Style

```
[*] --> idle
idle --> running : start
running --> paused : pause
running --> idle : stop
paused --> running : resume
idle --> [*] : shutdown
```

- `[*]` as source = initial state (first occurrence defines the initial)
- `[*]` as target = final state (the source state becomes final)
- Event suffixes: `start!` (hard), `check?` (soft)

---

## PlantUML Syntax

```
[*] --> idle
idle --> running : start
running --> idle : stop
idle --> [*] : shutdown
```

Same semantics as Mermaid state diagram syntax. State declarations (`state "label" as name`) are also supported.

---

## API Reference

### Re-exported Types

From `finitomata`:
- `Finitomata` — core trait
- `TransitionResult` — return type for on_transition
- `FinitomataSupervisor` — high-level supervisor
- `TransitionGraph`, `Transition`, `EventKind`
- `FsmState`, `BoundedHistory`, `Lifecycle`
- `FinitomataError`, `ValidationError`, `PersistencyError`
- `Persistency`, `Listener`, `NoopListener`, `TracingListener`
- `FsmTimer`, `FsmParser`, `ParsedFsm`, `ParsedTransition`, `ParseError`

From `joerl`:
- `ActorSystem`, `Pid`
- `RestartStrategy`, `RestartIntensity`, `SupervisorSpec`, `ChildSpec`

### Crate Features

The crate uses these workspace dependencies:
- `tokio` — async runtime
- `async-trait` — async methods in traits
- `dashmap` — concurrent maps (cache, in-memory persistence)
- `joerl` — actor system and supervision
- `serde` — serialization support for state types
- `tracing` — structured logging
- `thiserror` — error derivation

---

## Examples

Run the included examples:

```bash
cargo run --example turnstile         # Simple lock/unlock FSM
cargo run --example order_workflow    # Multi-state with timer + hard events
cargo run --example persistent_fsm    # Document workflow with persistence
cargo run --example supervised_fsm    # Fault-tolerant FSM with crash recovery
```
