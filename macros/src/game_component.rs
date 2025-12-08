use proc_macro::TokenStream;
use quote::quote;
use syn::{parse::Parse, parse_macro_input, spanned::Spanned, DeriveInput};

struct GameComponentArgs {
    name: syn::LitStr,
    is_constructible: bool,
}

impl Parse for GameComponentArgs {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let arg = input.parse::<syn::Ident>()?;
        if &arg != "name" {
            return syn::Result::Err(syn::Error::new(
                arg.span(),
                "Expected first argument as name",
            ));
        }
        input.parse::<syn::Token![=]>()?;
        let name = input.parse::<syn::LitStr>()?;

        // Assume component is constructible if using this macro unless specified otherwise.
        let mut is_constructible = true;
        if let Ok(_) = input.parse::<syn::Token![,]>() {
            let arg = input.parse::<syn::Ident>()?;
            if &arg != "constructible" {
                return syn::Result::Err(syn::Error::new(
                    arg.span(),
                    "Expected second argument to be constructible",
                ));
            }
            input.parse::<syn::Token![=]>()?;
            is_constructible = input.parse::<syn::LitBool>()?.value;
        }
        return Ok(GameComponentArgs {
            name,
            is_constructible,
        });
    }
}

pub fn impl_game_component_attr(attr: TokenStream, input: TokenStream) -> TokenStream {
    let item = parse_macro_input!(input as syn::ItemStruct);
    let mut game_component_args = parse_macro_input!(attr as GameComponentArgs);

    let name = &item.ident;
    let game_component_serde_name = game_component_args.name;
    let is_constructible = game_component_args.is_constructible;

    let constructible_impl = if is_constructible {
        quote! {
            fn is_constructible() -> bool {
                true
            }

            fn construct_component(dst_ptr: *mut u8) {
                let dst_ptr = dst_ptr as *mut Self;
                // Safety: dst_ptr should be allocated with the memory layout for this type.
                unsafe { dst_ptr.write(std::default::Default::default()) };
            }
        }
    } else {
        quote! {}
    };

    let gen = quote! {
        #item

        impl crate::engine::entity::component::GameComponent for #name {
            const NAME: &str = #game_component_serde_name;

            #constructible_impl

            fn clone_component(
                &self,
                ctx: &mut crate::engine::entity::component::GameComponentCloneContext<'_>,
                dst_ptr: *mut u8,
            ) {
                let dst_ptr = dst_ptr as *mut Self;
                // Safety: dst_ptr should be allocated with the memory layout for this type.
                unsafe { dst_ptr.write(std::clone::Clone::clone(self)) };
            }

            fn serialize_component(
                &self,
                ctx: &crate::engine::entity::component::GameComponentSerializeContext<'_>,
                ser: &mut dyn erased_serde::Serializer,
            ) -> erased_serde::Result<()> {
                erased_serde::Serialize::erased_serialize(self, ser)
            }

            unsafe fn deserialize_component(
                ctx: &mut crate::engine::entity::component::GameComponentDeserializeContext<'_>,
                de: &mut dyn erased_serde::Deserializer,
                dst_ptr: *mut u8,
            ) -> erased_serde::Result<()> {
                let dst_ptr = dst_ptr as *mut Self;
                // Safety: dst_ptr should be allocated with the memory layout for this type.
                unsafe { dst_ptr.write(erased_serde::deserialize::<Self>(de)?) };
                Ok(())
            }
        }
    };

    gen.into()
}
