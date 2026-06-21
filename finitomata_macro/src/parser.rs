use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    Normal,
    Hard,
    Soft,
}

#[derive(Debug, Clone)]
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

pub fn parse_mermaid(input: &str) -> Result<ParsedFsm, String> {
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

        if !line.contains("-->") {
            return Err(format!("line {}: cannot parse `{}`", line_num + 1, line));
        }

        let parts: Vec<&str> = line.splitn(2, "-->").collect();
        let from = parts[0].trim().to_string();
        let rest = parts[1].trim();

        // Try flowchart: FROM --> |EVENT| TO
        if let Some(pipe_start) = rest.find('|') {
            let after_first_pipe = &rest[pipe_start + 1..];
            if let Some(pipe_end) = after_first_pipe.find('|') {
                let raw_event = after_first_pipe[..pipe_end].trim().to_string();
                let to = after_first_pipe[pipe_end + 1..].trim().to_string();
                let (event, kind) = classify_event(&raw_event);

                if from == "[*]" {
                    if initial.is_none() {
                        initial = Some(to.clone());
                    }
                    states.insert(to);
                } else if to == "[*]" {
                    finals.insert(from.clone());
                    states.insert(from.clone());
                    transitions.push(ParsedTransition {
                        from,
                        to: "__terminal__".to_string(),
                        event,
                        kind,
                    });
                } else {
                    states.insert(from.clone());
                    states.insert(to.clone());
                    transitions.push(ParsedTransition {
                        from,
                        to,
                        event,
                        kind,
                    });
                }
                continue;
            }
        }

        // Try state diagram: FROM --> TO : EVENT
        if let Some(colon_pos) = rest.find(':') {
            let to = rest[..colon_pos].trim().to_string();
            let raw_event = rest[colon_pos + 1..].trim().to_string();
            let (event, kind) = classify_event(&raw_event);

            if from == "[*]" {
                if initial.is_none() {
                    initial = Some(to.clone());
                }
                states.insert(to);
            } else if to == "[*]" {
                finals.insert(from.clone());
                states.insert(from.clone());
                transitions.push(ParsedTransition {
                    from,
                    to: "__terminal__".to_string(),
                    event,
                    kind,
                });
            } else {
                states.insert(from.clone());
                states.insert(to.clone());
                transitions.push(ParsedTransition {
                    from,
                    to,
                    event,
                    kind,
                });
            }
            continue;
        }

        // Bare arrow: [*] --> state or state --> [*]
        let to = rest.to_string();
        if from == "[*]" && !to.is_empty() {
            if initial.is_none() {
                initial = Some(to.clone());
            }
            states.insert(to);
        } else if to == "[*]" {
            finals.insert(from.clone());
            states.insert(from);
        } else {
            return Err(format!("line {}: cannot parse `{}`", line_num + 1, line));
        }
    }

    let initial =
        initial.ok_or_else(|| "no initial state found (use `[*] --> state`)".to_string())?;

    Ok(ParsedFsm {
        initial,
        finals: finals.into_iter().collect(),
        transitions,
        states: states.into_iter().collect(),
    })
}

pub fn parse_plantuml(input: &str) -> Result<ParsedFsm, String> {
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
            || line.starts_with("state ")
        {
            continue;
        }

        let arrow = if line.contains("-->") {
            "-->"
        } else if line.contains("->") {
            "->"
        } else {
            return Err(format!("line {}: cannot parse `{}`", line_num + 1, line));
        };

        let parts: Vec<&str> = line.splitn(2, arrow).collect();
        if parts.len() != 2 {
            return Err(format!("line {}: cannot parse `{}`", line_num + 1, line));
        }

        let from = parts[0].trim().to_string();
        let rest = parts[1].trim();

        if let Some(colon_pos) = rest.find(':') {
            let to = rest[..colon_pos].trim().to_string();
            let raw_event = rest[colon_pos + 1..].trim().to_string();
            let (event, kind) = classify_event(&raw_event);

            if from == "[*]" {
                if initial.is_none() {
                    initial = Some(to.clone());
                }
                states.insert(to);
            } else if to == "[*]" {
                finals.insert(from.clone());
                states.insert(from.clone());
                transitions.push(ParsedTransition {
                    from,
                    to: "__terminal__".to_string(),
                    event,
                    kind,
                });
            } else {
                states.insert(from.clone());
                states.insert(to.clone());
                transitions.push(ParsedTransition {
                    from,
                    to,
                    event,
                    kind,
                });
            }
        } else {
            let to = rest.to_string();
            if from == "[*]" && !to.is_empty() {
                if initial.is_none() {
                    initial = Some(to.clone());
                }
                states.insert(to);
            } else if to == "[*]" {
                finals.insert(from.clone());
                states.insert(from);
            } else {
                return Err(format!("line {}: cannot parse `{}`", line_num + 1, line));
            }
        }
    }

    let initial =
        initial.ok_or_else(|| "no initial state found (use `[*] --> state`)".to_string())?;

    Ok(ParsedFsm {
        initial,
        finals: finals.into_iter().collect(),
        transitions,
        states: states.into_iter().collect(),
    })
}

fn classify_event(raw: &str) -> (String, EventKind) {
    if let Some(stripped) = raw.strip_suffix('!') {
        (stripped.to_string(), EventKind::Hard)
    } else if let Some(stripped) = raw.strip_suffix('?') {
        (stripped.to_string(), EventKind::Soft)
    } else {
        (raw.to_string(), EventKind::Normal)
    }
}
