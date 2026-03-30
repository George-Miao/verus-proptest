use databake::{Bake, CrateEnv, TokenStream};
use quote::{ToTokens, format_ident, quote};
use syn::{
    Block, Ident, Index, Pat,
    parse::{Parse, Parser},
};

#[cfg(target_pointer_width = "16")]
const SIZE_OF_USIZE: usize = 2;

#[cfg(target_pointer_width = "32")]
const SIZE_OF_USIZE: usize = 4;

#[cfg(target_pointer_width = "64")]
const SIZE_OF_USIZE: usize = 8;

use crate::*;

fn wraps(body: TokenStream) -> TokenStream {
    let macro_id = format_ident!("verus");
    let size_of_usize = Index::from(SIZE_OF_USIZE);

    quote! {
        extern crate alloc;

        use vstd::prelude::*;

        #macro_id! {
            global size_of usize == #size_of_usize;

            fn main() {
                #body
            }
        }
    }
}

pub struct RequiresCodegen<'a, T: Testable> {
    ctx: CrateEnv,
    args: &'a T::Args,
}

pub struct EnsuresCodegen<'a, T: Testable> {
    reqs: RequiresCodegen<'a, T>,
    ret: &'a T::Ret,
}

impl<'a, T: Testable> RequiresCodegen<'a, T> {
    pub fn new(args: &'a T::Args) -> Self {
        Self {
            ctx: CrateEnv::default(),
            args,
        }
    }

    fn arg_binding(&self) -> TokenStream {
        let var = format_ident!("args");
        let baked = self.args.bake(&self.ctx);
        let bind = T::ARGS.bind_to(&var);

        quote! {
            let #var = #baked;
            #bind
        }
    }

    pub fn codegen(&self) -> Option<TokenStream> {
        let reqs = T::REQUIRES?;
        let bind = self.arg_binding();
        let reqs = Block::parse_within
            .parse_str(reqs)
            .expect("failed to parse body");

        Some(wraps(quote! {
            #bind
            #( #reqs )*
        }))
    }
}

impl<'a, T: Testable> EnsuresCodegen<'a, T> {
    pub fn new(reqs: RequiresCodegen<'a, T>, ret: &'a T::Ret) -> Self {
        Self { reqs, ret }
    }

    pub fn codegen(&self) -> Option<TokenStream> {
        let ensures = T::ENSURES?;

        let arg_bind = self.reqs.arg_binding();

        let ret_pat = Pat::parse_single
            .parse_str(T::RET.unwrap_or("_"))
            .expect("failed to parse pattern");
        let baked = self.ret.bake(&self.reqs.ctx);
        let ensures = Block::parse_within
            .parse_str(ensures)
            .expect("failed to parse body");

        let ret_binding = if let Some(ret_type) = T::RET_TYPE {
            let ret_type_tokens = syn::Type::parse
                .parse_str(ret_type)
                .expect("failed to parse return type");
            quote! { let #ret_pat: #ret_type_tokens = #baked; }
        } else {
            quote! { let #ret_pat = #baked; }
        };

        Some(wraps(quote! {
            #arg_bind
            #ret_binding

            #( #ensures )*
        }))
    }
}

impl Args {
    fn bind_to(&self, args: &Ident) -> TokenStream {
        let i = (0..self.0.len()).map(syn::Index::from);
        let pat = self.0.iter().map(|x| {
            Pat::parse_single
                .parse_str(x.pattern)
                .expect("Failed to parse arg pattern")
        });

        quote! { #( let #pat = #args.#i; )* }
    }
}

impl ToTokens for RefStack {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        for st in self.0 {
            match st {
                Ref::Ref => tokens.extend(quote! { & }),
                Ref::Mut => tokens.extend(quote! { &mut }),
            }
        }
    }
}
