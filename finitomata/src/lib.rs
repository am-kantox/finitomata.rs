//! Finitomata — finite state machines with rich lifecycle callbacks, persistence, and supervision.
//!
//! A Rust port of the [Finitomata](https://github.com/am-kantox/finitomata) Elixir library,
//! providing compile-time validated FSMs with actor-based supervision via
//! [joerl](https://crates.io/crates/joerl).
//!
//! # Quick Start
//!
//! ```rust,ignore
//! use finitomata::{finitomata, Finitomata, FinitomataSupervisor, TransitionResult};
//!
//! #[finitomata(
//!     fsm = r#"
//!         [*] --> idle
//!         idle --> |start| running
//!         running --> |stop| idle
//!         idle --> |shutdown| off
//!         off --> |confirm| [*]
//!     "#,
//!     syntax = "mermaid",
//!     auto_terminate = true
//! )]
//! #[derive(Debug, Clone, Default)]
//! struct MyFsm;
//!
//! #[async_trait::async_trait]
//! impl Finitomata for MyFsm {
//!     type State = MyFsmState;
//!     type Event = MyFsmEvent;
//!     type Payload = String;
//!
//!     async fn on_transition(
//!         &mut self, from: &MyFsmState, event: &MyFsmEvent,
//!         _ep: &String, _sp: &mut String,
//!     ) -> TransitionResult<MyFsmState, String> {
//!         // transition logic here
//!         TransitionResult::Ok(MyFsmState::Running)
//!     }
//! }
//! ```

pub mod cache;
pub mod callbacks;
pub mod engine;
pub mod error;
pub mod listener;
pub mod parser;
pub mod persistency;
pub mod state;
pub mod supervisor;
pub mod timer;
pub mod transition;

pub use callbacks::{Finitomata, TransitionResult};
pub use engine::{TransitContext, TransitOutcome, transit};
pub use error::{FinitomataError, PersistencyError, ValidationError};
pub use listener::{Listener, NoopListener, TracingListener};
pub use parser::{FsmParser, ParseError, ParsedFsm, ParsedTransition};
pub use persistency::Persistency;
pub use state::{BoundedHistory, FsmState, Lifecycle};
pub use supervisor::FinitomataSupervisor;
pub use timer::FsmTimer;
pub use transition::{EventKind, Transition, TransitionGraph};

pub use finitomata_macro::finitomata;

pub use joerl::supervisor::{ChildSpec, RestartIntensity, RestartStrategy, SupervisorSpec};
pub use joerl::{ActorSystem, Pid};
