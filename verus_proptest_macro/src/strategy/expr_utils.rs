use proc_macro2::TokenStream;
use quote::{ToTokens, quote};
use syn::{BinOp, Expr, Lit, Pat, Type};

use super::{Bound, BoundValue};

pub fn extract_vec_element_type(ty: &Type) -> &Type {
    if let Type::Path(type_path) = ty {
        if let Some(segment) = type_path.path.segments.last() {
            if segment.ident == "Vec" {
                if let syn::PathArguments::AngleBracketed(args) = &segment.arguments {
                    if let Some(syn::GenericArgument::Type(inner_ty)) = args.args.first() {
                        return inner_ty;
                    }
                }
            }
        }
    }
    ty
}

pub fn bound_to_tokens(bound: &Bound, is_lower: bool) -> TokenStream {
    let value = match &bound.value {
        BoundValue::Literal(n) => {
            let lit = proc_macro2::Literal::i128_unsuffixed(*n);
            quote! { #lit }
        }
        BoundValue::MaxValue(s) => {
            let path: syn::Path =
                syn::parse_str(s).unwrap_or_else(|_| syn::parse_str("usize::MAX").unwrap());
            quote! { #path }
        }
        BoundValue::Expr(s) => {
            let expr: syn::Expr =
                syn::parse_str(s).unwrap_or_else(|_| syn::parse_str("0").unwrap());
            quote! { #expr }
        }
    };

    if !bound.inclusive {
        if is_lower {
            quote! { (#value + 1) }
        } else {
            quote! { (#value - 1) }
        }
    } else {
        value
    }
}

pub fn pat_to_name(pat: &Pat) -> Option<String> {
    match pat {
        Pat::Ident(ident) => Some(ident.ident.to_string()),
        _ => None,
    }
}

pub fn expr_to_arg_name(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Path(path) => {
            if path.path.segments.len() == 1 {
                Some(path.path.segments[0].ident.to_string())
            } else {
                None
            }
        }
        Expr::Paren(paren) => expr_to_arg_name(&paren.expr),
        _ => None,
    }
}

pub fn expr_to_literal(expr: &Expr) -> Option<BoundValue> {
    match expr {
        Expr::Lit(lit) => match &lit.lit {
            Lit::Int(int_lit) => int_lit.base10_parse::<i128>().ok().map(BoundValue::Literal),
            _ => None,
        },
        Expr::Path(path) => {
            let path_str = path.to_token_stream().to_string().replace(' ', "");
            if path_str.contains("MAX") || path_str.contains("MIN") {
                Some(BoundValue::MaxValue(path_str))
            } else {
                None
            }
        }
        Expr::Paren(paren) => expr_to_literal(&paren.expr),
        _ => None,
    }
}

pub fn expr_to_max_const(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Path(path) => {
            let path_str = path.to_token_stream().to_string().replace(' ', "");
            if path_str.ends_with("::MAX") {
                Some(path_str)
            } else {
                None
            }
        }
        _ => None,
    }
}

pub fn extract_primary_variable(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Path(path) => {
            if path.path.segments.len() == 1 {
                Some(path.path.segments[0].ident.to_string())
            } else {
                None
            }
        }
        Expr::Binary(bin) => {
            extract_primary_variable(&bin.left).or_else(|| extract_primary_variable(&bin.right))
        }
        Expr::Paren(paren) => extract_primary_variable(&paren.expr),
        _ => None,
    }
}

pub fn compute_linear_factor(expr: &Expr, var_name: &str) -> Option<i128> {
    match expr {
        Expr::Path(path) => {
            if path.path.segments.len() == 1 && path.path.segments[0].ident.to_string() == var_name
            {
                Some(1)
            } else {
                None
            }
        }
        Expr::Binary(bin) => match bin.op {
            BinOp::Add(_) => {
                let left = compute_linear_factor(&bin.left, var_name).unwrap_or(0);
                let right = compute_linear_factor(&bin.right, var_name).unwrap_or(0);
                Some(left + right)
            }
            BinOp::Mul(_) => {
                if let Some(const_val) = expr_to_literal(&bin.left) {
                    if let BoundValue::Literal(n) = const_val {
                        if let Some(inner) = compute_linear_factor(&bin.right, var_name) {
                            return Some(n * inner);
                        }
                    }
                }
                if let Some(const_val) = expr_to_literal(&bin.right) {
                    if let BoundValue::Literal(n) = const_val {
                        if let Some(inner) = compute_linear_factor(&bin.left, var_name) {
                            return Some(n * inner);
                        }
                    }
                }
                None
            }
            _ => None,
        },
        Expr::Paren(paren) => compute_linear_factor(&paren.expr, var_name),
        _ => None,
    }
}
