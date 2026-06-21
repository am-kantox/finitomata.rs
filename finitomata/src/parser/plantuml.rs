use std::collections::BTreeSet;

use super::{FsmParser, ParseError, ParsedFsm, ParsedTransition, classify_event};

pub struct PlantUmlParser;

impl FsmParser for PlantUmlParser {
    fn parse(input: &str) -> Result<ParsedFsm, ParseError> {
        let mut transitions = Vec::new();
        let mut initial: Option<String> = None;
        let mut finals: BTreeSet<String> = BTreeSet::new();
        let mut states: BTreeSet<String> = BTreeSet::new();

        for (line_num, line) in input.lines().enumerate() {
            let line = line.trim();
            if line.is_empty()
                || line.starts_with('\'')
                || line.starts_with("@startuml")
                || line.starts_with("@enduml")
                || line.starts_with("hide")
                || line.starts_with("skin")
            {
                continue;
            }

            if let Some(parsed) = parse_plantuml_transition(line) {
                match parsed {
                    PlantUmlLine::Transition { from, to, event } => {
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
                    PlantUmlLine::InitialTransition { to } => {
                        if initial.is_none() {
                            initial = Some(to.clone());
                        }
                        states.insert(to);
                    }
                    PlantUmlLine::FinalTransition { from } => {
                        finals.insert(from.clone());
                        states.insert(from);
                    }
                }
            } else if line.starts_with("state ") {
                // state declaration — extract name
                let name = line
                    .strip_prefix("state ")
                    .and_then(|s| s.split_whitespace().next())
                    .map(|s| s.trim_matches('"').to_string());
                if let Some(name) = name {
                    states.insert(name);
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

enum PlantUmlLine {
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

// PlantUML: `state1 --> state2 : event`  or  `[*] --> state1 : event`
fn parse_plantuml_transition(line: &str) -> Option<PlantUmlLine> {
    // Try both --> and -> arrows
    let arrow = if line.contains("-->") {
        "-->"
    } else if line.contains("->") {
        "->"
    } else {
        return None;
    };

    let parts: Vec<&str> = line.splitn(2, arrow).collect();
    if parts.len() != 2 {
        return None;
    }

    let from = parts[0].trim().to_string();
    let rest = parts[1].trim();

    // Check for colon (event label)
    if let Some(colon_pos) = rest.find(':') {
        let to = rest[..colon_pos].trim().to_string();
        let event = rest[colon_pos + 1..].trim().to_string();

        if to.is_empty() {
            return None;
        }

        if from == "[*]" {
            if event.is_empty() {
                return Some(PlantUmlLine::InitialTransition { to });
            }
            return Some(PlantUmlLine::InitialTransition { to });
        }
        if to == "[*]" {
            if event.is_empty() {
                return Some(PlantUmlLine::FinalTransition { from });
            }
            return Some(PlantUmlLine::Transition {
                from: from.clone(),
                to: "[*]".to_string(),
                event,
            });
        }

        if event.is_empty() {
            return None;
        }

        return Some(PlantUmlLine::Transition { from, to, event });
    }

    // No colon — bare arrow
    let to = rest.to_string();
    if from == "[*]" && !to.is_empty() {
        return Some(PlantUmlLine::InitialTransition { to });
    }
    if to == "[*]" && !from.is_empty() {
        return Some(PlantUmlLine::FinalTransition { from });
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transition::EventKind;

    #[test]
    fn test_plantuml_basic() {
        let input = r#"
            @startuml
            [*] --> idle
            idle --> running : start
            running --> idle : stop
            running --> [*] : shutdown
            @enduml
        "#;

        let fsm = PlantUmlParser::parse(input).unwrap();
        assert_eq!(fsm.initial, "idle");
        assert_eq!(fsm.finals, vec!["running"]);
        assert_eq!(fsm.transitions.len(), 3);
    }

    #[test]
    fn test_plantuml_hard_events() {
        let input = r#"
            [*] --> s1
            s1 --> s2 : go!
            s2 --> [*] : end
        "#;

        let fsm = PlantUmlParser::parse(input).unwrap();
        assert_eq!(fsm.transitions[0].event, "go");
        assert_eq!(fsm.transitions[0].kind, EventKind::Hard);
    }

    #[test]
    fn test_plantuml_with_state_declarations() {
        let input = r#"
            @startuml
            state "Idle" as idle
            state "Running" as running
            [*] --> idle
            idle --> running : start
            running --> [*] : done
            @enduml
        "#;

        let fsm = PlantUmlParser::parse(input).unwrap();
        assert_eq!(fsm.initial, "idle");
    }
}
