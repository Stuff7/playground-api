extern crate proc_macro;

mod structs;

use proc_macro2::TokenStream;
use quote::{__private::Span, quote, ToTokens};
use structs::camel_case;
use syn::{self, Ident};

use format as f;

#[proc_macro_attribute]
pub fn partial(
  _: proc_macro::TokenStream,
  input: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
  let input: TokenStream = input.into();
  let derive_input = syn::parse(input.clone().into()).expect("syn::parse failed");
  let mut fields_vec = Vec::new();
  let structs::StructParts {
    attrs,
    vis,
    ident: ty,
    ty_generics,
    impl_generics,
    where_clause,
    fields,
  } = structs::get_struct_parts(&derive_input, &mut fields_vec);

  let mut derives_default = false;
  let mut derives_serialize = false;
  let mut derives = Vec::new();
  for attr in attrs {
    if attr.path.is_ident("derive") {
      let tokens = f!("{}", attr.tokens);
      derives_default = derives_default || tokens.contains("Default");
      derives_serialize = derives_serialize || tokens.contains("Serialize");
    }
    derives.push(attr.into_token_stream());
  }
  let derive = if derives.is_empty() {
    TokenStream::new()
  } else {
    TokenStream::from_iter(derives.into_iter())
  };
  let partial_ident = Ident::new(&f!("Partial{}", ty), Span::call_site());

  let _field_var = fields.iter().map(|(vis, ident, ty, attrs)| {
    let is_option = f!("{}", ty.to_token_stream()).contains("Option");
    let ty = if is_option {
      quote! {#ty}
    } else {
      quote! {core::option::Option<#ty>}
    };
    let attrs = structs::attrs_to_token_stream(attrs);
    let missing_skip_attr = !f!("{attrs}").contains("skip_serializing_if");
    let serde_skip_serializing = if derives_serialize && missing_skip_attr {
      quote! {#[serde(skip_serializing_if = "Option::is_none")]}
    } else {
      quote! {}
    };
    quote! {
      #serde_skip_serializing
      #attrs
      #vis #ident: #ty
    }
  });
  let convert_branch = fields.iter().map(|(_vis, ident, ty, _attrs)| {
    let is_option = f!("{}", ty.to_token_stream()).contains("Option");
    let ty = if is_option {
      quote! {src.#ident}
    } else {
      quote! {Some(src.#ident)}
    };
    quote! {
      #ident: #ty
    }
  });

  let default_derive = if derives_default {
    quote! {}
  } else {
    quote! { #[derive(Default)] }
  };
  let tokens = quote! {
    #input
    #default_derive
    #derive
    #vis struct #partial_ident #ty_generics
      #where_clause
    {
      #(#_field_var),*
    }

    impl #impl_generics From<#ty #ty_generics> for #partial_ident #ty_generics
      #where_clause
    {
      fn from(src: #ty #ty_generics) -> #partial_ident #ty_generics {
        #partial_ident {
          #(#convert_branch),*
        }
      }
    }
  };

  tokens.into()
}

#[proc_macro_attribute]
pub fn omit_and_create(
  struct_name: proc_macro::TokenStream,
  input: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
  let struct_name: TokenStream = struct_name.into();
  let derive_input = syn::parse(input).expect("syn::parse failed");
  let mut fields_vec = Vec::new();
  let structs::StructParts {
    attrs,
    vis,
    ident,
    ty_generics,
    where_clause,
    fields,
    ..
  } = structs::get_struct_parts(&derive_input, &mut fields_vec);
  let derive = TokenStream::from_iter(attrs.iter().map(|a| a.into_token_stream()));

  let fields_omit = fields.iter().filter_map(|(vis, ident, ty, attrs)| {
    let attrs = structs::attrs_to_token_stream(attrs);
    let omit = f!("{attrs}").contains("omit");
    if omit {
      None
    } else {
      Some(quote! {
        #attrs
        #vis #ident: #ty
      })
    }
  });
  let fields = fields.iter().map(|(vis, ident, ty, attrs)| {
    let attrs = TokenStream::from_iter(attrs.iter().filter_map(|attr| {
      if attr.path.is_ident("omit") {
        None
      } else {
        Some(attr.into_token_stream())
      }
    }));
    quote! {
      #attrs
      #vis #ident: #ty
    }
  });
  let tokens = quote! {
    #derive
    #vis struct #ident #ty_generics
      #where_clause
    {
      #(#fields),*
    }
    #derive
    #vis struct #struct_name #ty_generics
      #where_clause
    {
      #(#fields_omit),*
    }
  };

  tokens.into()
}

#[proc_macro_derive(CamelFields)]
pub fn camel_fields(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
  let derive_input = syn::parse(input).expect("syn::parse failed");
  let mut fields_vec = Vec::new();
  let structs::StructParts { ident, fields, .. } =
    structs::get_struct_parts(&derive_input, &mut fields_vec);

  let functions = fields.iter().map(|(vis, ident, _ty, _attrs)| {
    let camel_field = camel_case(f!("{ident}"));
    quote! {
      #vis fn #ident() -> &'static str {
        #camel_field
      }
    }
  });

  let tokens = quote! {
    impl #ident {
      #(#functions)*
    }
  };

  tokens.into()
}
