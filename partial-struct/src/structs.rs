use std::ops::Deref;

use proc_macro2::TokenStream;
use quote::ToTokens;
use syn::{self, Attribute, DeriveInput, Fields, Ident, ImplGenerics, Type, Visibility};

type FieldsVec = Vec<(Visibility, Ident, Type, Vec<Attribute>)>;

pub struct StructParts<'a> {
  pub attrs: &'a Vec<Attribute>,
  pub vis: &'a Visibility,
  pub ident: &'a proc_macro2::Ident,
  pub ty_generics: syn::TypeGenerics<'a>,
  pub impl_generics: ImplGenerics<'a>,
  pub where_clause: Option<&'a syn::WhereClause>,
  pub fields: &'a FieldsVec,
}

pub fn get_struct_parts<'a>(
  derive_input: &'a DeriveInput,
  fields_vec: &'a mut FieldsVec,
) -> StructParts<'a> {
  let DeriveInput {
    attrs,
    vis,
    ident,
    generics,
    data,
    ..
  } = derive_input;

  let fields = filter_fields(
    match data {
      syn::Data::Struct(ref s) => &s.fields,
      _ => panic!("Field can only be derived for structs"),
    },
    fields_vec,
  );

  let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

  StructParts {
    attrs,
    vis,
    ident,
    ty_generics,
    impl_generics,
    where_clause,
    fields,
  }
}

pub fn attrs_to_token_stream(attrs: &[Attribute]) -> TokenStream {
  TokenStream::from_iter(attrs.iter().map(|attr| attr.into_token_stream()))
}

fn filter_fields<'a>(fields: &'a Fields, fields_vec: &'a mut FieldsVec) -> &'a FieldsVec {
  fields_vec.extend(fields.into_iter().filter_map(|field| {
    if field.ident.is_some() {
      let field_vis = field.vis.clone();
      let field_ident = field
        .ident
        .as_ref()
        .expect("Failed to filter fields")
        .clone();
      let field_ty = field.ty.clone();
      Some((field_vis, field_ident, field_ty, field.attrs.clone()))
    } else {
      None
    }
  }));
  fields_vec
}

pub fn camel_case(value: impl Deref<Target = str>) -> String {
  let mut upper = false;
  value
    .chars()
    .fold(String::new(), |mut a, b| {
      if b == '_' {
        upper = true;
        return a;
      }
      if upper {
        b.to_uppercase().for_each(|c| a.push(c));
        upper = false;
      } else {
        a.push(b);
      }
      a
    })
    .trim()
    .to_string()
}
