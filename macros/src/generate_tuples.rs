use proc_macro2::Ident;
use quote::quote;
use syn::{
    parse::{Parse, ParseStream},
    LitInt, Result,
};

struct GenerateTuplesInput {
    macro_impl: Ident,
    count: usize,
}

impl Parse for GenerateTuplesInput {
    fn parse(input: ParseStream) -> Result<Self> {
        let macro_impl = input.parse::<Ident>()?;
        input.parse::<syn::token::Comma>()?;
        let count = input.parse::<LitInt>()?.base10_parse()?;

        Ok(GenerateTuplesInput { macro_impl, count })
    }
}

pub fn impl_generate_tuples(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let GenerateTuplesInput { macro_impl, count } =
        syn::parse_macro_input!(input as GenerateTuplesInput);

    let mut gen = vec![quote! { #macro_impl!(); }];
    let mut generics = Vec::new();

    for i in 0..count {
        let param = Ident::new(&format!("P{}", i), proc_macro2::Span::call_site());

        generics.push(quote! { #param });
        gen.push(quote! {
            #macro_impl!(#(#generics),*);
        });
    }

    let gen = quote! {
        #(#gen)*
    };
    gen.into()
}
