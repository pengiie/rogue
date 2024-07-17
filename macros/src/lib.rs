use proc_macro::TokenStream;

mod generate_tuples;
mod resource;

#[proc_macro_derive(Resource)]
pub fn derive_resource(input: TokenStream) -> TokenStream {
    resource::impl_derive_resource(input)
}

/// Calls a macro implementation a number of times while generating generating generic arguments.
#[proc_macro]
pub fn generate_tuples(input: TokenStream) -> TokenStream {
    generate_tuples::impl_generate_tuples(input)
}
