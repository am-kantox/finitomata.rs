use async_trait::async_trait;
use finitomata::{Finitomata, FinitomataSupervisor, TransitionResult, finitomata};
use std::time::Duration;

#[finitomata(
    fsm = r#"
        [*] --> pending
        pending --> |confirm!| confirmed
        confirmed --> |ship| shipped
        shipped --> |deliver| delivered
        delivered --> |close| [*]
        pending --> |cancel| cancelled
        confirmed --> |cancel| cancelled
        cancelled --> |close| [*]
    "#,
    syntax = "mermaid",
    auto_terminate = true,
    timer = 2000
)]
#[derive(Debug, Clone, Default)]
struct OrderWorkflow {
    retry_count: u32,
}

#[derive(Debug, Clone)]
struct OrderPayload {
    order_id: String,
    customer: String,
    amount: f64,
    notes: Vec<String>,
}

#[async_trait]
impl Finitomata for OrderWorkflow {
    type State = OrderWorkflowState;
    type Event = OrderWorkflowEvent;
    type Payload = OrderPayload;

    async fn on_transition(
        &mut self,
        from: &OrderWorkflowState,
        event: &OrderWorkflowEvent,
        _event_payload: &OrderPayload,
        state_payload: &mut OrderPayload,
    ) -> TransitionResult<OrderWorkflowState, OrderPayload> {
        let note = format!("{from} → {event:?}");
        state_payload.notes.push(note);

        match event {
            OrderWorkflowEvent::Confirm => {
                println!(
                    "  [order {}] Payment confirmed for ${:.2}",
                    state_payload.order_id, state_payload.amount
                );
                TransitionResult::Ok(OrderWorkflowState::Confirmed)
            }
            OrderWorkflowEvent::Ship => {
                println!(
                    "  [order {}] Shipped to {}",
                    state_payload.order_id, state_payload.customer
                );
                TransitionResult::Ok(OrderWorkflowState::Shipped)
            }
            OrderWorkflowEvent::Deliver => {
                println!("  [order {}] Delivered!", state_payload.order_id);
                TransitionResult::Ok(OrderWorkflowState::Delivered)
            }
            OrderWorkflowEvent::Cancel => {
                println!("  [order {}] Cancelled from {from}", state_payload.order_id);
                TransitionResult::Ok(OrderWorkflowState::Cancelled)
            }
            OrderWorkflowEvent::Close => {
                println!("  [order {}] Closed", state_payload.order_id);
                TransitionResult::Ok(OrderWorkflowState::Delivered)
            }
        }
    }

    async fn on_enter(&mut self, state: &OrderWorkflowState, payload: &mut OrderPayload) {
        println!("  [order {}] entered: {state}", payload.order_id);
    }

    async fn on_timer(
        &mut self,
        state: &OrderWorkflowState,
        payload: &mut OrderPayload,
    ) -> Option<(OrderWorkflowEvent, OrderPayload)> {
        self.retry_count += 1;
        println!(
            "  [order {}] timer tick #{} in state {state}",
            payload.order_id, self.retry_count
        );

        // Auto-ship after 2 timer ticks when confirmed
        if *state == OrderWorkflowState::Confirmed && self.retry_count >= 2 {
            Some((OrderWorkflowEvent::Ship, payload.clone()))
        } else {
            None
        }
    }

    async fn on_terminate(&mut self, payload: &mut OrderPayload) {
        println!(
            "  [order {}] workflow complete. History: {:?}",
            payload.order_id, payload.notes
        );
    }
}

#[tokio::main]
async fn main() {
    println!("=== Finitomata Order Workflow Example ===\n");

    let graph = OrderWorkflow::build_graph();
    let supervisor = FinitomataSupervisor::<OrderWorkflow>::new("orders", graph)
        .with_auto_terminate(true)
        .with_timer(Duration::from_secs(1));

    let payload = OrderPayload {
        order_id: "ORD-001".into(),
        customer: "Acme Corp".into(),
        amount: 1250.00,
        notes: vec![],
    };

    println!("--- Starting order workflow ---");
    supervisor
        .start_fsm("order_1", OrderWorkflow::default(), payload)
        .await
        .unwrap();

    // The confirm event is Hard (!) so it auto-fires after entering pending
    tokio::time::sleep(Duration::from_millis(100)).await;

    println!("\n--- Current state ---");
    if let Some(state) = supervisor.state("order_1") {
        println!(
            "  State: {:?}, Lifecycle: {:?}",
            state.current, state.lifecycle
        );
    }

    // Wait for timer to auto-ship
    println!("\n--- Waiting for timer to auto-ship ---");
    tokio::time::sleep(Duration::from_secs(3)).await;

    if let Some(state) = supervisor.state("order_1") {
        println!("\n--- Final state ---");
        println!("  State: {:?}", state.current);
        println!("  Notes: {:?}", state.payload.notes);
    }

    println!("\n=== Done ===");
}
