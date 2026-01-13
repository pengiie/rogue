use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput};

pub fn impl_derive_resource(input: TokenStream) -> TokenStream {
    let ast = parse_macro_input!(input as DeriveInput);

    let name = &ast.ident;

    let crate_name = match proc_macro_crate::crate_name("rogue_engine") {
        Ok(proc_macro_crate::FoundCrate::Itself) => quote! { crate },
        Ok(proc_macro_crate::FoundCrate::Name(name)) => {
            let ident = syn::Ident::new(&name, proc_macro2::Span::call_site());
            quote! { #ident }
        }
        Err(_) => panic!("Couldn't figure out path for rogue_engine crate"),
    };

    let gen = quote! {
        impl #crate_name::resource::Resource for #name {}
    };

    gen.into()
}
