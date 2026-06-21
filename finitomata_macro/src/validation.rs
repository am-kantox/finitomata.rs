use std::collections::{BTreeSet, VecDeque};

use crate::parser::ParsedFsm;

pub fn validate(fsm: &ParsedFsm) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();

    // Check initial state exists
    if fsm.initial.is_empty() {
        errors.push("no initial state defined".to_string());
    }

    // Check at least one final state
    if fsm.finals.is_empty() {
        errors.push("no final state defined (no transition to [*])".to_string());
    }

    // Check all states reachable from initial
    let reachable = reachable_from(&fsm.initial, fsm);
    for state in &fsm.states {
        if state != &fsm.initial && !reachable.contains(state.as_str()) {
            errors.push(format!(
                "state `{}` is not reachable from initial state `{}`",
                state, fsm.initial
            ));
        }
    }

    // Check no duplicate transitions (same from + event)
    let mut seen: BTreeSet<(&str, &str)> = BTreeSet::new();
    for t in &fsm.transitions {
        if !seen.insert((&t.from, &t.event)) {
            errors.push(format!(
                "duplicate transition: state `{}` already has event `{}`",
                t.from, t.event
            ));
        }
    }

    // Check event names are valid identifiers
    for t in &fsm.transitions {
        if !is_valid_identifier(&t.event) {
            errors.push(format!(
                "event `{}` is not a valid Rust identifier",
                t.event
            ));
        }
    }

    // Check state names are valid identifiers
    for state in &fsm.states {
        if !is_valid_identifier(state) {
            errors.push(format!("state `{}` is not a valid Rust identifier", state));
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn reachable_from(start: &str, fsm: &ParsedFsm) -> BTreeSet<String> {
    let mut visited = BTreeSet::new();
    let mut queue = VecDeque::new();
    queue.push_back(start.to_string());

    while let Some(current) = queue.pop_front() {
        if visited.contains(&current) {
            continue;
        }
        visited.insert(current.clone());

        for t in &fsm.transitions {
            if t.from == current && !visited.contains(&t.to) && t.to != "__terminal__" {
                queue.push_back(t.to.clone());
            }
        }
    }

    visited
}

fn is_valid_identifier(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let first = s.chars().next().unwrap();
    if !first.is_alphabetic() && first != '_' {
        return false;
    }
    s.chars().all(|c| c.is_alphanumeric() || c == '_')
}
