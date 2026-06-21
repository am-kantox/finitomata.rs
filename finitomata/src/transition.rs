use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt::Debug;
use std::hash::Hash;

use crate::error::ValidationError;

/// Classifies how an event behaves in the FSM.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum EventKind {
    /// Standard event — requires explicit triggering.
    Normal,
    /// Hard event (suffix `!`) — auto-fires immediately after entering the source state.
    Hard,
    /// Soft event (suffix `?`) — silently ignored if the current state doesn't respond.
    Soft,
}

/// A single transition definition in the FSM graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transition<S, E> {
    /// Source state.
    pub from: S,
    /// Allowed target states (on_transition picks one).
    pub to: Vec<S>,
    /// The event that triggers this transition.
    pub event: E,
    /// Classification of the event.
    pub kind: EventKind,
}

/// The validated transition graph for a finite state machine.
///
/// Holds all states, events, and transitions, with an index for fast lookup.
/// Created at compile time by the `#[finitomata]` macro or at runtime via
/// [`TransitionGraph::new`].
#[derive(Debug, Clone)]
pub struct TransitionGraph<S: Ord + Clone, E: Ord + Clone> {
    initial: S,
    finals: BTreeSet<S>,
    transitions: Vec<Transition<S, E>>,
    index: BTreeMap<S, Vec<usize>>,
}

impl<S, E> TransitionGraph<S, E>
where
    S: Ord + Clone + Hash + Debug + Eq,
    E: Ord + Clone + Hash + Debug + Eq,
{
    /// Constructs a new transition graph from an initial state, final states, and transitions.
    pub fn new(initial: S, finals: BTreeSet<S>, transitions: Vec<Transition<S, E>>) -> Self {
        let mut index: BTreeMap<S, Vec<usize>> = BTreeMap::new();
        for (i, t) in transitions.iter().enumerate() {
            index.entry(t.from.clone()).or_default().push(i);
        }
        Self {
            initial,
            finals,
            transitions,
            index,
        }
    }

    /// Returns the initial (starting) state of the FSM.
    pub fn initial_state(&self) -> &S {
        &self.initial
    }

    /// Returns the set of final (terminal) states.
    pub fn final_states(&self) -> &BTreeSet<S> {
        &self.finals
    }

    /// Returns `true` if the given state is a final state.
    pub fn is_final(&self, state: &S) -> bool {
        self.finals.contains(state)
    }

    /// Returns the full list of transitions.
    pub fn transitions(&self) -> &[Transition<S, E>] {
        &self.transitions
    }

    /// Checks if a transition from `from` via `event` is allowed.
    /// Returns the allowed target states and the event kind, or `None`.
    pub fn allowed(&self, from: &S, event: &E) -> Option<(&[S], EventKind)> {
        self.index.get(from).and_then(|indices| {
            indices.iter().find_map(|&i| {
                let t = &self.transitions[i];
                if &t.event == event {
                    Some((t.to.as_slice(), t.kind))
                } else {
                    None
                }
            })
        })
    }

    /// Returns `true` if the given state responds to the given event.
    pub fn responds(&self, state: &S, event: &E) -> bool {
        self.allowed(state, event).is_some()
    }

    /// Returns all events available from the given state, with their kinds.
    pub fn events_for(&self, state: &S) -> Vec<(&E, EventKind)> {
        self.index
            .get(state)
            .map(|indices| {
                indices
                    .iter()
                    .map(|&i| (&self.transitions[i].event, self.transitions[i].kind))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Returns all states mentioned in the graph (initial, final, and transition participants).
    pub fn all_states(&self) -> BTreeSet<S> {
        let mut states = BTreeSet::new();
        states.insert(self.initial.clone());
        for s in &self.finals {
            states.insert(s.clone());
        }
        for t in &self.transitions {
            states.insert(t.from.clone());
            for to in &t.to {
                states.insert(to.clone());
            }
        }
        states
    }

    /// Returns all distinct events in the graph.
    pub fn all_events(&self) -> BTreeSet<&E> {
        self.transitions.iter().map(|t| &t.event).collect()
    }

    /// Finds the shortest path (sequence of events and intermediate states)
    /// from `from` to `to` using BFS. Returns `None` if unreachable.
    pub fn shortest_path(&self, from: &S, to: &S) -> Option<Vec<(E, S)>> {
        if from == to {
            return Some(vec![]);
        }

        let mut visited = BTreeSet::new();
        let mut queue: VecDeque<(S, Vec<(E, S)>)> = VecDeque::new();
        queue.push_back((from.clone(), vec![]));
        visited.insert(from.clone());

        while let Some((current, path)) = queue.pop_front() {
            if let Some(indices) = self.index.get(&current) {
                for &i in indices {
                    let t = &self.transitions[i];
                    for target in &t.to {
                        if target == to {
                            let mut result = path.clone();
                            result.push((t.event.clone(), target.clone()));
                            return Some(result);
                        }
                        if !visited.contains(target) {
                            visited.insert(target.clone());
                            let mut new_path = path.clone();
                            new_path.push((t.event.clone(), target.clone()));
                            queue.push_back((target.clone(), new_path));
                        }
                    }
                }
            }
        }
        None
    }

    /// Validates the graph for structural correctness.
    /// Returns `Ok(())` if valid, or a list of validation errors.
    pub fn validate(&self) -> Result<(), Vec<ValidationError>> {
        let mut errors = Vec::new();
        let all_states = self.all_states();

        if self.finals.is_empty() {
            errors.push(ValidationError::NoFinalState);
        }

        let reachable = self.reachable_from(&self.initial);
        for state in &all_states {
            if state != &self.initial && !reachable.contains(state) {
                errors.push(ValidationError::UnreachableState(format!("{state:?}")));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    fn reachable_from(&self, start: &S) -> BTreeSet<S> {
        let mut visited = BTreeSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(start.clone());

        while let Some(current) = queue.pop_front() {
            if visited.contains(&current) {
                continue;
            }
            visited.insert(current.clone());
            if let Some(indices) = self.index.get(&current) {
                for &i in indices {
                    for target in &self.transitions[i].to {
                        if !visited.contains(target) {
                            queue.push_back(target.clone());
                        }
                    }
                }
            }
        }
        visited
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_graph() {
        let graph: TransitionGraph<&str, &str> = TransitionGraph::new(
            "idle",
            BTreeSet::from(["done"]),
            vec![
                Transition {
                    from: "idle",
                    to: vec!["running"],
                    event: "start",
                    kind: EventKind::Normal,
                },
                Transition {
                    from: "running",
                    to: vec!["done"],
                    event: "finish",
                    kind: EventKind::Normal,
                },
            ],
        );

        assert_eq!(graph.initial_state(), &"idle");
        assert!(graph.is_final(&"done"));
        assert!(graph.responds(&"idle", &"start"));
        assert!(!graph.responds(&"idle", &"finish"));
        assert!(graph.validate().is_ok());
    }

    #[test]
    fn test_shortest_path() {
        let graph: TransitionGraph<&str, &str> = TransitionGraph::new(
            "a",
            BTreeSet::from(["c"]),
            vec![
                Transition {
                    from: "a",
                    to: vec!["b"],
                    event: "go",
                    kind: EventKind::Normal,
                },
                Transition {
                    from: "b",
                    to: vec!["c"],
                    event: "end",
                    kind: EventKind::Normal,
                },
            ],
        );

        let path = graph.shortest_path(&"a", &"c").unwrap();
        assert_eq!(path, vec![("go", "b"), ("end", "c")]);
    }

    #[test]
    fn test_unreachable_state() {
        let graph: TransitionGraph<&str, &str> = TransitionGraph::new(
            "a",
            BTreeSet::from(["c"]),
            vec![
                Transition {
                    from: "a",
                    to: vec!["b"],
                    event: "go",
                    kind: EventKind::Normal,
                },
                Transition {
                    from: "c",
                    to: vec!["a"],
                    event: "loop",
                    kind: EventKind::Normal,
                },
            ],
        );

        let errors = graph.validate().unwrap_err();
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, ValidationError::UnreachableState(_)))
        );
    }

    #[test]
    fn test_no_final_state() {
        let graph: TransitionGraph<&str, &str> = TransitionGraph::new(
            "a",
            BTreeSet::new(),
            vec![Transition {
                from: "a",
                to: vec!["b"],
                event: "go",
                kind: EventKind::Normal,
            }],
        );

        let errors = graph.validate().unwrap_err();
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, ValidationError::NoFinalState))
        );
    }
}
