use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{
  Data, DeriveInput, Expr, ExprLit, Fields, Lit, Path, Type, parse_macro_input, spanned::Spanned,
};

#[proc_macro_derive(SdkEnum, attributes(sdk))]
pub fn sdk_enum(input: TokenStream) -> TokenStream {
  let input = parse_macro_input!(input as DeriveInput);
  match expand_sdk_enum(&input) {
    Ok(tokens) => tokens.into(),
    Err(error) => error.to_compile_error().into(),
  }
}

#[proc_macro_derive(SdkObject, attributes(sdk))]
pub fn sdk_object(input: TokenStream) -> TokenStream {
  let input = parse_macro_input!(input as DeriveInput);
  match expand_sdk_object(&input) {
    Ok(tokens) => tokens.into(),
    Err(error) => error.to_compile_error().into(),
  }
}

fn expand_sdk_object(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
  let name = &input.ident;
  let validate = sdk_validate(input)?;
  let fields = match &input.data {
    Data::Struct(data) => match &data.fields {
      Fields::Named(fields) => fields,
      _ => {
        return Err(syn::Error::new(
          data.fields.span(),
          "SdkObject requires a struct with named fields",
        ));
      }
    },
    _ => {
      return Err(syn::Error::new(
        input.span(),
        "SdkObject can only be derived for structs",
      ));
    }
  };

  let mut read_fields = Vec::new();
  let mut write_fields = Vec::new();
  let mut size_fields = Vec::new();

  for field in &fields.named {
    let ident = field.ident.as_ref().expect("named field");
    let ty = &field.ty;

    if let Some((read_method, write_method, size)) = primitive_io(ty) {
      read_fields.push(quote! {
          #ident: reader.#read_method()?
      });
      write_fields.push(quote! {
          writer.#write_method(self.#ident)?;
      });
      size_fields.push(quote! { #size });
    } else if let Some(len) = byte_array_len(ty) {
      read_fields.push(quote! {
          #ident: reader.read_array::<#len>()?
      });
      write_fields.push(quote! {
          writer.write_all(&self.#ident)?;
      });
      size_fields.push(quote! { #len });
    } else {
      read_fields.push(quote! {
          #ident: <#ty as ::emfsdk::common::SdkRead>::read_from(reader)?
      });
      write_fields.push(quote! {
          <#ty as ::emfsdk::common::SdkWrite>::write_to(&self.#ident, writer)?;
      });
      size_fields.push(quote! {
          <#ty as ::emfsdk::common::SdkSize>::sdk_size(&self.#ident)
      });
    }
  }

  let validate_read = validate
    .as_ref()
    .map(|path| {
      quote! {
          #path(&value)?;
      }
    })
    .unwrap_or_default();
  let validate_write = validate
    .as_ref()
    .map(|path| {
      quote! {
          #path(self)?;
      }
    })
    .unwrap_or_default();

  Ok(quote! {
      impl ::emfsdk::common::SdkRead for #name {
          fn read_from<R: ::std::io::Read + ::std::io::Seek>(
              reader: &mut ::emfsdk::common::Reader<R>,
          ) -> ::emfsdk::common::Result<Self> {
              let value = Self {
                  #(#read_fields,)*
              };
              #validate_read
              Ok(value)
          }
      }

      impl ::emfsdk::common::SdkWrite for #name {
          fn write_to<W: ::std::io::Write>(
              &self,
              writer: &mut ::emfsdk::common::Writer<W>,
          ) -> ::emfsdk::common::Result<()> {
              #validate_write
              #(#write_fields)*
              Ok(())
          }
      }

      impl ::emfsdk::common::SdkSize for #name {
          fn sdk_size(&self) -> u64 {
              0 #(+ #size_fields as u64)*
          }
      }
  })
}

fn sdk_validate(input: &DeriveInput) -> syn::Result<Option<Path>> {
  let mut validate = None;
  for attr in &input.attrs {
    if !attr.path().is_ident("sdk") {
      continue;
    }
    attr.parse_nested_meta(|meta| {
      if meta.path.is_ident("validate") {
        let value = meta.value()?;
        if value.peek(Lit) {
          let lit: Lit = value.parse()?;
          match lit {
            Lit::Str(value) => validate = Some(value.parse()?),
            _ => {
              return Err(syn::Error::new(
                lit.span(),
                "sdk validate literal must be a string",
              ));
            }
          }
        } else {
          validate = Some(value.parse()?);
        }
      } else if meta.path.is_ident("format") {
        let value = meta.value()?;
        let _: syn::LitStr = value.parse()?;
      } else {
        return Err(meta.error("unsupported SdkObject attribute"));
      }
      Ok(())
    })?;
  }
  Ok(validate)
}

fn expand_sdk_enum(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
  let name = &input.ident;
  let repr = sdk_repr(input)?;
  let read_method = format_ident!("read_{repr}");
  let write_method = format_ident!("write_{repr}");
  let size = primitive_size_by_name(&repr).ok_or_else(|| {
    syn::Error::new(
      input.span(),
      "SdkEnum repr must be one of u8/i8/u16/i16/u32/i32/u64/i64",
    )
  })?;

  let variants = match &input.data {
    Data::Enum(data) => &data.variants,
    _ => {
      return Err(syn::Error::new(
        input.span(),
        "SdkEnum can only be derived for enums",
      ));
    }
  };

  let repr_ident = format_ident!("{repr}");
  let mut from_arms = Vec::new();
  let mut raw_arms = Vec::new();

  for variant in variants {
    if !matches!(variant.fields, Fields::Unit) {
      return Err(syn::Error::new(
        variant.span(),
        "SdkEnum only supports fieldless variants for now",
      ));
    }
    let variant_ident = &variant.ident;
    let (_, expr) = variant.discriminant.as_ref().ok_or_else(|| {
      syn::Error::new(
        variant.span(),
        "SdkEnum variants must have explicit discriminants",
      )
    })?;
    validate_integer_discriminant(expr)?;

    from_arms
      .push(quote! { value if value == (#expr as #repr_ident) => Some(Self::#variant_ident) });
    raw_arms.push(quote! { Self::#variant_ident => #expr as #repr_ident });
  }

  Ok(quote! {
      impl ::emfsdk::common::SdkEnumValue for #name {
          type Repr = #repr_ident;

          fn from_raw(value: Self::Repr) -> Option<Self> {
              match value {
                  #(#from_arms,)*
                  _ => None,
              }
          }

          fn raw(self) -> Self::Repr {
              match self {
                  #(#raw_arms,)*
              }
          }
      }

      impl ::emfsdk::common::SdkRead for #name {
          fn read_from<R: ::std::io::Read + ::std::io::Seek>(
              reader: &mut ::emfsdk::common::Reader<R>,
          ) -> ::emfsdk::common::Result<Self> {
              let offset = reader.position()?;
              let value = reader.#read_method()?;
              <Self as ::emfsdk::common::SdkEnumValue>::from_raw(value).ok_or_else(|| {
                  ::emfsdk::common::Error::invalid(
                      offset,
                      format!("invalid {} enum value: {}", stringify!(#name), value),
                  )
              })
          }
      }

      impl ::emfsdk::common::SdkWrite for #name {
          fn write_to<W: ::std::io::Write>(
              &self,
              writer: &mut ::emfsdk::common::Writer<W>,
          ) -> ::emfsdk::common::Result<()> {
              writer.#write_method(<Self as ::emfsdk::common::SdkEnumValue>::raw(*self))
          }
      }

      impl ::emfsdk::common::SdkSize for #name {
          fn sdk_size(&self) -> u64 {
              #size
          }
      }
  })
}

fn sdk_repr(input: &DeriveInput) -> syn::Result<String> {
  for attr in &input.attrs {
    if !attr.path().is_ident("sdk") {
      continue;
    }
    let mut repr = None;
    attr.parse_nested_meta(|meta| {
      if meta.path.is_ident("repr") {
        let value = meta.value()?;
        if value.peek(Lit) {
          let lit: Lit = value.parse()?;
          match lit {
            Lit::Str(value) => repr = Some(value.value()),
            _ => {
              return Err(syn::Error::new(
                lit.span(),
                "sdk repr literal must be a string",
              ));
            }
          }
        } else {
          let path: syn::Path = value.parse()?;
          repr = path.get_ident().map(|ident| ident.to_string());
        }
      }
      Ok(())
    })?;
    if let Some(repr) = repr {
      return Ok(repr);
    }
  }

  Err(syn::Error::new(
    input.span(),
    "SdkEnum requires #[sdk(repr = \"u32\")]",
  ))
}

fn primitive_io(ty: &Type) -> Option<(proc_macro2::Ident, proc_macro2::Ident, u64)> {
  let Type::Path(path) = ty else {
    return None;
  };
  let ident = path.path.get_ident()?.to_string();
  let size = primitive_size_by_name(&ident)?;
  Some((
    format_ident!("read_{ident}"),
    format_ident!("write_{ident}"),
    size,
  ))
}

fn byte_array_len(ty: &Type) -> Option<&Expr> {
  let Type::Array(array) = ty else {
    return None;
  };
  let Type::Path(element) = array.elem.as_ref() else {
    return None;
  };
  if !element.path.is_ident("u8") {
    return None;
  }
  Some(&array.len)
}

fn primitive_size_by_name(name: &str) -> Option<u64> {
  match name {
    "u8" | "i8" => Some(1),
    "u16" | "i16" => Some(2),
    "u32" | "i32" | "f32" => Some(4),
    "u64" | "i64" | "f64" => Some(8),
    _ => None,
  }
}

fn validate_integer_discriminant(expr: &Expr) -> syn::Result<()> {
  match expr {
    Expr::Lit(ExprLit {
      lit: Lit::Int(_), ..
    }) => Ok(()),
    Expr::Unary(value)
      if matches!(
        value.expr.as_ref(),
        Expr::Lit(ExprLit {
          lit: Lit::Int(_),
          ..
        })
      ) =>
    {
      Ok(())
    }
    _ => Err(syn::Error::new(
      expr.span(),
      "SdkEnum discriminants must be integer literals",
    )),
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use syn::parse_quote;

  #[test]
  fn sdk_object_rejects_unknown_attributes() {
    let input: DeriveInput = parse_quote! {
        #[sdk(formt = "emf")]
        struct Value { field: u32 }
    };
    let error = expand_sdk_object(&input).unwrap_err();
    assert!(
      error
        .to_string()
        .contains("unsupported SdkObject attribute")
    );
  }

  #[test]
  fn sdk_object_recognizes_fixed_byte_arrays() {
    let ty: Type = parse_quote!([u8; 32]);
    assert!(byte_array_len(&ty).is_some());

    let other: Type = parse_quote!([u16; 32]);
    assert!(byte_array_len(&other).is_none());
  }
}
