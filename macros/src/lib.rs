#![allow(warnings)]

use proc_macro::TokenStream;

mod game_component;
mod generate_tuples;
mod resource;

#[proc_macro_derive(Resource)]
pub fn derive_resource(input: TokenStream) -> TokenStream {
    resource::impl_derive_resource(input)
}

#[proc_macro_attribute]
pub fn game_component(attr: TokenStream, input: TokenStream) -> TokenStream {
    game_component::impl_game_component_attr(attr, input)
}

#[proc_macro_derive(GameComponent, attributes(game_component_field))]
pub fn derive_game_component(input: TokenStream) -> TokenStream {
    TokenStream::new()
}

/// Calls a macro implementation a number of times while generating generating generic arguments.
#[proc_macro]
pub fn generate_tuples(input: TokenStream) -> TokenStream {
    generate_tuples::impl_generate_tuples(input)
}
