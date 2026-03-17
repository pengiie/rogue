use proc_macro2::Ident;
use quote::quote;
use syn::{
    parse::{Parse, ParseStream},
    LitInt, Result,
};

struct GenerateTuplesInput {
    macro_impl: Ident,
    min_count: usize,
    max_count: usize,
}

impl Parse for GenerateTuplesInput {
    fn parse(input: ParseStream) -> Result<Self> {
        let macro_impl = input.parse::<Ident>()?;
        input.parse::<syn::token::Comma>()?;
        let min_count = input.parse::<LitInt>()?.base10_parse()?;
        input.parse::<syn::token::Comma>()?;
        let max_count = input.parse::<LitInt>()?.base10_parse()?;

        Ok(GenerateTuplesInput {
            macro_impl,
            min_count,
            max_count,
        })
    }
}

pub fn impl_generate_tuples(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let GenerateTuplesInput {
        macro_impl,
        min_count,
        max_count,
    } = syn::parse_macro_input!(input as GenerateTuplesInput);

    let mut gen = if min_count == 0 {
        vec![quote! { #macro_impl!(); }]
    } else {
        Vec::new()
    };
    let mut generics = Vec::new();
    let mut numbers = Vec::new();

    // 0 means no generics so sub 1 so generics and literals start at 0.
    let min_count = min_count.saturating_sub(1);
    for i in 0..min_count {
        let param = Ident::new(&format!("P{}", i), proc_macro2::Span::call_site());
        let lit = LitInt::new(&format!("{}", i), proc_macro2::Span::call_site());

        generics.push(quote! { #param });
        numbers.push(quote! { #lit });
    }
    for i in min_count..=max_count {
        let param = Ident::new(&format!("P{}", i), proc_macro2::Span::call_site());
        let lit = LitInt::new(&format!("{}", i), proc_macro2::Span::call_site());

        generics.push(quote! { #param });
        numbers.push(quote! { #lit });
        gen.push(quote! {
            #macro_impl!(#(#generics),* , #(#numbers),*);
        });
    }

    let gen = quote! {
        #(#gen)*
    };
    gen.into()
}
