use async_trait::async_trait;
use finitomata::{Finitomata, FinitomataSupervisor, TransitionResult, finitomata};

#[finitomata(
    fsm = r#"
        [*] --> locked
        locked --> |coin| unlocked
        unlocked --> |push| locked
        unlocked --> |coin| unlocked
        locked --> |off| off
        off --> |reset| [*]
    "#,
    syntax = "mermaid",
    auto_terminate = true
)]
#[derive(Debug, Clone, Default)]
struct Turnstile {
    coins: u32,
    pushes: u32,
}

#[async_trait]
impl Finitomata for Turnstile {
    type State = TurnstileState;
    type Event = TurnstileEvent;
    type Payload = TurnstilePayload;

    async fn on_transition(
        &mut self,
        _from: &TurnstileState,
        event: &TurnstileEvent,
        _event_payload: &TurnstilePayload,
        state_payload: &mut TurnstilePayload,
    ) -> TransitionResult<TurnstileState, TurnstilePayload> {
        match event {
            TurnstileEvent::Coin => {
                self.coins += 1;
                state_payload.total_coins += 1;
                TransitionResult::Ok(TurnstileState::Unlocked)
            }
            TurnstileEvent::Push => {
                self.pushes += 1;
                state_payload.total_pushes += 1;
                TransitionResult::Ok(TurnstileState::Locked)
            }
            TurnstileEvent::Off => TransitionResult::Ok(TurnstileState::Off),
            TurnstileEvent::Reset => TransitionResult::Ok(TurnstileState::Off),
        }
    }

    async fn on_enter(&mut self, state: &TurnstileState, _payload: &mut TurnstilePayload) {
        println!("  → entered state: {state}");
    }

    async fn on_exit(&mut self, state: &TurnstileState, _payload: &mut TurnstilePayload) {
        println!("  ← exiting state: {state}");
    }

    async fn on_terminate(&mut self, _payload: &mut TurnstilePayload) {
        println!(
            "  ✓ turnstile shutting down (coins: {}, pushes: {})",
            self.coins, self.pushes
        );
    }
}

#[derive(Debug, Clone)]
struct TurnstilePayload {
    total_coins: u32,
    total_pushes: u32,
}

#[tokio::main]
async fn main() {
    println!("=== Finitomata Turnstile Example ===\n");

    let graph = Turnstile::build_graph();
    println!("Initial state: {}", graph.initial_state());
    println!("Final states: {:?}", graph.final_states());
    println!();

    let supervisor =
        FinitomataSupervisor::<Turnstile>::new("turnstile_sup", graph).with_auto_terminate(true);

    let payload = TurnstilePayload {
        total_coins: 0,
        total_pushes: 0,
    };
    supervisor
        .start_fsm("gate_1", Turnstile::default(), payload)
        .await
        .unwrap();

    println!("\n--- Inserting coin ---");
    supervisor
        .transition(
            "gate_1",
            TurnstileEvent::Coin,
            TurnstilePayload {
                total_coins: 0,
                total_pushes: 0,
            },
        )
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    println!("\n--- Pushing through ---");
    supervisor
        .transition(
            "gate_1",
            TurnstileEvent::Push,
            TurnstilePayload {
                total_coins: 0,
                total_pushes: 0,
            },
        )
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    println!("\n--- Turning off ---");
    supervisor
        .transition(
            "gate_1",
            TurnstileEvent::Off,
            TurnstilePayload {
                total_coins: 0,
                total_pushes: 0,
            },
        )
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    if let Some(state) = supervisor.state("gate_1") {
        println!("\nFinal state: {:?}", state.current);
        println!("Payload: {:?}", state.payload);
    }

    println!("\n=== Done ===");
}
