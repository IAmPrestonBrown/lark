//! Which functions can reach an allocation. Rule M-18.
//!
//! A collection starts at an allocation, so a thread that cannot allocate
//! cannot start one. Rule M-16 puts a poll at every loop back edge, and rule
//! M-18 removes the poll from a function that cannot reach an allocation. A
//! program of pure computation then pays nothing for the collector.
//!
//! The analysis is a fixed point over the call graph of the whole program.
//!
//! | Construct | Can allocate |
//! |---|---|
//! | `new T { ... }` and `new T[n]` | yes |
//! | A call to a function that can allocate | yes |
//! | An indirect call, through a pointer | yes, rule M-18 |
//! | A call to a name that no module defines | yes, rule M-18 |
//! | A call to a `gc_leaf` or `gc_safe` extern | no |
//! | A method call through an interface | yes, the target is not fixed |
//!
//! The analysis is conservative in one direction only. A function that it
//! marks might not allocate. A function that it clears never allocates.

use std::collections::{BTreeMap, BTreeSet};

use lark_resolve::ModuleGraph;
use lark_syntax::SyntaxKind::{
    ARG_LIST, CALL_EXPR, FN_DEF, GENERIC_ARGS, IDENT, METHOD_EXPR, NAME_EXPR, NAME_REF,
    NEW_ARRAY_EXPR, NEW_EXPR, PATH,
};
use lark_syntax::{SyntaxNode, all_tokens};

use crate::foreign::Foreign;
use crate::names;

/// Which functions of a program can reach an allocation.
#[derive(Clone, Debug, Default)]
pub struct Reach {
    allocating: BTreeSet<String>,
    /// True when the analysis could not see the whole program.
    ///
    /// The emitter then polls everywhere, which is what it did before the
    /// analysis existed.
    unknown: bool,
}

impl Reach {
    /// Reports whether a function needs a poll at its loop back edges.
    ///
    /// A name that the analysis never saw counts as able to allocate, so a
    /// gap in the analysis costs a poll rather than a missed safepoint.
    #[must_use]
    pub fn needs_poll(&self, function: Option<&str>) -> bool {
        if self.unknown {
            return true;
        }
        match function {
            Some(name) => self.allocating.contains(name),
            None => true,
        }
    }

    /// Returns the number of functions that can reach an allocation.
    #[must_use]
    pub fn count(&self) -> usize {
        self.allocating.len()
    }
}

/// Runs the analysis over every module of a program.
#[must_use]
pub fn analyze(graph: &ModuleGraph, foreign: &Foreign) -> Reach {
    let mut defined: BTreeMap<String, Facts> = BTreeMap::new();

    for module in graph.modules() {
        let root = module.parse.syntax();
        for item in root.descendants().filter(|node| node.kind() == FN_DEF) {
            let Some(name) = names::declared_name(&item) else {
                continue;
            };
            defined.insert(name, facts_of(&item));
        }
    }

    let mut result = Reach::default();

    // A call to a name that no module defines can allocate, unless a foreign
    // marker says otherwise. Rule M-18 and rule M-21.
    for (name, facts) in &defined {
        let mut allocates = facts.allocates;
        for target in &facts.calls {
            if defined.contains_key(target) {
                continue;
            }
            if foreign.get(target).is_some() {
                // A marked extern states its contract. Neither marker allows
                // the callee to allocate a Lark object.
                continue;
            }
            allocates = true;
            break;
        }
        if allocates {
            result.allocating.insert(name.clone());
        }
    }

    // Propagate along the call graph until nothing changes. The graph is at
    // most the size of the program, so the loop runs at most that many times.
    let mut changed = true;
    while changed {
        changed = false;
        for (name, facts) in &defined {
            if result.allocating.contains(name) {
                continue;
            }
            let reaches = facts
                .calls
                .iter()
                .any(|target| result.allocating.contains(target));
            if reaches {
                result.allocating.insert(name.clone());
                changed = true;
            }
        }
    }

    result
}

/// An analysis that knows nothing, so every loop polls.
#[must_use]
pub fn unknown() -> Reach {
    Reach {
        allocating: BTreeSet::new(),
        unknown: true,
    }
}

/// What one function body holds.
#[derive(Clone, Debug, Default)]
struct Facts {
    /// The function allocates, or does something the analysis cannot follow.
    allocates: bool,
    /// Every name that the body calls directly.
    calls: BTreeSet<String>,
}

/// Reads one function body.
fn facts_of(item: &SyntaxNode) -> Facts {
    let mut facts = Facts::default();
    for node in item.descendants() {
        match node.kind() {
            // A `new` allocates. A method call through an interface reaches a
            // target that the analysis cannot name, so rule M-18 counts it the
            // same way.
            NEW_EXPR | NEW_ARRAY_EXPR | METHOD_EXPR => facts.allocates = true,
            CALL_EXPR => match callee_name(&node) {
                Some(name) => {
                    facts.calls.insert(name);
                }
                // Rule M-18. An indirect call counts as able to allocate.
                None => facts.allocates = true,
            },
            _ => {}
        }
    }
    facts
}

/// Returns the name that a call expression names, when it names one.
///
/// A qualified call gives the name after the last `::`, because rule X-5 keeps
/// that name in the emitted C. A call through a pointer or an expression gives
/// nothing, and rule M-18 then counts it as able to allocate.
fn callee_name(call: &SyntaxNode) -> Option<String> {
    let callee = call
        .children()
        .find(|child| !matches!(child.kind(), ARG_LIST | GENERIC_ARGS))?;
    match callee.kind() {
        NAME_EXPR | NAME_REF | PATH => all_tokens(&callee)
            .filter(|token| token.kind() == IDENT)
            .map(|token| token.text().to_owned())
            .last(),
        _ => None,
    }
}
