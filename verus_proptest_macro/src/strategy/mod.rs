mod analyze;
mod codegen;
mod expr_utils;

use proc_macro2::TokenStream;
use syn::{FnArgKind, ItemFn, PatType, Type};

use crate::{FuncGenerator, without_ref};

impl FuncGenerator<'_> {
    pub fn generate_strategy(&self) -> Option<TokenStream> {
        let requires = self.func.sig.spec.requires.as_ref()?;
        let exprs = &requires.exprs.exprs;

        let mut analyzer = StrategyAnalyzer::new(self.func);

        for expr in exprs.iter() {
            analyzer.analyze_constraint(expr);
        }

        analyzer.generate()
    }
}

pub(crate) struct StrategyAnalyzer<'a> {
    pub func: &'a ItemFn,
    pub constraints: Vec<ArgConstraint>,
    pub arg_types: Vec<&'a Type>,
}

#[derive(Debug, Clone)]
pub(crate) struct ArgConstraint {
    pub arg_name: String,
    pub arg_index: usize,
    pub kind: ConstraintKind,
}

#[derive(Debug, Clone)]
pub(crate) enum ConstraintKind {
    /// Simple range: `lo <= x <= hi` or `lo < x < hi`
    Range {
        lo: Option<Bound>,
        hi: Option<Bound>,
    },
    /// Dependent range: `x < y` where both are arguments
    LessThan {
        other_arg: String,
        other_index: usize,
        strict: bool,
    },
    /// Element-wise constraint on a collection: `forall|i| 0 <= i < s.len() ==>
    /// lo <= s[i] <= hi`
    ElementRange {
        lo: Option<Bound>,
        hi: Option<Bound>,
    },
    /// Sorted vector constraint: `forall|i, j| 0 <= i < j < s.len() ==> s[i] <=
    /// s[j]`
    Sorted { ascending: bool },
}

#[derive(Debug, Clone)]
pub(crate) struct Bound {
    pub value: BoundValue,
    pub inclusive: bool,
}

#[derive(Debug, Clone)]
pub(crate) enum BoundValue {
    Literal(i128),
    MaxValue(String), // e.g., "usize::MAX"
    Expr(String),     // fallback for complex expressions
}

impl<'a> StrategyAnalyzer<'a> {
    pub fn new(func: &'a ItemFn) -> Self {
        let arg_types: Vec<&'a Type> = func
            .sig
            .inputs
            .iter()
            .map(|arg| {
                let FnArgKind::Typed(PatType { ty, .. }) = &arg.kind else {
                    panic!("Expected typed argument");
                };
                without_ref(ty)
            })
            .collect();

        Self {
            func,
            constraints: Vec::new(),
            arg_types,
        }
    }

    pub fn find_arg_index(&self, name: &str) -> Option<usize> {
        self.func
            .sig
            .inputs
            .iter()
            .enumerate()
            .find_map(|(i, arg)| {
                if let syn::FnArgKind::Typed(pat_type) = &arg.kind {
                    if expr_utils::pat_to_name(&pat_type.pat) == Some(name.to_string()) {
                        return Some(i);
                    }
                }
                None
            })
    }

    pub fn add_lower_bound(
        &mut self,
        arg_name: String,
        arg_index: usize,
        value: BoundValue,
        inclusive: bool,
    ) {
        if let Some(constraint) = self
            .constraints
            .iter_mut()
            .find(|c| c.arg_name == arg_name && matches!(c.kind, ConstraintKind::Range { .. }))
        {
            if let ConstraintKind::Range { ref mut lo, .. } = constraint.kind {
                *lo = Some(Bound { value, inclusive });
            }
        } else {
            self.constraints.push(ArgConstraint {
                arg_name,
                arg_index,
                kind: ConstraintKind::Range {
                    lo: Some(Bound { value, inclusive }),
                    hi: None,
                },
            });
        }
    }

    pub fn add_upper_bound(
        &mut self,
        arg_name: String,
        arg_index: usize,
        value: BoundValue,
        inclusive: bool,
    ) {
        if let Some(constraint) = self
            .constraints
            .iter_mut()
            .find(|c| c.arg_name == arg_name && matches!(c.kind, ConstraintKind::Range { .. }))
        {
            if let ConstraintKind::Range { ref mut hi, .. } = constraint.kind {
                *hi = Some(Bound { value, inclusive });
            }
        } else {
            self.constraints.push(ArgConstraint {
                arg_name,
                arg_index,
                kind: ConstraintKind::Range {
                    lo: None,
                    hi: Some(Bound { value, inclusive }),
                },
            });
        }
    }
}
