use std::collections::BTreeSet;

use super::{FsmParser, ParseError, ParsedFsm, ParsedTransition, classify_event};

pub struct MermaidParser;

impl FsmParser for MermaidParser {
    fn parse(input: &str) -> Result<ParsedFsm, ParseError> {
        let mut transitions = Vec::new();
        let mut initial: Option<String> = None;
        let mut finals: BTreeSet<String> = BTreeSet::new();
        let mut states: BTreeSet<String> = BTreeSet::new();

        for (line_num, line) in input.lines().enumerate() {
            let line = line.trim();
            if line.is_empty()
                || line.starts_with("%%")
                || line.starts_with("graph")
                || line.starts_with("stateDiagram")
                || line.starts_with("---")
            {
                continue;
            }

            if let Some(parsed) = parse_flowchart_line(line) {
                match parsed {
                    ParsedLine::Transition { from, to, event } => {
                        let (event_name, kind) = classify_event(&event);

                        if from == "[*]" {
                            if initial.is_none() {
                                initial = Some(to.clone());
                            }
                            states.insert(to.clone());
                        } else if to == "[*]" {
                            finals.insert(from.clone());
                            states.insert(from.clone());
                            transitions.push(ParsedTransition {
                                from,
                                to: "__terminal__".to_string(),
                                event: event_name,
                                kind,
                            });
                        } else {
                            states.insert(from.clone());
                            states.insert(to.clone());
                            transitions.push(ParsedTransition {
                                from,
                                to,
                                event: event_name,
                                kind,
                            });
                        }
                    }
                    ParsedLine::InitialTransition { to } => {
                        if initial.is_none() {
                            initial = Some(to.clone());
                        }
                        states.insert(to);
                    }
                    ParsedLine::FinalTransition { from } => {
                        finals.insert(from.clone());
                        states.insert(from);
                    }
                }
            } else if let Some(parsed) = parse_state_diagram_line(line) {
                match parsed {
                    ParsedLine::Transition { from, to, event } => {
                        let (event_name, kind) = classify_event(&event);

                        if from == "[*]" {
                            if initial.is_none() {
                                initial = Some(to.clone());
                            }
                            states.insert(to.clone());
                        } else if to == "[*]" {
                            finals.insert(from.clone());
                            states.insert(from.clone());
                            transitions.push(ParsedTransition {
                                from,
                                to: "__terminal__".to_string(),
                                event: event_name,
                                kind,
                            });
                        } else {
                            states.insert(from.clone());
                            states.insert(to.clone());
                            transitions.push(ParsedTransition {
                                from,
                                to,
                                event: event_name,
                                kind,
                            });
                        }
                    }
                    ParsedLine::InitialTransition { to } => {
                        if initial.is_none() {
                            initial = Some(to.clone());
                        }
                        states.insert(to);
                    }
                    ParsedLine::FinalTransition { from } => {
                        finals.insert(from.clone());
                        states.insert(from);
                    }
                }
            } else {
                return Err(ParseError::SyntaxError {
                    line: line_num + 1,
                    message: format!("cannot parse: `{line}`"),
                });
            }
        }

        let initial = initial.ok_or(ParseError::NoInitialState)?;

        if transitions.is_empty() && states.len() <= 1 {
            return Err(ParseError::EmptyDefinition);
        }

        Ok(ParsedFsm {
            initial,
            finals: finals.into_iter().collect(),
            transitions,
            states: states.into_iter().collect(),
        })
    }
}

enum ParsedLine {
    Transition {
        from: String,
        to: String,
        event: String,
    },
    InitialTransition {
        to: String,
    },
    FinalTransition {
        from: String,
    },
}

// Mermaid flowchart: `state1 --> |event| state2`
fn parse_flowchart_line(line: &str) -> Option<ParsedLine> {
    // Pattern: FROM --> |EVENT| TO
    let parts: Vec<&str> = line.splitn(2, "-->").collect();
    if parts.len() != 2 {
        return None;
    }

    let from = parts[0].trim().to_string();
    let rest = parts[1].trim();

    // Check for |event| syntax
    if let Some(pipe_start) = rest.find('|') {
        let after_first_pipe = &rest[pipe_start + 1..];
        if let Some(pipe_end) = after_first_pipe.find('|') {
            let event = after_first_pipe[..pipe_end].trim().to_string();
            let to = after_first_pipe[pipe_end + 1..].trim().to_string();

            if event.is_empty() || to.is_empty() {
                return None;
            }

            // Always return Transition when there's an event — the main loop
            // handles [*] detection for initial/final classification
            return Some(ParsedLine::Transition { from, to, event });
        }
    }

    // Pattern: [*] --> state (no event, bare arrow)
    let to = rest.to_string();
    if !to.is_empty() {
        if from == "[*]" {
            return Some(ParsedLine::InitialTransition { to });
        }
        if to == "[*]" {
            return Some(ParsedLine::FinalTransition { from });
        }
    }

    None
}

// Mermaid state diagram: `state1 --> state2 : event`
fn parse_state_diagram_line(line: &str) -> Option<ParsedLine> {
    let parts: Vec<&str> = line.splitn(2, "-->").collect();
    if parts.len() != 2 {
        return None;
    }

    let from = parts[0].trim().to_string();
    let rest = parts[1].trim();

    if let Some(colon_pos) = rest.find(':') {
        let to = rest[..colon_pos].trim().to_string();
        let event = rest[colon_pos + 1..].trim().to_string();

        if to.is_empty() || event.is_empty() {
            return None;
        }

        // Always return Transition when there's an event — the main loop
        // handles [*] detection for initial/final classification
        return Some(ParsedLine::Transition { from, to, event });
    }

    // No colon — bare transition (initial/final)
    let to = rest.to_string();
    if from == "[*]" && !to.is_empty() {
        return Some(ParsedLine::InitialTransition { to });
    }
    if to == "[*]" && !from.is_empty() {
        return Some(ParsedLine::FinalTransition { from });
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transition::EventKind;

    #[test]
    fn test_flowchart_basic() {
        let input = r#"
            [*] --> idle
            idle --> |start| running
            running --> |stop| idle
            idle --> |shutdown| [*]
        "#;

        let fsm = MermaidParser::parse(input).unwrap();
        assert_eq!(fsm.initial, "idle");
        assert_eq!(fsm.finals, vec!["idle"]);
        assert_eq!(fsm.transitions.len(), 3);
        assert_eq!(fsm.transitions[0].event, "start");
    }

    #[test]
    fn test_hard_and_soft_events() {
        let input = r#"
            [*] --> s1
            s1 --> |go!| s2
            s2 --> |try?| s3
            s3 --> |end| [*]
        "#;

        let fsm = MermaidParser::parse(input).unwrap();
        assert_eq!(fsm.transitions[0].kind, EventKind::Hard);
        assert_eq!(fsm.transitions[0].event, "go");
        assert_eq!(fsm.transitions[1].kind, EventKind::Soft);
        assert_eq!(fsm.transitions[1].event, "try");
        assert_eq!(fsm.transitions[2].kind, EventKind::Normal);
    }

    #[test]
    fn test_state_diagram_syntax() {
        let input = r#"
            [*] --> idle
            idle --> running : start
            running --> idle : stop
            idle --> [*] : shutdown
        "#;

        let fsm = MermaidParser::parse(input).unwrap();
        assert_eq!(fsm.initial, "idle");
        assert_eq!(fsm.transitions.len(), 3);
        assert_eq!(fsm.transitions[0].from, "idle");
        assert_eq!(fsm.transitions[0].to, "running");
        assert_eq!(fsm.transitions[0].event, "start");
    }

    #[test]
    fn test_no_initial_state() {
        let input = r#"
            idle --> |start| running
        "#;

        let err = MermaidParser::parse(input).unwrap_err();
        assert!(matches!(err, ParseError::NoInitialState));
    }
}
