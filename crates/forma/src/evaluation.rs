#![cfg_attr(not(test), allow(dead_code))]

use crate::Location;
use std::collections::HashMap;
use std::hash::Hash;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct FailureId(u32);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum FailureClass {
    Recoverable,
    Terminal,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum FailureOperation {
    Unary,
    Binary,
    Field,
    Index,
    Call,
    NativeCall,
    Condition,
    Match,
    Array,
    Tuple,
    Tagged,
    Dict,
    Interpolation,
    Binding,
    ModuleResult,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FailureLimits {
    pub(crate) max_propagated_nodes: usize,
    pub(crate) max_causes_per_node: usize,
    pub(crate) max_render_depth: usize,
}

impl FailureLimits {
    pub(crate) const fn new(
        max_propagated_nodes: usize,
        max_causes_per_node: usize,
        max_render_depth: usize,
    ) -> Self {
        Self {
            max_propagated_nodes,
            max_causes_per_node,
            max_render_depth,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum EvalOutcome<T> {
    Value(T),
    Never(FailureId),
}

impl<T> EvalOutcome<T> {
    pub(crate) fn map<U>(self, map: impl FnOnce(T) -> U) -> EvalOutcome<U> {
        match self {
            Self::Value(value) => EvalOutcome::Value(map(value)),
            Self::Never(failure) => EvalOutcome::Never(failure),
        }
    }

    pub(crate) const fn failure(&self) -> Option<FailureId> {
        match self {
            Self::Value(_) => None,
            Self::Never(failure) => Some(*failure),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum FailureNode<R> {
    Root {
        failure: R,
    },
    Propagated {
        operation: FailureOperation,
        location: Option<Location>,
        causes: Box<[FailureId]>,
    },
    Truncated {
        causes: Box<[FailureId]>,
    },
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct PropagationKey {
    operation: FailureOperation,
    location: Option<Location>,
    causes: Box<[FailureId]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum LineageStep<'a, R> {
    Root(&'a R),
    Propagated {
        operation: FailureOperation,
        location: Option<Location>,
    },
    Truncated,
}

#[derive(Debug)]
pub(crate) struct FailureArena<R> {
    nodes: Vec<FailureNode<R>>,
    propagated: HashMap<PropagationKey, FailureId>,
    propagated_count: usize,
    truncated: Option<FailureId>,
    limits: FailureLimits,
}

impl<R> FailureArena<R> {
    pub(crate) fn new(limits: FailureLimits) -> Self {
        Self {
            nodes: Vec::new(),
            propagated: HashMap::new(),
            propagated_count: 0,
            truncated: None,
            limits,
        }
    }

    pub(crate) fn root(&mut self, class: FailureClass, failure: R) -> Result<EvalOutcome<()>, R> {
        if class == FailureClass::Terminal {
            return Err(failure);
        }
        let id = self.push(FailureNode::Root { failure });
        Ok(EvalOutcome::Never(id))
    }

    pub(crate) fn propagate<T>(
        &mut self,
        operation: FailureOperation,
        location: Option<Location>,
        inputs: &[EvalOutcome<T>],
    ) -> Option<EvalOutcome<()>> {
        let causes = inputs
            .iter()
            .filter_map(EvalOutcome::failure)
            .collect::<Vec<_>>();
        (!causes.is_empty())
            .then(|| EvalOutcome::Never(self.propagate_causes(operation, location, causes)))
    }

    pub(crate) fn propagate_causes(
        &mut self,
        operation: FailureOperation,
        location: Option<Location>,
        causes: impl IntoIterator<Item = FailureId>,
    ) -> FailureId {
        let causes = normalize_causes(causes, self.limits.max_causes_per_node);
        assert!(!causes.is_empty(), "propagation requires a Never cause");
        let key = PropagationKey {
            operation,
            location,
            causes: causes.clone().into_boxed_slice(),
        };
        if let Some(id) = self.propagated.get(&key) {
            return *id;
        }
        if self.propagated_count >= self.limits.max_propagated_nodes {
            if let Some(id) = self.truncated {
                return id;
            }
            let id = self.push(FailureNode::Truncated {
                causes: causes.into_boxed_slice(),
            });
            self.truncated = Some(id);
            return id;
        }
        let id = self.push(FailureNode::Propagated {
            operation,
            location,
            causes: causes.into_boxed_slice(),
        });
        self.propagated.insert(key, id);
        self.propagated_count += 1;
        id
    }

    pub(crate) fn node(&self, id: FailureId) -> Option<&FailureNode<R>> {
        self.nodes.get(id.0 as usize)
    }

    pub(crate) fn lineage(&self, start: FailureId) -> Vec<LineageStep<'_, R>> {
        let mut output = Vec::new();
        let mut current = start;
        for _ in 0..self.limits.max_render_depth {
            match self.node(current) {
                Some(FailureNode::Root { failure }) => {
                    output.push(LineageStep::Root(failure));
                    return output;
                }
                Some(FailureNode::Propagated {
                    operation,
                    location,
                    causes,
                }) => {
                    output.push(LineageStep::Propagated {
                        operation: *operation,
                        location: *location,
                    });
                    let Some(next) = causes.first() else {
                        output.push(LineageStep::Truncated);
                        return output;
                    };
                    current = *next;
                }
                Some(FailureNode::Truncated { causes }) => {
                    output.push(LineageStep::Truncated);
                    let Some(next) = causes.first() else {
                        return output;
                    };
                    current = *next;
                }
                None => {
                    output.push(LineageStep::Truncated);
                    return output;
                }
            }
        }
        output.push(LineageStep::Truncated);
        output
    }

    fn push(&mut self, node: FailureNode<R>) -> FailureId {
        let index = u32::try_from(self.nodes.len()).expect("failure arena exceeds u32::MAX nodes");
        self.nodes.push(node);
        FailureId(index)
    }
}

fn normalize_causes(causes: impl IntoIterator<Item = FailureId>, limit: usize) -> Vec<FailureId> {
    let mut normalized = Vec::new();
    for cause in causes {
        if normalized.contains(&cause) {
            continue;
        }
        if normalized.len() == limit {
            break;
        }
        normalized.push(cause);
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SourceDatabase, TextRange};

    fn location(offset: u32) -> Location {
        let mut sources = SourceDatabase::default();
        let source = sources.add("lineage.forma", "0123456789");
        Location::new(source, TextRange::new(offset, offset + 1).unwrap())
    }

    fn limits() -> FailureLimits {
        FailureLimits::new(8, 3, 8)
    }

    fn root(arena: &mut FailureArena<&'static str>, message: &'static str) -> FailureId {
        arena
            .root(FailureClass::Recoverable, message)
            .unwrap()
            .failure()
            .unwrap()
    }

    #[test]
    fn outcome_map_preserves_never_without_running_user_code() {
        let mut called = false;
        let outcome = EvalOutcome::<i32>::Never(FailureId(4)).map(|value| {
            called = true;
            value + 1
        });
        assert!(!called);
        assert_eq!(outcome, EvalOutcome::Never(FailureId(4)));
    }

    #[test]
    fn propagation_is_stable_deduplicated_and_interned() {
        let mut arena = FailureArena::new(limits());
        let first = root(&mut arena, "first");
        let second = root(&mut arena, "second");
        let location = Some(location(3));
        let propagated =
            arena.propagate_causes(FailureOperation::Binary, location, [first, second, first]);
        let reused =
            arena.propagate_causes(FailureOperation::Binary, location, [first, second, first]);
        assert_eq!(propagated, reused);
        assert_eq!(
            arena.node(propagated),
            Some(&FailureNode::Propagated {
                operation: FailureOperation::Binary,
                location,
                causes: vec![first, second].into_boxed_slice(),
            })
        );
    }

    #[test]
    fn aliases_reuse_ids_and_operation_propagation_collects_never_inputs() {
        let mut arena = FailureArena::new(limits());
        let failure = root(&mut arena, "bad input");
        let alias = EvalOutcome::<i32>::Never(failure);
        assert_eq!(alias.failure(), Some(failure));
        let inputs = [EvalOutcome::Value(1), alias, EvalOutcome::Never(failure)];
        let propagated = arena
            .propagate(FailureOperation::Call, Some(location(2)), &inputs)
            .unwrap();
        let id = propagated.failure().unwrap();
        let FailureNode::Propagated { causes, .. } = arena.node(id).unwrap() else {
            panic!("expected propagation node")
        };
        assert_eq!(causes.as_ref(), &[failure]);
    }

    #[test]
    fn terminal_failures_cannot_enter_the_arena() {
        let mut arena = FailureArena::new(limits());
        assert_eq!(
            arena.root(FailureClass::Terminal, "cancelled"),
            Err("cancelled")
        );
        assert!(arena.nodes.is_empty());
    }

    #[test]
    fn propagation_and_render_depth_budgets_truncate_deterministically() {
        let mut arena = FailureArena::new(FailureLimits::new(1, 2, 2));
        let first = root(&mut arena, "first");
        let second = root(&mut arena, "second");
        let one = arena.propagate_causes(
            FailureOperation::Binary,
            Some(location(1)),
            [first, second, first],
        );
        let truncated =
            arena.propagate_causes(FailureOperation::Call, Some(location(2)), [one, second]);
        let reused = arena.propagate_causes(FailureOperation::Field, Some(location(3)), [second]);
        assert_eq!(truncated, reused);
        assert!(matches!(
            arena.node(truncated),
            Some(FailureNode::Truncated { .. })
        ));
        assert_eq!(
            arena.lineage(truncated),
            vec![
                LineageStep::Truncated,
                LineageStep::Propagated {
                    operation: FailureOperation::Binary,
                    location: Some(location(1)),
                },
                LineageStep::Truncated
            ]
        );
    }

    #[test]
    fn every_propagation_category_is_a_distinct_stable_value() {
        let operations = [
            FailureOperation::Unary,
            FailureOperation::Binary,
            FailureOperation::Field,
            FailureOperation::Index,
            FailureOperation::Call,
            FailureOperation::NativeCall,
            FailureOperation::Condition,
            FailureOperation::Match,
            FailureOperation::Array,
            FailureOperation::Tuple,
            FailureOperation::Tagged,
            FailureOperation::Dict,
            FailureOperation::Interpolation,
            FailureOperation::Binding,
            FailureOperation::ModuleResult,
            FailureOperation::Other,
        ];
        let unique = operations
            .into_iter()
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(unique.len(), operations.len());
    }
}
