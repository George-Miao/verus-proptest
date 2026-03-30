use proc_macro2::TokenStream;
use quote::quote;

use super::{
    Bound, BoundValue, ConstraintKind, StrategyAnalyzer,
    expr_utils::{bound_to_tokens, extract_vec_element_type},
};

/// Maximum practical value for test generation to avoid memory exhaustion.
/// This caps ranges that would otherwise generate astronomically large values.
/// Kept small to ensure Verus verification completes in reasonable time.
const PRACTICAL_MAX: i128 = 100;

impl StrategyAnalyzer<'_> {
    pub fn generate(&self) -> Option<TokenStream> {
        if self.constraints.is_empty() {
            return None;
        }

        let arg_count = self.func.sig.inputs.len();

        // Group constraints by argument index
        let mut strategies: Vec<Option<TokenStream>> = vec![None; arg_count];

        for constraint in &self.constraints {
            let strategy = self.generate_single_strategy(constraint);
            // For now just use the latest strategy for each argument
            // A more sophisticated approach would intersect ranges
            strategies[constraint.arg_index] = Some(strategy);
        }

        // Check if we have any dependent constraints
        let has_dependent = self
            .constraints
            .iter()
            .any(|c| matches!(c.kind, ConstraintKind::LessThan { .. }));

        if has_dependent {
            return self.generate_dependent_strategy();
        }

        // Generate independent strategies
        let strategy_exprs: Vec<TokenStream> = strategies
            .into_iter()
            .enumerate()
            .map(|(i, s)| {
                s.unwrap_or_else(|| {
                    // Fall back to any() for unconstrained arguments with explicit type
                    let arg_type = self.arg_types[i];
                    quote! { ::verus_proptest::hidden::proptest::prelude::any::<#arg_type>() }
                })
            })
            .collect();

        // Combine into tuple strategy
        let combined = if strategy_exprs.len() == 1 {
            let s = &strategy_exprs[0];
            quote! {
                (#s).prop_map(|x| (x,)).boxed()
            }
        } else {
            quote! {
                (#(#strategy_exprs),*).boxed()
            }
        };

        Some(quote! {
            fn strategy() -> ::verus_proptest::hidden::proptest::strategy::BoxedStrategy<Self::Args> {
                use ::verus_proptest::hidden::proptest::prelude::*;
                #combined
            }
        })
    }

    fn generate_single_strategy(&self, constraint: &super::ArgConstraint) -> TokenStream {
        let arg_index = constraint.arg_index;
        match &constraint.kind {
            ConstraintKind::Range { lo, hi } => self.generate_range_strategy(arg_index, lo, hi),
            ConstraintKind::ElementRange { lo, hi } => {
                self.generate_element_range_strategy(arg_index, lo, hi)
            }
            ConstraintKind::LessThan { .. } => {
                // Handled in generate_dependent_strategy
                let arg_type = self.arg_types[arg_index];
                quote! { ::verus_proptest::hidden::proptest::prelude::any::<#arg_type>() }
            }
            ConstraintKind::Sorted { ascending } => {
                self.generate_sorted_strategy(arg_index, *ascending)
            }
        }
    }

    fn generate_range_strategy(
        &self,
        arg_index: usize,
        lo: &Option<Bound>,
        hi: &Option<Bound>,
    ) -> TokenStream {
        let arg_type = self.arg_types[arg_index];

        // Check if the upper bound is impractically large (e.g., usize::MAX / 3)
        let hi_is_large = hi
            .as_ref()
            .is_some_and(|b| !matches!(b.value, BoundValue::Literal(n) if n <= PRACTICAL_MAX));

        let lo_expr = lo.as_ref().map(|b| bound_to_tokens(b, true));
        let hi_expr = if hi_is_large {
            // Cap to practical maximum for testing
            let practical_max = proc_macro2::Literal::i128_unsuffixed(PRACTICAL_MAX);
            Some(quote! { #practical_max })
        } else {
            hi.as_ref().map(|b| bound_to_tokens(b, false))
        };

        match (lo_expr, hi_expr) {
            (Some(lo), Some(hi)) => {
                quote! { ((#lo as #arg_type)..=(#hi as #arg_type)) }
            }
            (Some(lo), None) => {
                // No upper bound - cap to practical max
                let practical_max = proc_macro2::Literal::i128_unsuffixed(PRACTICAL_MAX);
                quote! { ((#lo as #arg_type)..=(#practical_max as #arg_type)) }
            }
            (None, Some(hi)) => {
                quote! { (0..=(#hi as #arg_type)) }
            }
            (None, None) => {
                quote! { ::verus_proptest::hidden::proptest::prelude::any::<#arg_type>() }
            }
        }
    }

    fn generate_element_range_strategy(
        &self,
        arg_index: usize,
        lo: &Option<Bound>,
        hi: &Option<Bound>,
    ) -> TokenStream {
        // Extract element type from Vec<T>
        let arg_type = self.arg_types[arg_index];
        let element_type = extract_vec_element_type(arg_type);

        let lo_expr = lo.as_ref().map(|b| bound_to_tokens(b, true));
        let hi_expr = hi.as_ref().map(|b| bound_to_tokens(b, false));

        let element_strategy = match (&lo_expr, &hi_expr) {
            (Some(lo), Some(hi)) => {
                quote! { ((#lo as #element_type)..=(#hi as #element_type)) }
            }
            (Some(lo), None) => {
                quote! { ((#lo as #element_type)..) }
            }
            (None, Some(hi)) => {
                quote! { (..=(#hi as #element_type)) }
            }
            (None, None) => {
                quote! { ::verus_proptest::hidden::proptest::prelude::any::<#element_type>() }
            }
        };

        quote! {
            ::verus_proptest::hidden::proptest::collection::vec(#element_strategy, 0..100)
        }
    }

    fn generate_sorted_strategy(&self, arg_index: usize, ascending: bool) -> TokenStream {
        // Extract element type from Vec<T>
        let arg_type = self.arg_types[arg_index];
        let element_type = extract_vec_element_type(arg_type);

        // Generate a random vector and then sort it
        if ascending {
            quote! {
                ::verus_proptest::hidden::proptest::collection::vec(
                    ::verus_proptest::hidden::proptest::prelude::any::<#element_type>(),
                    0..100
                ).prop_map(|mut v| { v.sort(); v })
            }
        } else {
            quote! {
                ::verus_proptest::hidden::proptest::collection::vec(
                    ::verus_proptest::hidden::proptest::prelude::any::<#element_type>(),
                    0..100
                ).prop_map(|mut v| { v.sort(); v.reverse(); v })
            }
        }
    }

    fn generate_dependent_strategy(&self) -> Option<TokenStream> {
        // For dependent constraints like `0 <= i < j < 20`, we need prop_flat_map
        // This is a simplified implementation for the common case of two dependent
        // variables

        // Find the LessThan constraints
        let less_than_constraints: Vec<_> = self
            .constraints
            .iter()
            .filter(|c| matches!(c.kind, ConstraintKind::LessThan { .. }))
            .collect();

        if less_than_constraints.is_empty() {
            return None;
        }

        // For now, handle the simple case: `i < j` with optional bounds on both
        // Generate: (lo..hi).prop_flat_map(|i| ((i+1)..hi).prop_map(move |j| (i, j)))

        let first = &less_than_constraints[0];
        let ConstraintKind::LessThan {
            other_arg,
            other_index,
            strict,
        } = &first.kind
        else {
            return None;
        };

        let first_name = syn::Ident::new(&first.arg_name, proc_macro2::Span::call_site());
        let second_name = syn::Ident::new(other_arg, proc_macro2::Span::call_site());

        // Find bounds for first variable
        let first_bounds = self.constraints.iter().find(|c| {
            c.arg_index == first.arg_index && matches!(c.kind, ConstraintKind::Range { .. })
        });

        // Find bounds for second variable
        let second_bounds = self.constraints.iter().find(|c| {
            c.arg_index == *other_index && matches!(c.kind, ConstraintKind::Range { .. })
        });

        let (first_lo, _first_hi) = match first_bounds.map(|c| &c.kind) {
            Some(ConstraintKind::Range { lo, hi }) => (lo.clone(), hi.clone()),
            _ => (None, None),
        };

        let (_second_lo, second_hi) = match second_bounds.map(|c| &c.kind) {
            Some(ConstraintKind::Range { lo, hi }) => (lo.clone(), hi.clone()),
            _ => (None, None),
        };

        // Use the more restrictive bounds
        let lo_expr = first_lo
            .as_ref()
            .map(|b| bound_to_tokens(b, true))
            .unwrap_or_else(|| quote! { 0 });
        let hi_expr = second_hi
            .as_ref()
            .map(|b| bound_to_tokens(b, false))
            .unwrap_or_else(|| quote! { 100 }); // reasonable default

        let offset = if *strict {
            quote! { 1 }
        } else {
            quote! { 0 }
        };

        Some(quote! {
            fn strategy() -> ::verus_proptest::hidden::proptest::strategy::BoxedStrategy<Self::Args> {
                use ::verus_proptest::hidden::proptest::prelude::*;
                (#lo_expr..#hi_expr).prop_flat_map(|#first_name| {
                    ((#first_name + #offset)..=#hi_expr).prop_map(move |#second_name| (#first_name, #second_name))
                }).boxed()
            }
        })
    }
}
