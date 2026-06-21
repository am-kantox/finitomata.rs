pub mod mermaid;
pub mod plantuml;

use crate::transition::EventKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedTransition {
    pub from: String,
    pub to: String,
    pub event: String,
    pub kind: EventKind,
}

#[derive(Debug, Clone)]
pub struct ParsedFsm {
    pub initial: String,
    pub finals: Vec<String>,
    pub transitions: Vec<ParsedTransition>,
    pub states: Vec<String>,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum ParseError {
    #[error("syntax error at line {line}: {message}")]
    SyntaxError { line: usize, message: String },
    #[error("no initial state found")]
    NoInitialState,
    #[error("empty FSM definition")]
    EmptyDefinition,
}

pub trait FsmParser {
    fn parse(input: &str) -> Result<ParsedFsm, ParseError>;
}

pub fn classify_event(raw: &str) -> (String, EventKind) {
    if let Some(stripped) = raw.strip_suffix('!') {
        (stripped.to_string(), EventKind::Hard)
    } else if let Some(stripped) = raw.strip_suffix('?') {
        (stripped.to_string(), EventKind::Soft)
    } else {
        (raw.to_string(), EventKind::Normal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_event() {
        assert_eq!(classify_event("start"), ("start".into(), EventKind::Normal));
        assert_eq!(classify_event("go!"), ("go".into(), EventKind::Hard));
        assert_eq!(classify_event("try?"), ("try".into(), EventKind::Soft));
    }
}
