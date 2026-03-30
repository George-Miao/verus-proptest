#![allow(dead_code)]

use heck::ToUpperCamelCase;
use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, quote};
use syn::{
    Error, File, FnArg, FnArgKind, FnMode, Ident, Index, Item, ItemFn, ItemMacro, Pat, PatType,
    Result, Type, TypeReference,
    parse::{Parse, ParseStream},
    parse_macro_input, parse_quote,
    visit_mut::{VisitMut, visit_file_mut},
};

mod strategy;

#[proc_macro_attribute]
pub fn generate(_: proc_macro::TokenStream, _: proc_macro::TokenStream) -> proc_macro::TokenStream {
    Error::new(
        Span::call_site(),
        "This macro cannot not be called independently. Wrap `verus!` with \
         `#[verus_proptest::verus_proptest]` first.",
    )
    .into_compile_error()
    .into()
}

#[proc_macro_attribute]
pub fn verus_proptest(
    attr: proc_macro::TokenStream,
    input: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let attr = parse_macro_input!(attr as Attr);
    let mut body = parse_macro_input!(input as ItemMacro);
    let mut file = body.mac.parse_body::<File>().unwrap();

    let mut v = Visitor::new(attr);

    visit_file_mut(&mut v, &mut file);

    file.items.extend(v.generated);

    body.mac.tokens = file.to_token_stream();
    if let Some(e) = v.errors {
        body.mac.tokens.extend(e.to_compile_error());
    }
    body.to_token_stream().into()
}

#[derive(Clone)]
struct Attr {}

impl Parse for Attr {
    fn parse(_: ParseStream) -> Result<Self> {
        Ok(Self {})
    }
}

struct Visitor {
    attr: Attr,
    generated: Vec<Item>,
    errors: Option<Error>,
}

impl Visitor {
    fn new(attr: Attr) -> Self {
        Self {
            attr,
            generated: Default::default(),
            errors: Default::default(),
        }
    }
}

impl VisitMut for Visitor {
    fn visit_item_fn_mut(&mut self, i: &mut syn::ItemFn) {
        let mut generate = false;

        i.attrs.retain(|x| {
            let segs = &x.meta.path().segments;
            let matched = segs.first().is_some_and(|x| x.ident == "verus_proptest")
                && segs.iter().nth(1).is_some_and(|x| x.ident == "generate");
            generate |= matched;
            !matched
        });

        if !generate {
            return;
        }

        match FuncGenerator::new(i).generate_testable() {
            Ok(items) => self.generated.extend(items),
            Err(e) => self.errors = Some(e),
        }
    }
}

struct FuncGenerator<'a> {
    func: &'a ItemFn,
}

impl<'a> FuncGenerator<'a> {
    fn new(func: &'a ItemFn) -> Self {
        Self { func }
    }

    fn generate_testable(self) -> Result<[Item; 2]> {
        if !self.func.sig.generics.params.is_empty() {
            return Err(syn::Error::new_spanned(
                &self.func.sig.generics,
                "Generics are not supported",
            ));
        }

        if !matches!(self.func.sig.mode, FnMode::Default | FnMode::Exec(_)) {
            return Err(syn::Error::new_spanned(
                &self.func.sig.mode,
                "Only exec functions are supported",
            ));
        }

        if let Some(FnArg {
            kind: FnArgKind::Receiver(recv),
            ..
        }) = self.func.sig.inputs.first()
        {
            return Err(Error::new_spanned(recv, "Receiver is not supported"));
        };

        let ident = self.ident();
        let args = self.generate_args();
        let args_ty = self.generate_args_type();
        let ret = self.generate_ret();
        let ret_ty = self.generate_ret_type();
        let ret_type_const = self.generate_ret_type_const();
        let requires = self.generate_requires();
        let ensures = self.generate_ensures();
        let run = self.generate_run();
        let strategy = self.generate_strategy();

        let def_item: Item = parse_quote! {
            struct #ident;
        };

        let impl_item: Item = parse_quote! {
            impl ::verus_proptest::Testable for #ident {
                #args_ty
                #ret_ty

                #args
                #ret
                #ret_type_const
                #requires
                #ensures

                #run
                #strategy

            }
        };

        Ok([def_item, impl_item])
    }

    fn generate_run(&self) -> TokenStream {
        let fn_ident = &self.func.sig.ident;
        let refs = self.func.sig.inputs.iter().map(|x| {
            let FnArgKind::Typed(PatType { ty, .. }) = &x.kind else {
                unreachable!()
            };

            let refs = ref_stack(ty);
            let mut tt = TokenStream::new();
            for r in refs {
                match r {
                    Ref::Ref => tt.extend(quote! { & }),
                    Ref::Mut => tt.extend(quote! { &mut }),
                }
            }
            tt
        });
        let idx = (0..self.arg_count()).map(Index::from);

        quote! {
            fn run(args: Self::Args) -> Self::Ret {
                #fn_ident ( #( #refs args . #idx ),* )
            }
        }
    }

    fn generate_args(&self) -> TokenStream {
        let args = self.func.sig.inputs.iter().map(|x| {
            let FnArgKind::Typed(PatType { pat, ty, .. }) = &x.kind else {
                unreachable!()
            };

            let pattern = pat.to_token_stream().to_string();
            let refs = ref_stack(ty);

            quote! {
                ::verus_proptest::Arg {
                    pattern: #pattern,
                    ref_stack: ::verus_proptest::RefStack( &[#( #refs ),*] )
                }
            }
        });

        quote! {
            const ARGS: ::verus_proptest::Args = ::verus_proptest::Args( &[ #(#args),* ] );
        }
    }

    fn generate_args_type(&self) -> TokenStream {
        let args = self.func.sig.inputs.iter().map(|x| {
            let FnArgKind::Typed(PatType { ty, .. }) = &x.kind else {
                unreachable!()
            };

            without_ref(ty)
        });

        quote! {
            type Args = ( #( #args, )* );
        }
    }

    fn generate_ret(&self) -> Option<TokenStream> {
        let ret = self.ret()?.to_token_stream().to_string();
        Some(quote! { const RET: Option<&str> = Some( #ret ); })
    }

    fn generate_ret_type(&self) -> TokenStream {
        match &self.func.sig.output {
            syn::ReturnType::Default => quote! { type Ret = (); },
            syn::ReturnType::Type(_, _, _, ty) => quote! { type Ret = #ty; },
        }
    }

    fn generate_ret_type_const(&self) -> Option<TokenStream> {
        match &self.func.sig.output {
            syn::ReturnType::Default => None,
            syn::ReturnType::Type(_, _, _, ty) => {
                let ty_str = ty.to_token_stream().to_string();
                Some(quote! { const RET_TYPE: Option<&str> = Some( #ty_str ); })
            }
        }
    }

    fn generate_requires(&self) -> Option<TokenStream> {
        let requires = self.func.sig.spec.requires.as_ref()?.exprs.exprs.iter();
        let tt = quote! { #( assert(#requires); )*}.to_string();

        Some(quote! {
            const REQUIRES: Option<&str> = Some( #tt );
        })
    }

    fn generate_ensures(&self) -> Option<TokenStream> {
        let ensures = self.func.sig.spec.ensures.as_ref()?.exprs.exprs.iter();
        let tt = quote! { #( assert(#ensures); )* }.to_string();

        Some(quote! {
            const ENSURES: Option<&str> = Some( #tt );
        })
    }

    fn ident(&self) -> Ident {
        let ident = &self.func.sig.ident;
        Ident::new(&ident.to_string().to_upper_camel_case(), ident.span())
    }

    fn args(&self) -> impl Iterator<Item = &FnArg> {
        self.func.sig.inputs.iter()
    }

    fn arg_count(&self) -> usize {
        self.func.sig.inputs.len()
    }

    fn ret(&self) -> Option<&Pat> {
        match &self.func.sig.output {
            syn::ReturnType::Default => None,
            syn::ReturnType::Type(_, _, pat, _) => pat.as_ref().map(|x| &x.1),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ref {
    Ref,
    Mut,
}

impl ToTokens for Ref {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.extend(if *self == Ref::Mut {
            quote! { ::verus_proptest::Ref::Mut }
        } else {
            quote! { ::verus_proptest::Ref::Ref }
        });
    }
}

fn without_ref(ty: &Type) -> &Type {
    match ty {
        Type::Reference(type_reference) => without_ref(&type_reference.elem),
        ty => ty,
    }
}

fn ref_stack(mut ty: &Type) -> Vec<Ref> {
    let mut ret = vec![];
    loop {
        match ty {
            Type::Reference(TypeReference {
                mutability, elem, ..
            }) => {
                ret.push(if mutability.is_some() {
                    Ref::Mut
                } else {
                    Ref::Ref
                });
                ty = elem;
            }
            _ => return ret,
        }
    }
}
