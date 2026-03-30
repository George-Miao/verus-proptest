use syn::{BinOp, Expr, ExprBinary, ExprClosure, ExprUnary, UnOp};

use super::{
    ArgConstraint, Bound, BoundValue, ConstraintKind, StrategyAnalyzer,
    expr_utils::{
        compute_linear_factor, expr_to_arg_name, expr_to_literal, expr_to_max_const,
        extract_primary_variable,
    },
};

impl StrategyAnalyzer<'_> {
    pub fn analyze_constraint(&mut self, expr: &Expr) {
        if self.try_match_simple_range(expr) {
            return;
        }
        if self.try_match_chained_comparison(expr) {
            return;
        }
        if self.try_match_forall_element_constraint(expr) {
            return;
        }
        if self.try_match_forall_unary(expr) {
            return;
        }
        if self.try_match_forall_sorted(expr) {
            return;
        }
        if self.try_match_arithmetic_bound(expr) {
            return;
        }
    }

    /// Match patterns like `10 < x`, `x < 20`, `x <= 20`, `10 <= x`
    fn try_match_simple_range(&mut self, expr: &Expr) -> bool {
        let Expr::Binary(bin) = expr else {
            return false;
        };

        match &bin.op {
            BinOp::Lt(_) | BinOp::Le(_) | BinOp::Gt(_) | BinOp::Ge(_) => {}
            _ => return false,
        }

        let (left_name, right_name) = (expr_to_arg_name(&bin.left), expr_to_arg_name(&bin.right));
        let (left_lit, right_lit) = (expr_to_literal(&bin.left), expr_to_literal(&bin.right));

        // Case: `x < 20` or `x <= 20`
        if let (Some(arg_name), Some(lit_val)) = (&left_name, &right_lit) {
            if let Some(arg_index) = self.find_arg_index(arg_name) {
                let inclusive = matches!(bin.op, BinOp::Le(_));
                self.add_upper_bound(arg_name.clone(), arg_index, lit_val.clone(), inclusive);
                return true;
            }
        }

        // Case: `10 < x` or `10 <= x`
        if let (Some(lit_val), Some(arg_name)) = (&left_lit, &right_name) {
            if let Some(arg_index) = self.find_arg_index(arg_name) {
                let inclusive = matches!(bin.op, BinOp::Le(_));
                self.add_lower_bound(arg_name.clone(), arg_index, lit_val.clone(), inclusive);
                return true;
            }
        }

        // Case: `x > 10` or `x >= 10`
        if let (Some(arg_name), Some(lit_val)) = (&left_name, &right_lit) {
            if let Some(arg_index) = self.find_arg_index(arg_name) {
                let inclusive = matches!(bin.op, BinOp::Ge(_));
                self.add_lower_bound(arg_name.clone(), arg_index, lit_val.clone(), inclusive);
                return true;
            }
        }

        // Case: `20 > x` or `20 >= x`
        if let (Some(lit_val), Some(arg_name)) = (&left_lit, &right_name) {
            if let Some(arg_index) = self.find_arg_index(arg_name) {
                let inclusive = matches!(bin.op, BinOp::Ge(_));
                self.add_upper_bound(arg_name.clone(), arg_index, lit_val.clone(), inclusive);
                return true;
            }
        }

        // Case: `x < y` where both are arguments (dependent constraint)
        if let (Some(left_arg), Some(right_arg)) = (&left_name, &right_name) {
            if let (Some(left_idx), Some(right_idx)) = (
                self.find_arg_index(left_arg),
                self.find_arg_index(right_arg),
            ) {
                let strict = matches!(bin.op, BinOp::Lt(_) | BinOp::Gt(_));
                self.constraints.push(ArgConstraint {
                    arg_name: left_arg.clone(),
                    arg_index: left_idx,
                    kind: ConstraintKind::LessThan {
                        other_arg: right_arg.clone(),
                        other_index: right_idx,
                        strict,
                    },
                });
                return true;
            }
        }

        false
    }

    /// Match chained comparisons like `0 <= i < j < 20`
    fn try_match_chained_comparison(&mut self, expr: &Expr) -> bool {
        let Expr::Binary(outer) = expr else {
            return false;
        };

        // Check for pattern: `(a <= b) && (b < c)`
        if matches!(outer.op, BinOp::And(_)) {
            let left_matched = self.try_match_simple_range(&outer.left);
            let right_matched = self.try_match_simple_range(&outer.right);
            return left_matched || right_matched;
        }

        // Handle Verus chained comparison syntax: `a <= b < c`
        // These get parsed as nested binary expressions
        if let Expr::Binary(inner) = outer.left.as_ref() {
            // Pattern: `(lo <= x) < hi` means lo <= x && x < hi
            if let Some(middle_name) = expr_to_arg_name(&inner.right) {
                if let Some(arg_index) = self.find_arg_index(&middle_name) {
                    // Check left side: `lo <= x` or `lo < x`
                    if let Some(lo_lit) = expr_to_literal(&inner.left) {
                        let lo_inclusive = matches!(inner.op, BinOp::Le(_));
                        self.add_lower_bound(middle_name.clone(), arg_index, lo_lit, lo_inclusive);
                    }

                    // Check right side: `x < hi` or `x <= hi`
                    if let Some(hi_lit) = expr_to_literal(&outer.right) {
                        let hi_inclusive = matches!(outer.op, BinOp::Le(_));
                        self.add_upper_bound(middle_name, arg_index, hi_lit, hi_inclusive);
                    }

                    return true;
                }
            }
        }

        false
    }

    /// Match forall patterns like `forall|i: int| 0 <= i < s.len() ==> lo <=
    /// s[i] <= hi`
    fn try_match_forall_element_constraint(&mut self, expr: &Expr) -> bool {
        let Expr::MethodCall(method) = expr else {
            return false;
        };

        if let Some(first_arg) = method.args.first() {
            if let Expr::Closure(closure) = first_arg {
                return self.try_extract_element_constraint_from_closure(closure);
            }
        }

        false
    }

    /// Match Verus forall expressions parsed as unary expressions:
    /// `forall|i: int| 0 <= i < s.len() ==> 65 <= s[i] <= 90`
    fn try_match_forall_unary(&mut self, expr: &Expr) -> bool {
        let Expr::Unary(ExprUnary {
            op: UnOp::Forall(_),
            expr: inner,
            ..
        }) = expr
        else {
            return false;
        };

        let Expr::Closure(closure) = inner.as_ref() else {
            return false;
        };

        self.try_extract_element_constraint_from_closure(closure)
    }

    /// Match forall sorted pattern: `forall|i, j| 0 <= i < j < s.len() ==> s[i]
    /// <= s[j]`
    fn try_match_forall_sorted(&mut self, expr: &Expr) -> bool {
        let Expr::Unary(ExprUnary {
            op: UnOp::Forall(_),
            expr: inner,
            ..
        }) = expr
        else {
            return false;
        };

        let Expr::Closure(closure) = inner.as_ref() else {
            return false;
        };

        // Check if closure has two parameters (i, j pattern)
        if closure.inputs.len() != 2 {
            return false;
        }

        let Expr::Binary(bin) = closure.body.as_ref() else {
            return false;
        };

        if !matches!(bin.op, BinOp::Imply(_)) {
            return false;
        }

        // Check if RHS is a comparison between indexed elements: s[i] <= s[j]
        if let Some((collection_name, ascending)) = self.extract_sorted_constraint(&bin.right) {
            if let Some(arg_index) = self.find_arg_index(&collection_name) {
                self.constraints.push(ArgConstraint {
                    arg_name: collection_name,
                    arg_index,
                    kind: ConstraintKind::Sorted { ascending },
                });
                return true;
            }
        }

        false
    }

    fn extract_sorted_constraint(&self, expr: &Expr) -> Option<(String, bool)> {
        let Expr::Binary(bin) = expr else {
            return None;
        };

        // Check for s[i] <= s[j] or s[i] < s[j] pattern
        let ascending = match &bin.op {
            BinOp::Le(_) | BinOp::Lt(_) => true,
            BinOp::Ge(_) | BinOp::Gt(_) => false,
            _ => return None,
        };

        // Extract collection names from both sides
        let left_collection = self.extract_collection_from_index(&bin.left)?;
        let right_collection = self.extract_collection_from_index(&bin.right)?;

        // Both should be the same collection
        if left_collection == right_collection {
            Some((left_collection, ascending))
        } else {
            None
        }
    }

    fn try_extract_element_constraint_from_closure(&mut self, closure: &ExprClosure) -> bool {
        let Expr::Binary(bin) = closure.body.as_ref() else {
            return false;
        };

        // Check for implication operator
        if !matches!(bin.op, BinOp::Imply(_)) {
            return false;
        }

        // The RHS of the implication contains the element constraint
        self.try_extract_element_bounds(&bin.right)
    }

    fn try_extract_element_bounds(&mut self, expr: &Expr) -> bool {
        let Expr::Binary(bin) = expr else {
            return false;
        };

        // Check for chained comparison on indexed expression
        // Pattern: `(lo <= s[i]) <= hi` or `(lo <= s[i]) < hi`
        if let Expr::Binary(inner) = bin.left.as_ref() {
            if let Some((collection_name, lo, hi)) =
                self.extract_indexed_bounds(inner, &bin.right, &bin.op)
            {
                if let Some(arg_index) = self.find_arg_index(&collection_name) {
                    self.constraints.push(ArgConstraint {
                        arg_name: collection_name,
                        arg_index,
                        kind: ConstraintKind::ElementRange { lo, hi },
                    });
                    return true;
                }
            }
        }

        // Also try to match single-sided bounds like `s[i] >= 65`
        if let Some((collection_name, lo, hi)) = self.extract_single_indexed_bound(bin) {
            if let Some(arg_index) = self.find_arg_index(&collection_name) {
                // Merge with existing constraint if present
                if let Some(existing) = self.constraints.iter_mut().find(|c| {
                    c.arg_name == collection_name
                        && matches!(c.kind, ConstraintKind::ElementRange { .. })
                }) {
                    if let ConstraintKind::ElementRange {
                        lo: ref mut existing_lo,
                        hi: ref mut existing_hi,
                    } = existing.kind
                    {
                        if lo.is_some() {
                            *existing_lo = lo;
                        }
                        if hi.is_some() {
                            *existing_hi = hi;
                        }
                    }
                } else {
                    self.constraints.push(ArgConstraint {
                        arg_name: collection_name,
                        arg_index,
                        kind: ConstraintKind::ElementRange { lo, hi },
                    });
                }
                return true;
            }
        }

        false
    }

    fn extract_single_indexed_bound(
        &self,
        bin: &ExprBinary,
    ) -> Option<(String, Option<Bound>, Option<Bound>)> {
        // Match patterns like `s[i] >= 65` or `65 <= s[i]`
        let (indexed_expr, lit_expr, is_upper) = match &bin.op {
            BinOp::Le(_) | BinOp::Lt(_) => {
                // `s[i] <= hi` or `lo <= s[i]`
                if let Some(name) = self.extract_collection_from_index(&bin.left) {
                    (name, &bin.right, true)
                } else if let Some(name) = self.extract_collection_from_index(&bin.right) {
                    (name, &bin.left, false)
                } else {
                    return None;
                }
            }
            BinOp::Ge(_) | BinOp::Gt(_) => {
                // `s[i] >= lo` or `hi >= s[i]`
                if let Some(name) = self.extract_collection_from_index(&bin.left) {
                    (name, &bin.right, false)
                } else if let Some(name) = self.extract_collection_from_index(&bin.right) {
                    (name, &bin.left, true)
                } else {
                    return None;
                }
            }
            _ => return None,
        };

        let bound_value = expr_to_literal(lit_expr)?;
        let inclusive = matches!(bin.op, BinOp::Le(_) | BinOp::Ge(_));
        let bound = Some(Bound {
            value: bound_value,
            inclusive,
        });

        if is_upper {
            Some((indexed_expr, None, bound))
        } else {
            Some((indexed_expr, bound, None))
        }
    }

    fn extract_indexed_bounds(
        &self,
        inner: &ExprBinary,
        outer_right: &Expr,
        outer_op: &BinOp,
    ) -> Option<(String, Option<Bound>, Option<Bound>)> {
        // Pattern: `(lo <= s[i]) <= hi`
        // inner.left = lo, inner.right = s[i], outer_right = hi

        let indexed = &inner.right;
        let collection_name = self.extract_collection_from_index(indexed)?;

        let lo = expr_to_literal(&inner.left).map(|v| Bound {
            value: v,
            inclusive: matches!(inner.op, BinOp::Le(_)),
        });

        let hi = expr_to_literal(outer_right).map(|v| Bound {
            value: v,
            inclusive: matches!(outer_op, BinOp::Le(_)),
        });

        Some((collection_name, lo, hi))
    }

    fn extract_collection_from_index(&self, expr: &Expr) -> Option<String> {
        match expr {
            Expr::Index(idx) => expr_to_arg_name(&idx.expr),
            Expr::MethodCall(mc) if mc.method == "index" => expr_to_arg_name(&mc.receiver),
            _ => None,
        }
    }

    /// Match arithmetic bounds like `n + (2 * n) <= usize::MAX`
    fn try_match_arithmetic_bound(&mut self, expr: &Expr) -> bool {
        let Expr::Binary(bin) = expr else {
            return false;
        };

        if !matches!(bin.op, BinOp::Le(_) | BinOp::Lt(_)) {
            return false;
        }

        // Check if right side is a MAX constant
        let Some(max_type) = expr_to_max_const(&bin.right) else {
            return false;
        };

        // Extract variable from left side arithmetic expression
        let Some(arg_name) = extract_primary_variable(&bin.left) else {
            return false;
        };
        let Some(arg_index) = self.find_arg_index(&arg_name) else {
            return false;
        };

        // For `n + (2 * n) <= usize::MAX`, we need `3n <= MAX`, so `n <= MAX/3`
        if let Some(factor) = compute_linear_factor(&bin.left, &arg_name) {
            let bound_value = match max_type.as_str() {
                "usize::MAX" => BoundValue::Expr(format!("usize::MAX / {}", factor)),
                "u64::MAX" => BoundValue::Expr(format!("u64::MAX / {}", factor)),
                "u32::MAX" => BoundValue::Expr(format!("u32::MAX / {}", factor)),
                "u16::MAX" => BoundValue::Expr(format!("u16::MAX / {}", factor)),
                "u8::MAX" => BoundValue::Expr(format!("u8::MAX / {}", factor)),
                "i64::MAX" => BoundValue::Expr(format!("i64::MAX / {}", factor)),
                "i32::MAX" => BoundValue::Expr(format!("i32::MAX / {}", factor)),
                _ => return false,
            };

            self.add_upper_bound(
                arg_name,
                arg_index,
                bound_value,
                matches!(bin.op, BinOp::Le(_)),
            );
            return true;
        }

        false
    }
}
