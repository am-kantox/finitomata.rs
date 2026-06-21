//! FSM supervision backed by joerl's actor system.
//!
//! Provides [`FinitomataSupervisor`] — the high-level API for spawning, managing,
//! and querying FSM actor instances. Supports both unsupervised (`start_fsm`) and
//! fault-tolerant supervised (`spawn_supervised`) modes with automatic restart and
//! state recovery from persistence.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::RwLock;

use crate::cache::StateCache;
use crate::callbacks::Finitomata;
use crate::engine::{self, TransitContext, TransitOutcome};
use crate::error::FinitomataError;
use crate::listener::Listener;
use crate::persistency::Persistency;
use crate::state::{FsmState, Lifecycle};
use crate::transition::{EventKind, TransitionGraph};

pub use joerl::supervisor::{
    ChildSpec, RestartIntensity, RestartStrategy, SupervisorSpec, spawn_supervisor,
};
use joerl::system::ActorRef;
use joerl::{Actor, ActorContext, ActorSystem, ExitReason, Message, Pid};

// --- FSM Actor: wraps Finitomata trait as a joerl Actor ---

struct FsmActor<F: Finitomata> {
    fsm: F,
    state: FsmState<F::State, F::Payload>,
    graph: Arc<TransitionGraph<F::State, F::Event>>,
    cache: Arc<StateCache<F::State, F::Payload>>,
    persistency: Option<Arc<dyn Persistency<F>>>,
    listener: Option<Arc<dyn Listener<F>>>,
    auto_terminate: bool,
    timer_interval: Option<Duration>,
    name: String,
    recover_on_start: bool,
    system: Option<Arc<ActorSystem>>,
}

struct TransitionMsg<E: Send + 'static, P: Send + 'static> {
    event: E,
    payload: P,
}

struct TimerTickMsg;
struct ShutdownMsg;

#[async_trait::async_trait]
impl<F: Finitomata + Default> Actor for FsmActor<F>
where
    F::State: Send + Sync + 'static,
    F::Event: Send + Sync + 'static,
    F::Payload: Send + Sync + 'static,
{
    async fn started(&mut self, ctx: &mut ActorContext) {
        if self.recover_on_start {
            if let Some(ref persist) = self.persistency
                && let Ok(Some((lc, recovered_state, recovered_payload))) =
                    persist.load(&self.name).await
            {
                tracing::info!(
                    "FSM `{}` recovered from persistence: state={:?}, lifecycle={:?}",
                    self.name,
                    recovered_state,
                    lc
                );
                self.state = FsmState::new(self.name.clone(), recovered_state, recovered_payload);
                self.state.lifecycle = Lifecycle::Loaded;
                self.fsm.on_start(&mut self.state.payload).await;
                self.fsm
                    .on_enter(&self.state.current, &mut self.state.payload)
                    .await;
                self.state.lifecycle = Lifecycle::Running;
                self.cache.update(&self.state);
            }

            if let Some(ref system) = self.system {
                let _ = system.unregister(&self.name);
                let _ = system.register(&self.name, ctx.pid());
            }
        }

        if let Some(interval) = self.timer_interval {
            ctx.send_after(
                joerl::Destination::Pid(ctx.pid()),
                Box::new(TimerTickMsg),
                interval,
            );
        }

        let hard_event: Option<F::Event> = self
            .graph
            .events_for(&self.state.current)
            .into_iter()
            .find(|(_, kind)| *kind == EventKind::Hard)
            .map(|(evt, _)| evt.clone());

        if let Some(evt) = hard_event {
            let payload_clone = self.state.payload.clone();
            self.do_transition(evt, payload_clone, ctx).await;
        }
    }

    async fn handle_message(&mut self, msg: Message, ctx: &mut ActorContext) {
        if msg.downcast_ref::<TimerTickMsg>().is_some() {
            self.handle_timer_tick(ctx).await;
            if let Some(interval) = self.timer_interval
                && matches!(self.state.lifecycle, Lifecycle::Running)
            {
                ctx.send_after(
                    joerl::Destination::Pid(ctx.pid()),
                    Box::new(TimerTickMsg),
                    interval,
                );
            }
        } else if msg.downcast_ref::<ShutdownMsg>().is_some() {
            self.state.lifecycle = Lifecycle::Terminating;
            self.fsm.on_terminate(&mut self.state.payload).await;
            self.state.lifecycle = Lifecycle::Terminated;
            self.cache.update(&self.state);
            ctx.stop(ExitReason::Normal);
        } else if let Some(transition) = msg.downcast_ref::<TransitionMsg<F::Event, F::Payload>>() {
            let event = transition.event.clone();
            let payload = transition.payload.clone();
            self.do_transition(event, payload, ctx).await;
        }
    }

    async fn stopped(&mut self, _reason: &ExitReason, _ctx: &mut ActorContext) {
        if matches!(self.state.lifecycle, Lifecycle::Running) {
            self.state.lifecycle = Lifecycle::Terminated;
            self.fsm.on_terminate(&mut self.state.payload).await;
            self.cache.update(&self.state);
        }
    }
}

impl<F: Finitomata + Default> FsmActor<F>
where
    F::State: Send + Sync + 'static,
    F::Event: Send + Sync + 'static,
    F::Payload: Send + Sync + 'static,
{
    async fn do_transition(
        &mut self,
        event: F::Event,
        event_payload: F::Payload,
        ctx: &mut ActorContext,
    ) {
        let transit_ctx = TransitContext {
            persistency: self.persistency.as_deref(),
            listener: self.listener.as_deref(),
            auto_terminate: self.auto_terminate,
        };

        match engine::transit(
            &mut self.fsm,
            &self.graph,
            &mut self.state,
            event,
            event_payload,
            &transit_ctx,
        )
        .await
        {
            Ok(TransitOutcome::AutoTerminate) => {
                self.cache.update(&self.state);
                ctx.stop(ExitReason::Normal);
            }
            Ok(TransitOutcome::HardContinue) => {
                self.cache.update(&self.state);
                let hard_event: Option<F::Event> = self
                    .graph
                    .events_for(&self.state.current)
                    .into_iter()
                    .find(|(_, kind)| *kind == EventKind::Hard)
                    .map(|(evt, _)| evt.clone());

                if let Some(evt) = hard_event {
                    let payload_clone = self.state.payload.clone();
                    Box::pin(self.do_transition(evt, payload_clone, ctx)).await;
                }
            }
            Ok(_) => {
                self.cache.update(&self.state);
            }
            Err(_) => {
                self.cache.update(&self.state);
            }
        }
    }

    async fn handle_timer_tick(&mut self, ctx: &mut ActorContext) {
        if let Some((event, event_payload)) = self
            .fsm
            .on_timer(&self.state.current, &mut self.state.payload)
            .await
        {
            self.do_transition(event, event_payload, ctx).await;
        }
    }
}

// --- FinitomataSupervisor: high-level API backed by joerl ---

/// High-level supervisor for managing multiple FSM actor instances.
///
/// `FinitomataSupervisor` is the primary user-facing API for creating and
/// interacting with FSM instances. Each FSM runs as a joerl actor with its
/// own mailbox, enabling concurrent operation of many FSMs.
///
/// # Usage Modes
///
/// - **Unsupervised** ([`start_fsm`](Self::start_fsm)): Spawns a standalone FSM actor.
///   If it crashes, it stays dead.
/// - **Supervised** ([`spawn_supervised`](Self::spawn_supervised)): Wraps the FSM under
///   a joerl `Supervisor` that automatically restarts it on crash, recovering state
///   from persistence.
///
/// # Builder Pattern
///
/// ```rust,ignore
/// let supervisor = FinitomataSupervisor::<MyFsm>::new("my_sup", graph)
///     .with_persistency(InMemoryPersistency::new())
///     .with_listener(TracingListener)
///     .with_auto_terminate(true)
///     .with_timer(Duration::from_secs(5));
/// ```
pub struct FinitomataSupervisor<F: Finitomata> {
    id: String,
    system: Arc<ActorSystem>,
    graph: Arc<TransitionGraph<F::State, F::Event>>,
    cache: Arc<StateCache<F::State, F::Payload>>,
    actors: Arc<RwLock<HashMap<String, Pid>>>,
    persistency: Option<Arc<dyn Persistency<F>>>,
    listener: Option<Arc<dyn Listener<F>>>,
    auto_terminate: bool,
    timer_interval: Option<Duration>,
    _supervisor_ref: Option<ActorRef>,
}

impl<F: Finitomata + Default> FinitomataSupervisor<F>
where
    F::State: Send + Sync + 'static,
    F::Event: Send + Sync + 'static,
    F::Payload: Send + Sync + 'static,
{
    /// Creates a new supervisor with the given ID and transition graph.
    ///
    /// A fresh joerl `ActorSystem` is created automatically. Use
    /// [`with_system`](Self::with_system) to share a system across supervisors.
    pub fn new(id: impl Into<String>, graph: TransitionGraph<F::State, F::Event>) -> Self {
        let system = ActorSystem::new();
        Self {
            id: id.into(),
            system,
            graph: Arc::new(graph),
            cache: Arc::new(StateCache::new()),
            actors: Arc::new(RwLock::new(HashMap::new())),
            persistency: None,
            listener: None,
            auto_terminate: false,
            timer_interval: None,
            _supervisor_ref: None,
        }
    }

    /// Uses a shared joerl `ActorSystem` instead of creating a new one.
    pub fn with_system(mut self, system: Arc<ActorSystem>) -> Self {
        self.system = system;
        self
    }

    /// Configures a persistence backend for state recovery.
    pub fn with_persistency(mut self, persistency: impl Persistency<F> + 'static) -> Self {
        self.persistency = Some(Arc::new(persistency));
        self
    }

    /// Configures a transition listener for observability.
    pub fn with_listener(mut self, listener: impl Listener<F> + 'static) -> Self {
        self.listener = Some(Arc::new(listener));
        self
    }

    /// Enables automatic termination when the FSM reaches a final state.
    pub fn with_auto_terminate(mut self, auto_terminate: bool) -> Self {
        self.auto_terminate = auto_terminate;
        self
    }

    /// Configures a recurring timer interval for `on_timer` callbacks.
    pub fn with_timer(mut self, interval: Duration) -> Self {
        self.timer_interval = Some(interval);
        self
    }

    /// Spawns an unsupervised FSM actor.
    ///
    /// The FSM starts in the graph's initial state (or recovers from persistence
    /// if a prior state exists). If the actor crashes, it will not be restarted.
    /// Use [`spawn_supervised`](Self::spawn_supervised) for fault-tolerant operation.
    pub async fn start_fsm(
        &self,
        name: impl Into<String>,
        mut fsm: F,
        payload: F::Payload,
    ) -> Result<(), FinitomataError> {
        let name = name.into();

        let (initial_state, initial_payload, lifecycle) =
            if let Some(ref persist) = self.persistency {
                match persist.load(&name).await {
                    Ok(Some((lc, state, pl))) => (state, pl, lc),
                    _ => (
                        self.graph.initial_state().clone(),
                        payload,
                        Lifecycle::Created,
                    ),
                }
            } else {
                (
                    self.graph.initial_state().clone(),
                    payload,
                    Lifecycle::Created,
                )
            };

        let mut state = FsmState::new(name.clone(), initial_state, initial_payload);
        state.lifecycle = lifecycle;

        fsm.on_start(&mut state.payload).await;
        fsm.on_enter(&state.current, &mut state.payload).await;
        state.lifecycle = Lifecycle::Running;

        self.cache.update(&state);

        let actor = FsmActor {
            fsm,
            state,
            graph: self.graph.clone(),
            cache: self.cache.clone(),
            persistency: self.persistency.clone(),
            listener: self.listener.clone(),
            auto_terminate: self.auto_terminate,
            timer_interval: self.timer_interval,
            name: name.clone(),
            recover_on_start: false,
            system: None,
        };

        let actor_ref = self.system.spawn(actor);
        let pid = actor_ref.pid();

        let _ = self.system.register(&name, pid);

        {
            let mut actors = self.actors.write().await;
            actors.insert(name, pid);
        }

        Ok(())
    }

    /// Spawns an FSM actor under a joerl Supervisor with automatic restart on crash.
    ///
    /// If persistence is configured, the restarted actor recovers its last persisted
    /// state. Without persistence, the actor restarts from the graph's initial state
    /// with the provided default payload.
    ///
    /// Uses default restart intensity (3 restarts within 5 seconds). For custom
    /// limits, use [`spawn_supervised_with_intensity`](Self::spawn_supervised_with_intensity).
    pub async fn spawn_supervised(
        &self,
        name: impl Into<String>,
        fsm: F,
        payload: F::Payload,
        strategy: RestartStrategy,
    ) -> Result<(), FinitomataError> {
        self.spawn_supervised_with_intensity(
            name,
            fsm,
            payload,
            strategy,
            RestartIntensity::default(),
        )
        .await
    }

    /// Spawns a supervised FSM with custom restart intensity limits.
    ///
    /// The supervisor terminates itself if restarts exceed `intensity.max_restarts`
    /// within `intensity.within_seconds`.
    pub async fn spawn_supervised_with_intensity(
        &self,
        name: impl Into<String>,
        _fsm: F,
        payload: F::Payload,
        strategy: RestartStrategy,
        intensity: RestartIntensity,
    ) -> Result<(), FinitomataError> {
        let name = name.into();

        if let Some(ref persist) = self.persistency
            && persist.load(&name).await.ok().flatten().is_none()
        {
            let s = self.graph.initial_state().clone();
            let _ = persist.store(&name, &s, &payload).await;
        }

        let child_name = name.clone();
        let graph = self.graph.clone();
        let cache = self.cache.clone();
        let persistency = self.persistency.clone();
        let listener = self.listener.clone();
        let auto_terminate = self.auto_terminate;
        let timer_interval = self.timer_interval;
        let default_payload = payload;
        let system_for_factory = self.system.clone();

        let child_spec = ChildSpec::new(name.clone(), move || {
            let initial = graph.initial_state().clone();
            let fsm_state = FsmState::new(child_name.clone(), initial, default_payload.clone());
            Box::new(FsmActor {
                fsm: F::default(),
                state: fsm_state,
                graph: graph.clone(),
                cache: cache.clone(),
                persistency: persistency.clone(),
                listener: listener.clone(),
                auto_terminate,
                timer_interval,
                name: child_name.clone(),
                recover_on_start: true,
                system: Some(system_for_factory.clone()),
            }) as Box<dyn Actor>
        });

        let sup_spec = SupervisorSpec::new(strategy)
            .intensity(intensity)
            .child(child_spec);

        let _sup_ref = spawn_supervisor(&self.system, sup_spec);

        {
            let mut actors = self.actors.write().await;
            actors.insert(name, Pid::new());
        }

        Ok(())
    }

    /// Sends an event to a running FSM instance, triggering a state transition.
    ///
    /// The event and payload are delivered asynchronously to the FSM actor's
    /// mailbox. Returns an error if the FSM is not found or unreachable.
    pub async fn transition(
        &self,
        name: &str,
        event: F::Event,
        payload: F::Payload,
    ) -> Result<(), FinitomataError> {
        let pid = self.resolve_pid(name).await?;

        let msg: Message = Box::new(TransitionMsg { event, payload });
        self.system
            .send(pid, msg)
            .await
            .map_err(|e| FinitomataError::validation(format!("failed to send to `{name}`: {e}")))
    }

    /// Returns the full state snapshot for the given FSM (from the local cache).
    pub fn state(&self, name: &str) -> Option<FsmState<F::State, F::Payload>> {
        self.cache.get(name)
    }

    /// Returns just the current state for the given FSM.
    pub fn current_state(&self, name: &str) -> Option<F::State> {
        self.cache.get_state(name)
    }

    /// Returns `true` if the given FSM exists and is in the `Running` lifecycle.
    pub fn alive(&self, name: &str) -> bool {
        self.cache
            .get(name)
            .map(|s| matches!(s.lifecycle, Lifecycle::Running))
            .unwrap_or(false)
    }

    /// Returns all FSM instances managed by this supervisor.
    #[allow(clippy::type_complexity)]
    pub fn all(&self) -> Vec<(String, FsmState<F::State, F::Payload>)> {
        self.cache.all()
    }

    /// Sends a graceful shutdown signal to the given FSM.
    ///
    /// The FSM will call `on_terminate`, update its lifecycle to `Terminated`,
    /// and stop the actor. For supervised FSMs, a normal shutdown does NOT
    /// trigger a restart (only abnormal exits do).
    pub async fn shutdown(&self, name: &str) -> Result<(), FinitomataError> {
        let pid = self.resolve_pid(name).await?;

        let msg: Message = Box::new(ShutdownMsg);
        self.system
            .send(pid, msg)
            .await
            .map_err(|e| FinitomataError::validation(format!("failed to send to `{name}`: {e}")))
    }

    /// Returns a reference to the underlying joerl `ActorSystem`.
    pub fn system(&self) -> &Arc<ActorSystem> {
        &self.system
    }

    /// Returns the supervisor's ID.
    pub fn id(&self) -> &str {
        &self.id
    }

    async fn resolve_pid(&self, name: &str) -> Result<Pid, FinitomataError> {
        if let Some(pid) = self.system.whereis(name) {
            return Ok(pid);
        }
        let actors = self.actors.read().await;
        actors
            .get(name)
            .copied()
            .ok_or_else(|| FinitomataError::validation(format!("FSM `{name}` not found")))
    }
}
