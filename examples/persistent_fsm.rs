use async_trait::async_trait;
use finitomata::{
    Finitomata, FinitomataSupervisor, TransitionResult, finitomata,
    persistency::memory::InMemoryPersistency,
};
use std::time::Duration;

#[finitomata(
    fsm = r#"
        [*] --> draft
        draft --> |submit| review
        review --> |approve| published
        review --> |reject| draft
        published --> |archive| archived
        archived --> |restore| draft
        draft --> |delete| deleted
        deleted --> |confirm| [*]
    "#,
    syntax = "mermaid",
    auto_terminate = true
)]
#[derive(Debug, Clone, Default)]
struct Document;

#[derive(Debug, Clone)]
struct DocPayload {
    title: String,
    version: u32,
    reviewer: Option<String>,
}

#[async_trait]
impl Finitomata for Document {
    type State = DocumentState;
    type Event = DocumentEvent;
    type Payload = DocPayload;

    async fn on_transition(
        &mut self,
        _from: &DocumentState,
        event: &DocumentEvent,
        _event_payload: &DocPayload,
        state_payload: &mut DocPayload,
    ) -> TransitionResult<DocumentState, DocPayload> {
        match event {
            DocumentEvent::Submit => {
                state_payload.version += 1;
                println!(
                    "  Submitted '{}' v{} for review",
                    state_payload.title, state_payload.version
                );
                TransitionResult::Ok(DocumentState::Review)
            }
            DocumentEvent::Approve => {
                state_payload.reviewer = Some("editor@example.com".into());
                println!(
                    "  Approved '{}' by {:?}",
                    state_payload.title, state_payload.reviewer
                );
                TransitionResult::Ok(DocumentState::Published)
            }
            DocumentEvent::Reject => {
                println!("  Rejected '{}' — back to draft", state_payload.title);
                TransitionResult::Ok(DocumentState::Draft)
            }
            DocumentEvent::Archive => {
                println!("  Archived '{}'", state_payload.title);
                TransitionResult::Ok(DocumentState::Archived)
            }
            DocumentEvent::Restore => {
                println!("  Restored '{}' from archive", state_payload.title);
                state_payload.reviewer = None;
                TransitionResult::Ok(DocumentState::Draft)
            }
            DocumentEvent::Delete => {
                println!("  Deleted '{}'", state_payload.title);
                TransitionResult::Ok(DocumentState::Deleted)
            }
            DocumentEvent::Confirm => {
                println!("  Confirmed deletion of '{}'", state_payload.title);
                TransitionResult::Ok(DocumentState::Deleted)
            }
        }
    }

    async fn on_enter(&mut self, state: &DocumentState, payload: &mut DocPayload) {
        println!("  [{}] entered: {state}", payload.title);
    }
}

#[tokio::main]
async fn main() {
    println!("=== Finitomata Persistent Document Workflow ===\n");

    let graph = Document::build_graph();
    let persistency = InMemoryPersistency::<Document>::new();

    let supervisor = FinitomataSupervisor::<Document>::new("docs", graph)
        .with_persistency(persistency)
        .with_auto_terminate(true);

    let payload = DocPayload {
        title: "Architecture RFC".into(),
        version: 0,
        reviewer: None,
    };

    println!("--- Starting document workflow ---");
    supervisor
        .start_fsm("doc_1", Document, payload)
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    println!("\n--- Submitting for review ---");
    let ep = DocPayload {
        title: String::new(),
        version: 0,
        reviewer: None,
    };
    supervisor
        .transition("doc_1", DocumentEvent::Submit, ep.clone())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    println!("\n--- Rejecting (back to draft) ---");
    supervisor
        .transition("doc_1", DocumentEvent::Reject, ep.clone())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    println!("\n--- Re-submitting ---");
    supervisor
        .transition("doc_1", DocumentEvent::Submit, ep.clone())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    println!("\n--- Approving ---");
    supervisor
        .transition("doc_1", DocumentEvent::Approve, ep.clone())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    println!("\n--- Archiving ---");
    supervisor
        .transition("doc_1", DocumentEvent::Archive, ep.clone())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    if let Some(state) = supervisor.state("doc_1") {
        println!("\n--- Final state ---");
        println!("  State: {:?}", state.current);
        println!("  Payload: {:?}", state.payload);
        println!("  History: {} entries", state.history.len());
    }

    println!("\n=== Done ===");
}
