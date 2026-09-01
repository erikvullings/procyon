//! Procedural derives for bounded, symmetric `olecfsdk` binary structures.

#![forbid(unsafe_code)]

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{
  Data, DeriveInput, Expr, ExprLit, Fields, GenericArgument, Ident, Lit, LitInt, Path,
  PathArguments, RangeLimits, Type, parse_macro_input, spanned::Spanned,
};

#[proc_macro_derive(SdkObject, attributes(sdk))]
pub fn sdk_object(input: TokenStream) -> TokenStream {
  let input = parse_macro_input!(input as DeriveInput);
  expand_sdk_object(&input)
    .unwrap_or_else(syn::Error::into_compile_error)
    .into()
}

#[proc_macro_derive(SdkEnum, attributes(sdk))]
pub fn sdk_enum(input: TokenStream) -> TokenStream {
  let input = parse_macro_input!(input as DeriveInput);
  expand_sdk_enum(&input)
    .unwrap_or_else(syn::Error::into_compile_error)
    .into()
}

#[proc_macro_derive(SdkBitfield, attributes(sdk))]
pub fn sdk_bitfield(input: TokenStream) -> TokenStream {
  let input = parse_macro_input!(input as DeriveInput);
  expand_sdk_bitfield(&input)
    .unwrap_or_else(syn::Error::into_compile_error)
    .into()
}

fn expand_sdk_bitfield(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
  let name = &input.ident;
  let (repr, validate) = sdk_bitfield_container_attrs(input)?;
  let repr_name = repr.to_string();
  let repr_bits = match repr_name.as_str() {
    "u8" => 8u8,
    "u16" => 16,
    "u32" => 32,
    "u64" => 64,
    _ => {
      return Err(syn::Error::new(
        repr.span(),
        "SdkBitfield repr must be u8, u16, u32, or u64",
      ));
    }
  };
  let read_method = format_ident!("read_{repr}");
  let write_method = format_ident!("write_{repr}");
  let fields = match &input.data {
    Data::Struct(data) => match &data.fields {
      Fields::Named(fields) => fields,
      _ => {
        return Err(syn::Error::new(
          data.fields.span(),
          "SdkBitfield requires a struct with named fields",
        ));
      }
    },
    _ => {
      return Err(syn::Error::new(
        input.span(),
        "SdkBitfield can only be derived for structs",
      ));
    }
  };

  let mut covered = 0u128;
  let mut read_fields = Vec::new();
  let mut write_fields = Vec::new();
  for field in &fields.named {
    let ident = field.ident.as_ref().expect("named field");
    let ty = &field.ty;
    let (start, end) = sdk_bit_range(field)?;
    if end >= repr_bits {
      return Err(syn::Error::new(
        field.span(),
        format!("bit {end} is outside {repr_name}"),
      ));
    }
    let width = end - start + 1;
    let value_mask = (1u128 << width) - 1;
    let physical_mask = value_mask << start;
    if covered & physical_mask != 0 {
      return Err(syn::Error::new(field.span(), "SdkBitfield fields overlap"));
    }
    covered |= physical_mask;

    if is_bool(ty) {
      if width != 1 {
        return Err(syn::Error::new(
          field.span(),
          "bool fields must occupy exactly one bit",
        ));
      }
      read_fields.push(quote! { #ident: (sdk_raw & ((1 as #repr) << #start)) != 0 });
      write_fields.push(quote! {
          if self.#ident {
              sdk_raw |= (1 as #repr) << #start;
          }
      });
    } else {
      let field_bits = unsigned_integer_bits(ty)?;
      if width > field_bits {
        return Err(syn::Error::new(
          field.span(),
          format!(
            "declared {width}-bit range does not fit {}-bit field type",
            field_bits
          ),
        ));
      }
      read_fields.push(quote! {
          #ident: ((sdk_raw >> #start) & (#value_mask as #repr)) as #ty
      });
      write_fields.push(quote! {
          let sdk_value = self.#ident as u128;
          if sdk_value > #value_mask {
              return Err(::olecfsdk::Error::invalid(
                  sdk_offset,
                  concat!("value exceeds declared bit width for ", stringify!(#ident)),
              ));
          }
          sdk_raw |= ((sdk_value as #repr) & (#value_mask as #repr)) << #start;
      });
    }
  }

  let repr_mask = (1u128 << repr_bits) - 1;
  let uncovered = repr_mask & !covered;
  let validate_read = validate
    .as_ref()
    .map(|path| quote! { #path(&value, sdk_offset)?; });
  let validate_write = validate
    .as_ref()
    .map(|path| quote! { #path(self, sdk_offset)?; });
  let size = u64::from(repr_bits / 8);

  Ok(quote! {
      impl ::olecfsdk::io::SdkRead for #name {
          fn read_from<R: ::std::io::Read + ::std::io::Seek>(
              reader: &mut ::olecfsdk::io::Reader<R>,
          ) -> ::olecfsdk::Result<Self> {
              let sdk_offset = reader.position()?;
              let sdk_raw = reader.#read_method()?;
              if sdk_raw & (#uncovered as #repr) != 0 {
                  return Err(::olecfsdk::Error::invalid(
                      sdk_offset,
                      concat!("reserved bits are nonzero for ", stringify!(#name)),
                  ));
              }
              let value = Self { #(#read_fields,)* };
              #validate_read
              Ok(value)
          }
      }

      impl ::olecfsdk::io::SdkWrite for #name {
          fn write_to<W: ::std::io::Write>(
              &self,
              writer: &mut ::olecfsdk::io::Writer<W>,
          ) -> ::olecfsdk::Result<()> {
              let sdk_offset = writer.position()?;
              #validate_write
              let mut sdk_raw: #repr = 0;
              #(#write_fields)*
              writer.#write_method(sdk_raw)
          }
      }

      impl ::olecfsdk::io::SdkSize for #name {
          fn sdk_size(&self) -> u64 { #size }
      }
  })
}

fn sdk_bitfield_container_attrs(input: &DeriveInput) -> syn::Result<(Ident, Option<Path>)> {
  let mut repr = None;
  let mut validate = None;
  for attr in &input.attrs {
    if !attr.path().is_ident("sdk") {
      continue;
    }
    attr.parse_nested_meta(|meta| {
      if meta.path.is_ident("repr") {
        if repr.is_some() {
          return Err(meta.error("duplicate repr attribute"));
        }
        repr = Some(parse_ident_value(meta.value()?, "repr")?);
        Ok(())
      } else if meta.path.is_ident("validate") {
        if validate.is_some() {
          return Err(meta.error("duplicate validate attribute"));
        }
        let value = meta.value()?;
        validate = Some(if value.peek(Lit) {
          match value.parse::<Lit>()? {
            Lit::Str(value) => value.parse()?,
            lit => return Err(syn::Error::new(lit.span(), "validate must be a path")),
          }
        } else {
          value.parse()?
        });
        Ok(())
      } else {
        Err(meta.error("unsupported SdkBitfield attribute"))
      }
    })?;
  }
  Ok((
    repr.ok_or_else(|| {
      syn::Error::new(input.span(), "SdkBitfield requires #[sdk(repr = \"u16\")]")
    })?,
    validate,
  ))
}

fn sdk_bit_range(field: &syn::Field) -> syn::Result<(u8, u8)> {
  let mut range = None;
  for attr in &field.attrs {
    if !attr.path().is_ident("sdk") {
      continue;
    }
    attr.parse_nested_meta(|meta| {
      if range.is_some() {
        return Err(meta.error("SdkBitfield field needs exactly one bit or bits attribute"));
      }
      if meta.path.is_ident("bit") {
        let bit = parse_u8_expr(meta.value()?.parse()?)?;
        range = Some((bit, bit));
        Ok(())
      } else if meta.path.is_ident("bits") {
        let value: syn::ExprRange = meta.value()?.parse()?;
        if !matches!(value.limits, RangeLimits::Closed(_)) {
          return Err(syn::Error::new(
            value.span(),
            "bits range must be inclusive",
          ));
        }
        let range_span = value.span();
        let start = value
          .start
          .ok_or_else(|| syn::Error::new(range_span, "bits range needs a start"))?;
        let end = value
          .end
          .ok_or_else(|| syn::Error::new(range_span, "bits range needs an end"))?;
        let start = parse_u8_expr(*start)?;
        let end = parse_u8_expr(*end)?;
        if start > end {
          return Err(syn::Error::new(range_span, "bits range is reversed"));
        }
        range = Some((start, end));
        Ok(())
      } else {
        Err(meta.error("unsupported SdkBitfield field attribute"))
      }
    })?;
  }
  range.ok_or_else(|| {
    syn::Error::new(
      field.span(),
      "SdkBitfield fields require #[sdk(bit = N)] or #[sdk(bits = A..=B)]",
    )
  })
}

fn parse_ident_value(input: syn::parse::ParseStream<'_>, label: &str) -> syn::Result<Ident> {
  if input.peek(Lit) {
    match input.parse::<Lit>()? {
      Lit::Str(value) => value.parse(),
      lit => Err(syn::Error::new(
        lit.span(),
        format!("{label} must name an unsigned integer primitive"),
      )),
    }
  } else {
    input.parse()
  }
}

fn parse_u8_expr(expr: Expr) -> syn::Result<u8> {
  let Expr::Lit(ExprLit {
    lit: Lit::Int(value),
    ..
  }) = expr
  else {
    return Err(syn::Error::new(
      expr.span(),
      "bit positions must be integer literals",
    ));
  };
  parse_u8_lit(&value)
}

fn parse_u8_lit(value: &LitInt) -> syn::Result<u8> {
  value.base10_parse::<u8>()
}

fn is_bool(ty: &Type) -> bool {
  matches!(ty, Type::Path(path) if path.path.is_ident("bool"))
}

fn unsigned_integer_bits(ty: &Type) -> syn::Result<u8> {
  let bits = match ty {
    Type::Path(path) => match path.path.get_ident().map(Ident::to_string).as_deref() {
      Some("u8") => Some(8),
      Some("u16") => Some(16),
      Some("u32") => Some(32),
      Some("u64") => Some(64),
      _ => None,
    },
    _ => None,
  };
  bits.ok_or_else(|| {
    syn::Error::new(
      ty.span(),
      "SdkBitfield fields must be bool or an unsigned integer primitive",
    )
  })
}

fn expand_sdk_object(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
  let name = &input.ident;
  let object_attrs = sdk_object_attrs(input)?;
  let validate = object_attrs.validation;
  let size_prefix = object_attrs.size_prefix;
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
  let mut field_idents = Vec::new();
  let mut write_fields = Vec::new();
  let mut size_fields = Vec::new();
  let field_count = fields.named.len();
  let optional_fields = fields
    .named
    .iter()
    .map(sdk_field_attrs)
    .collect::<syn::Result<Vec<_>>>()?;
  let optional_tail_start = optional_fields.iter().position(|attrs| attrs.optional);
  if let Some(start) = optional_tail_start
    && optional_fields[start..].iter().any(|attrs| !attrs.optional)
  {
    return Err(syn::Error::new(
      fields.named[start].span(),
      "optional fields must form the final contiguous physical suffix",
    ));
  }
  for (field_index, field) in fields.named.iter().enumerate() {
    let ident = field.ident.as_ref().expect("named field");
    let ty = &field.ty;
    let attrs = sdk_field_attrs(field)?;
    if attrs.remaining && field_index + 1 != field_count {
      return Err(syn::Error::new(
        field.span(),
        "remaining must be the final physical field",
      ));
    }
    if attrs.optional_remaining && field_index + 1 != field_count {
      return Err(syn::Error::new(
        field.span(),
        "optional_remaining must be the final physical field",
      ));
    }
    field_idents.push(ident);
    if attrs.remaining {
      let element = vec_element_type(ty).ok_or_else(|| {
        syn::Error::new(field.span(), "remaining is only supported on Vec fields")
      })?;
      if let Some(element_size) = attrs.remaining_element_size.as_ref() {
        if primitive_io(element).is_some() {
          return Err(syn::Error::new(
            element.span(),
            "remaining element_size is only needed for non-primitive fixed-layout elements",
          ));
        }
        read_fields.push(quote! {
                    let sdk_remaining_bytes = reader.remaining()?;
                    if sdk_remaining_bytes % #element_size != 0 {
                        return Err(::olecfsdk::Error::invalid(
                            reader.position()?,
                            concat!(
                                "remaining byte length is not divisible by the declared element size for ",
                                stringify!(#ident),
                            ),
                        ));
                    }
                    let sdk_remaining_count = ::core::convert::TryInto::<usize>::try_into(
                        sdk_remaining_bytes / #element_size
                    )
                        .map_err(|_| ::olecfsdk::Error::Limit(
                            concat!("remaining element count does not fit usize for ", stringify!(#ident)).into(),
                        ))?;
                    reader.ensure_allocation(sdk_remaining_count, #element_size as usize)?;
                    let mut #ident = ::std::vec::Vec::with_capacity(sdk_remaining_count);
                    for _ in 0..sdk_remaining_count {
                        let sdk_value = {
                            let sdk_element_offset = reader.position()?;
                            let mut sdk_element_reader = reader.sub_reader(#element_size)?;
                            let sdk_value = <#element as ::olecfsdk::io::SdkRead>::read_from(
                                &mut sdk_element_reader,
                            )?;
                            if sdk_element_reader.remaining()? != 0 {
                                return Err(::olecfsdk::Error::invalid(
                                    sdk_element_offset,
                                    concat!(
                                        "element did not consume the declared fixed layout for ",
                                        stringify!(#ident),
                                    ),
                                ));
                            }
                            sdk_value
                        };
                        #ident.push(sdk_value);
                    }
                });
        write_fields.push(quote! {
            for value in &self.#ident {
                let sdk_element_offset = writer.position()?;
                if ::olecfsdk::io::SdkSize::sdk_size(value) != #element_size {
                    return Err(::olecfsdk::Error::invalid(
                        sdk_element_offset,
                        concat!(
                            "element size does not match the declared fixed layout for ",
                            stringify!(#ident),
                        ),
                    ));
                }
                <#element as ::olecfsdk::io::SdkWrite>::write_to(value, writer)?;
                let sdk_written = writer.position()?.checked_sub(sdk_element_offset)
                    .ok_or_else(|| ::olecfsdk::Error::invalid(
                        sdk_element_offset,
                        concat!("writer moved backwards for ", stringify!(#ident)),
                    ))?;
                if sdk_written != #element_size {
                    return Err(::olecfsdk::Error::invalid(
                        sdk_element_offset,
                        concat!(
                            "element writer did not produce the declared fixed layout for ",
                            stringify!(#ident),
                        ),
                    ));
                }
            }
        });
        size_fields.push(quote! { (self.#ident.len() as u64) * #element_size });
        continue;
      }
      let (read_method, write_method, element_size) = primitive_io(element).ok_or_else(|| {
        syn::Error::new(
          element.span(),
          "remaining Vec elements must be fixed-size primitive values or declare element_size",
        )
      })?;
      let divisibility_check = (element_size > 1).then(|| {
                quote! {
                    if sdk_remaining_bytes % #element_size != 0 {
                        return Err(::olecfsdk::Error::invalid(
                            reader.position()?,
                            concat!("remaining byte length is not divisible by element size for ", stringify!(#ident)),
                        ));
                    }
                }
            });
      read_fields.push(quote! {
                let sdk_remaining_bytes = reader.remaining()?;
                #divisibility_check
                let sdk_remaining_count = ::core::convert::TryInto::<usize>::try_into(
                    sdk_remaining_bytes / #element_size
                )
                    .map_err(|_| ::olecfsdk::Error::Limit(
                        concat!("remaining element count does not fit usize for ", stringify!(#ident)).into(),
                    ))?;
                reader.ensure_allocation(sdk_remaining_count, #element_size as usize)?;
                let mut #ident = ::std::vec::Vec::with_capacity(sdk_remaining_count);
                for _ in 0..sdk_remaining_count {
                    #ident.push(reader.#read_method()?);
                }
            });
      write_fields.push(quote! {
          for value in &self.#ident {
              writer.#write_method(*value)?;
          }
      });
      size_fields.push(quote! { (self.#ident.len() as u64) * #element_size });
    } else if attrs.optional_remaining {
      let element = option_element_type(ty).ok_or_else(|| {
        syn::Error::new(
          field.span(),
          "optional_remaining is only supported on Option fields",
        )
      })?;
      let (read_value, write_value, size_value) = type_io_tokens(element);
      read_fields.push(quote! {
          let #ident = if reader.remaining()? == 0 {
              None
          } else {
              let sdk_optional_value = #read_value;
              if reader.remaining()? != 0 {
                  return Err(::olecfsdk::Error::invalid(
                      reader.position()?,
                      concat!(
                          "optional remaining field did not consume the bounded input for ",
                          stringify!(#ident),
                      ),
                  ));
              }
              Some(sdk_optional_value)
          };
      });
      write_fields.push(quote! {
          if let Some(value) = &self.#ident {
              #write_value
          }
      });
      size_fields.push(quote! {
          self.#ident.as_ref().map_or(0, |value| { #size_value })
      });
    } else if attrs.optional {
      let element = option_element_type(ty).ok_or_else(|| {
        syn::Error::new(field.span(), "optional is only supported on Option fields")
      })?;
      if primitive_io(element).is_none()
        && byte_array_len(element).is_none()
        && primitive_array_io(element).is_none()
      {
        return Err(syn::Error::new(
          element.span(),
          "optional tail fields must have a fixed primitive or primitive-array layout",
        ));
      }
      let (read_value, write_value, size_value) = type_io_tokens(element);
      let final_remaining_check = (field_index + 1 == field_count).then(|| {
        quote! {
            if reader.remaining()? != 0 {
                return Err(::olecfsdk::Error::invalid(
                    reader.position()?,
                    concat!("bytes remain after optional tail field ", stringify!(#ident)),
                ));
            }
        }
      });
      read_fields.push(quote! {
          let #ident = if reader.remaining()? == 0 {
              None
          } else {
              Some(#read_value)
          };
          #final_remaining_check
      });
      write_fields.push(quote! {
          if let Some(value) = &self.#ident {
              if sdk_optional_tail_ended {
                  return Err(::olecfsdk::Error::invalid(
                      writer.position()?,
                      concat!("optional tail has a gap before ", stringify!(#ident)),
                  ));
              }
              #write_value
          } else {
              sdk_optional_tail_ended = true;
          }
      });
      size_fields.push(quote! {
          self.#ident.as_ref().map_or(0, |value| { #size_value })
      });
    } else if let Some(alignment) = attrs.align.as_ref() {
      require_plain_byte_vec(&attrs, ty, "align")?;
      read_fields.push(quote! {
          let #ident = reader.read_alignment(#alignment)?;
      });
      write_fields.push(quote! {
          let sdk_padding = writer.alignment_padding(#alignment)?;
          if self.#ident.len() != sdk_padding {
              return Err(::olecfsdk::Error::invalid(
                  writer.position()?,
                  concat!("alignment padding mismatch for ", stringify!(#ident)),
              ));
          }
          ::std::io::Write::write_all(writer, &self.#ident)?;
      });
      size_fields.push(quote! { self.#ident.len() as u64 });
    } else if let Some(condition) = attrs.condition.as_ref() {
      let element = option_element_type(ty).ok_or_else(|| {
        syn::Error::new(field.span(), "condition is only supported on Option fields")
      })?;
      let read_condition = if let Some(mask) = attrs.mask.as_ref() {
        quote! { (#condition & (#mask)) != 0 }
      } else {
        quote! { #condition != 0 }
      };
      let write_condition = if let Some(mask) = attrs.mask.as_ref() {
        quote! { (self.#condition & (#mask)) != 0 }
      } else {
        quote! { self.#condition != 0 }
      };
      let (read_value, write_value, size_value) = type_io_tokens(element);
      read_fields.push(quote! {
          let #ident = if #read_condition { Some(#read_value) } else { None };
      });
      write_fields.push(quote! {
          match (#write_condition, &self.#ident) {
              (true, Some(value)) => { #write_value }
              (false, None) => {}
              _ => return Err(::olecfsdk::Error::invalid(
                  writer.position()?,
                  concat!("condition mismatch for ", stringify!(#ident)),
              )),
          }
      });
      size_fields.push(quote! {
          self.#ident.as_ref().map_or(0, |value| { #size_value })
      });
    } else if let Some(repr) = attrs.bitflags.as_ref() {
      let repr_ty = Type::Path(syn::TypePath {
        qself: None,
        path: repr.clone().into(),
      });
      let (read_method, write_method, size) = primitive_io(&repr_ty).ok_or_else(|| {
        syn::Error::new(repr.span(), "bitflags repr must be an integer primitive")
      })?;
      read_fields.push(quote! {
          let #ident = <#ty>::from_bits_retain(reader.#read_method()?);
      });
      write_fields.push(quote! {
          writer.#write_method(self.#ident.bits())?;
      });
      size_fields.push(quote! { #size });
    } else if let Some((read_method, write_method, size)) = primitive_io(ty) {
      reject_count(&attrs, ty)?;
      read_fields.push(quote! { let #ident = reader.#read_method()?; });
      write_fields.push(quote! { writer.#write_method(self.#ident)?; });
      size_fields.push(quote! { #size });
    } else if let Some(len) = byte_array_len(ty) {
      reject_count(&attrs, ty)?;
      read_fields.push(quote! { let #ident = reader.read_array::<#len>()?; });
      write_fields.push(quote! {
          ::std::io::Write::write_all(writer, &self.#ident)?;
      });
      size_fields.push(quote! { #len });
    } else if let Some((read_method, write_method, element_size, len)) = primitive_array_io(ty) {
      reject_count(&attrs, ty)?;
      read_fields.push(quote! {
          let #ident = {
              let mut values = [::core::default::Default::default(); #len];
              for value in &mut values {
                  *value = reader.#read_method()?;
              }
              values
          };
      });
      write_fields.push(quote! {
          for value in &self.#ident {
              writer.#write_method(*value)?;
          }
      });
      size_fields.push(quote! { (#len as u64) * #element_size });
    } else if let Some(element) = vec_element_type(ty) {
      let count = attrs.count;
      let count_prefix = attrs.count_prefix;
      let min_element_size = attrs.min_element_size;
      if count.is_none() && count_prefix.is_none() {
        return Err(syn::Error::new(
          field.span(),
          "Vec fields require #[sdk(count = \"field\")] or #[sdk(count_prefix = \"u16\")]",
        ));
      }
      let (read_value, read_allocation_size) =
        if let Some((read_method, _, size)) = primitive_io(element) {
          (quote! { reader.#read_method()? }, quote! { #size as usize })
        } else {
          (
            quote! { <#element as ::olecfsdk::io::SdkRead>::read_from(reader)? },
            quote! { ::core::mem::size_of::<#element>().max(1) },
          )
        };
      let (read_count, write_count, prefix_size) = if let Some(count) = count {
        (
          quote! { #count },
          quote! {
              let sdk_expected = ::core::convert::TryInto::<usize>::try_into(self.#count)
                  .map_err(|_| ::olecfsdk::Error::invalid(
                      sdk_count_offset,
                      concat!("invalid count for ", stringify!(#ident)),
                  ))?;
              if self.#ident.len() != sdk_expected {
                  return Err(::olecfsdk::Error::invalid(
                      sdk_count_offset,
                      concat!("count mismatch for ", stringify!(#ident)),
                  ));
              }
          },
          0u64,
        )
      } else {
        let repr = count_prefix.expect("count mode checked above");
        let repr_ty = Type::Path(syn::TypePath {
          qself: None,
          path: repr.clone().into(),
        });
        let (read_method, write_method, size) =
          integer_primitive_io(&repr_ty).ok_or_else(|| {
            syn::Error::new(repr.span(), "count_prefix must be an integer primitive")
          })?;
        (
          quote! { reader.#read_method()? },
          quote! {
              let sdk_encoded_count: #repr_ty =
                  ::core::convert::TryFrom::<usize>::try_from(self.#ident.len())
                      .map_err(|_| ::olecfsdk::Error::Limit(
                          concat!("count prefix overflow for ", stringify!(#ident)).into(),
                      ))?;
              writer.#write_method(sdk_encoded_count)?;
          },
          size,
        )
      };
      let minimum_count_check = min_element_size.as_ref().map(|minimum| {
        quote! {
            let sdk_count_u64 = ::core::convert::TryInto::<u64>::try_into(sdk_count)
                .map_err(|_| ::olecfsdk::Error::Limit(
                    concat!("element count does not fit u64 for ", stringify!(#ident)).into(),
                ))?;
            if sdk_count_u64 > reader.remaining()? / #minimum {
                return Err(::olecfsdk::Error::invalid(
                    sdk_count_offset,
                    concat!(
                        "element count exceeds the bounded input at the declared minimum size for ",
                        stringify!(#ident),
                    ),
                ));
            }
        }
      });
      read_fields.push(quote! {
          let sdk_count_offset = reader.position()?;
          let sdk_count = ::core::convert::TryInto::<usize>::try_into(#read_count)
              .map_err(|_| ::olecfsdk::Error::invalid(
                  sdk_count_offset,
                  concat!("invalid count for ", stringify!(#ident)),
              ))?;
          #minimum_count_check
          reader.ensure_allocation(sdk_count, #read_allocation_size)?;
          let mut #ident = ::std::vec::Vec::with_capacity(sdk_count);
          for _ in 0..sdk_count {
              #ident.push(#read_value);
          }
      });
      let count_check = quote! {
          let sdk_count_offset = writer.position()?;
          #write_count
      };
      if let Some((_, write_method, size)) = primitive_io(element) {
        write_fields.push(quote! {
            #count_check
            for value in &self.#ident {
                writer.#write_method(*value)?;
            }
        });
        size_fields.push(quote! { #prefix_size + (self.#ident.len() as u64) * #size });
      } else {
        write_fields.push(quote! {
            #count_check
            for value in &self.#ident {
                <#element as ::olecfsdk::io::SdkWrite>::write_to(value, writer)?;
            }
        });
        size_fields.push(quote! {
            #prefix_size
                + self.#ident.iter().map(::olecfsdk::io::SdkSize::sdk_size).sum::<u64>()
        });
      }
    } else {
      reject_count(&attrs, ty)?;
      read_fields.push(quote! {
          let #ident = <#ty as ::olecfsdk::io::SdkRead>::read_from(reader)?;
      });
      write_fields.push(quote! {
          <#ty as ::olecfsdk::io::SdkWrite>::write_to(&self.#ident, writer)?;
      });
      size_fields.push(quote! {
          <#ty as ::olecfsdk::io::SdkSize>::sdk_size(&self.#ident)
      });
    }
  }

  let validation_offset_read = validate
    .as_ref()
    .filter(|validation| validation.with_offset)
    .map(|_| quote! { let sdk_validation_offset = reader.position()?; });
  let validation_offset_write = validate
    .as_ref()
    .filter(|validation| validation.with_offset)
    .map(|_| quote! { let sdk_validation_offset = writer.position()?; });
  let validate_read = validate.as_ref().map(|validation| {
    let path = &validation.path;
    if validation.with_offset {
      quote! { #path(&value, sdk_validation_offset)?; }
    } else {
      quote! { #path(&value)?; }
    }
  });
  let validate_write = validate.as_ref().map(|validation| {
    let path = &validation.path;
    if validation.with_offset {
      quote! { #path(self, sdk_validation_offset)?; }
    } else {
      quote! { #path(self)?; }
    }
  });
  let optional_tail_write_state =
    optional_tail_start.map(|_| quote! { let mut sdk_optional_tail_ended = false; });

  let payload_size = quote! { 0u64 #(+ #size_fields as u64)* };
  let (read_prefix, read_remainder_check, write_prefix, write_size_check, total_size) =
    if let Some(repr) = size_prefix {
      let repr_ty = Type::Path(syn::TypePath {
        qself: None,
        path: repr.clone().into(),
      });
      let (read_method, write_method, prefix_size) = unsigned_integer_primitive_io(&repr_ty)
        .ok_or_else(|| syn::Error::new(repr.span(), "size_prefix must be u8, u16, u32, or u64"))?;
      (
        quote! {
            let sdk_payload_size = ::core::convert::Into::<u64>::into(
                reader.#read_method()?
            );
            let mut sdk_payload_reader = reader.sub_reader(sdk_payload_size)?;
            let reader = &mut sdk_payload_reader;
        },
        quote! {
            if reader.remaining()? != 0 {
                return Err(::olecfsdk::Error::invalid(
                    reader.position()?,
                    concat!("size-prefixed payload has trailing bytes for ", stringify!(#name)),
                ));
            }
        },
        quote! {
            let sdk_payload_size = #payload_size;
            let sdk_encoded_payload_size: #repr_ty =
                ::core::convert::TryFrom::<u64>::try_from(sdk_payload_size)
                    .map_err(|_| ::olecfsdk::Error::Limit(
                        concat!("size prefix overflow for ", stringify!(#name)).into(),
                    ))?;
            writer.#write_method(sdk_encoded_payload_size)?;
            let sdk_payload_offset = writer.position()?;
        },
        quote! {
            let sdk_written_payload = writer.position()?
                .checked_sub(sdk_payload_offset)
                .ok_or_else(|| ::olecfsdk::Error::invalid(
                    sdk_payload_offset,
                    concat!("writer moved backwards for ", stringify!(#name)),
                ))?;
            if sdk_written_payload != sdk_payload_size {
                return Err(::olecfsdk::Error::invalid(
                    sdk_payload_offset,
                    concat!("writer size mismatch for ", stringify!(#name)),
                ));
            }
        },
        quote! { #prefix_size + #payload_size },
      )
    } else {
      (
        quote! {},
        quote! {},
        quote! {},
        quote! {},
        payload_size.clone(),
      )
    };

  Ok(quote! {
      impl ::olecfsdk::io::SdkRead for #name {
          fn read_from<R: ::std::io::Read + ::std::io::Seek>(
              reader: &mut ::olecfsdk::io::Reader<R>,
          ) -> ::olecfsdk::Result<Self> {
              #validation_offset_read
              #read_prefix
              #(#read_fields)*
              let value = Self { #(#field_idents,)* };
              #read_remainder_check
              #validate_read
              Ok(value)
          }
      }

      impl ::olecfsdk::io::SdkWrite for #name {
          fn write_to<W: ::std::io::Write>(
              &self,
              writer: &mut ::olecfsdk::io::Writer<W>,
          ) -> ::olecfsdk::Result<()> {
              #validation_offset_write
              #validate_write
              #write_prefix
              #optional_tail_write_state
              #(#write_fields)*
              #write_size_check
              Ok(())
          }
      }

      impl ::olecfsdk::io::SdkSize for #name {
          fn sdk_size(&self) -> u64 {
              #total_size
          }
      }
  })
}

#[derive(Default)]
struct FieldAttrs {
  count: Option<Ident>,
  count_prefix: Option<Ident>,
  min_element_size: Option<LitInt>,
  condition: Option<Ident>,
  mask: Option<Expr>,
  align: Option<Expr>,
  remaining: bool,
  remaining_element_size: Option<LitInt>,
  optional_remaining: bool,
  optional: bool,
  bitflags: Option<Ident>,
}

fn sdk_field_attrs(field: &syn::Field) -> syn::Result<FieldAttrs> {
  let mut attrs = FieldAttrs::default();
  for attr in &field.attrs {
    if !attr.path().is_ident("sdk") {
      continue;
    }
    attr.parse_nested_meta(|meta| {
      if meta.path.is_ident("count") || meta.path.is_ident("condition") {
        let target = if meta.path.is_ident("count") {
          &mut attrs.count
        } else {
          &mut attrs.condition
        };
        if target.is_some() {
          return Err(meta.error("duplicate SdkObject field attribute"));
        }
        let value = meta.value()?;
        *target = Some(if value.peek(Lit) {
          match value.parse::<Lit>()? {
            Lit::Str(value) => value.parse()?,
            lit => {
              return Err(syn::Error::new(lit.span(), "attribute must name a field"));
            }
          }
        } else {
          value.parse()?
        });
        Ok(())
      } else if meta.path.is_ident("bitflags") || meta.path.is_ident("count_prefix") {
        let (target, label) = if meta.path.is_ident("bitflags") {
          (&mut attrs.bitflags, "bitflags")
        } else {
          (&mut attrs.count_prefix, "count_prefix")
        };
        if target.is_some() {
          return Err(meta.error(format!("duplicate {label} attribute")));
        }
        let value = meta.value()?;
        *target = Some(if value.peek(Lit) {
          match value.parse::<Lit>()? {
            Lit::Str(value) => value.parse()?,
            lit => {
              return Err(syn::Error::new(
                lit.span(),
                format!("{label} must name an integer primitive"),
              ));
            }
          }
        } else {
          value.parse()?
        });
        Ok(())
      } else if meta.path.is_ident("min_element_size") {
        if attrs.min_element_size.is_some() {
          return Err(meta.error("duplicate min_element_size attribute"));
        }
        let size: LitInt = meta.value()?.parse()?;
        if size.base10_parse::<u64>()? == 0 {
          return Err(syn::Error::new(
            size.span(),
            "min_element_size must be greater than zero",
          ));
        }
        attrs.min_element_size = Some(size);
        Ok(())
      } else if meta.path.is_ident("mask") || meta.path.is_ident("align") {
        let target = if meta.path.is_ident("mask") {
          &mut attrs.mask
        } else {
          &mut attrs.align
        };
        if target.is_some() {
          return Err(meta.error("duplicate SdkObject field attribute"));
        }
        *target = Some(meta.value()?.parse()?);
        Ok(())
      } else if meta.path.is_ident("remaining") {
        if attrs.remaining {
          return Err(meta.error("duplicate remaining attribute"));
        }
        attrs.remaining = true;
        if meta.input.peek(syn::token::Paren) {
          meta.parse_nested_meta(|nested| {
            if !nested.path.is_ident("element_size") {
              return Err(nested.error("remaining only supports element_size"));
            }
            if attrs.remaining_element_size.is_some() {
              return Err(nested.error("duplicate remaining element_size"));
            }
            let size: LitInt = nested.value()?.parse()?;
            if size.base10_parse::<u64>()? == 0 {
              return Err(syn::Error::new(
                size.span(),
                "remaining element_size must be greater than zero",
              ));
            }
            attrs.remaining_element_size = Some(size);
            Ok(())
          })?;
        }
        Ok(())
      } else if meta.path.is_ident("optional_remaining") {
        if attrs.optional_remaining {
          return Err(meta.error("duplicate optional_remaining attribute"));
        }
        attrs.optional_remaining = true;
        Ok(())
      } else if meta.path.is_ident("optional") {
        if attrs.optional {
          return Err(meta.error("duplicate optional attribute"));
        }
        attrs.optional = true;
        Ok(())
      } else {
        Err(meta.error("unsupported SdkObject field attribute"))
      }
    })?;
  }

  let modes = usize::from(attrs.count.is_some())
    + usize::from(attrs.count_prefix.is_some())
    + usize::from(attrs.condition.is_some())
    + usize::from(attrs.align.is_some())
    + usize::from(attrs.remaining)
    + usize::from(attrs.optional_remaining)
    + usize::from(attrs.optional)
    + usize::from(attrs.bitflags.is_some());
  if modes > 1 {
    return Err(syn::Error::new(
      field.span(),
      "count, count_prefix, condition, align, remaining, optional_remaining, optional, and bitflags are mutually exclusive",
    ));
  }
  if attrs.mask.is_some() && attrs.condition.is_none() {
    return Err(syn::Error::new(
      field.span(),
      "mask requires a condition attribute",
    ));
  }
  if attrs.min_element_size.is_some() && attrs.count.is_none() && attrs.count_prefix.is_none() {
    return Err(syn::Error::new(
      field.span(),
      "min_element_size requires count or count_prefix",
    ));
  }
  Ok(attrs)
}

fn require_plain_byte_vec(attrs: &FieldAttrs, ty: &Type, attribute: &str) -> syn::Result<()> {
  let is_byte_vec = vec_element_type(ty)
    .is_some_and(|element| matches!(element, Type::Path(path) if path.path.is_ident("u8")));
  if !is_byte_vec
    || attrs.count.is_some()
    || attrs.count_prefix.is_some()
    || attrs.condition.is_some()
  {
    return Err(syn::Error::new(
      ty.span(),
      format!("{attribute} is only supported on Vec<u8> fields"),
    ));
  }
  Ok(())
}

fn reject_count(attrs: &FieldAttrs, ty: &Type) -> syn::Result<()> {
  if attrs.count.is_some() || attrs.count_prefix.is_some() {
    Err(syn::Error::new(
      ty.span(),
      "count and count_prefix are only supported on Vec fields",
    ))
  } else {
    Ok(())
  }
}

fn vec_element_type(ty: &Type) -> Option<&Type> {
  let Type::Path(path) = ty else { return None };
  let segment = path.path.segments.last()?;
  if segment.ident != "Vec" {
    return None;
  }
  let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
    return None;
  };
  match arguments.args.first()? {
    GenericArgument::Type(element) => Some(element),
    _ => None,
  }
}

fn option_element_type(ty: &Type) -> Option<&Type> {
  let Type::Path(path) = ty else { return None };
  let segment = path.path.segments.last()?;
  if segment.ident != "Option" {
    return None;
  }
  let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
    return None;
  };
  match arguments.args.first()? {
    GenericArgument::Type(element) => Some(element),
    _ => None,
  }
}

fn type_io_tokens(
  ty: &Type,
) -> (
  proc_macro2::TokenStream,
  proc_macro2::TokenStream,
  proc_macro2::TokenStream,
) {
  if let Some((read_method, write_method, size)) = primitive_io(ty) {
    (
      quote! { reader.#read_method()? },
      quote! { writer.#write_method(*value)?; },
      quote! { #size },
    )
  } else if let Some(len) = byte_array_len(ty) {
    (
      quote! { reader.read_array::<#len>()? },
      quote! { ::std::io::Write::write_all(writer, value)?; },
      quote! { #len },
    )
  } else if let Some((read_method, write_method, element_size, len)) = primitive_array_io(ty) {
    (
      quote! {{
          let mut values = [::core::default::Default::default(); #len];
          for value in &mut values {
              *value = reader.#read_method()?;
          }
          values
      }},
      quote! {
          for item in value {
              writer.#write_method(*item)?;
          }
      },
      quote! { (#len as u64) * #element_size },
    )
  } else {
    (
      quote! { <#ty as ::olecfsdk::io::SdkRead>::read_from(reader)? },
      quote! { <#ty as ::olecfsdk::io::SdkWrite>::write_to(value, writer)?; },
      quote! { <#ty as ::olecfsdk::io::SdkSize>::sdk_size(value) },
    )
  }
}

struct ObjectValidation {
  path: Path,
  with_offset: bool,
}

#[derive(Default)]
struct ObjectAttrs {
  validation: Option<ObjectValidation>,
  size_prefix: Option<Ident>,
}

fn sdk_object_attrs(input: &DeriveInput) -> syn::Result<ObjectAttrs> {
  let mut attrs = ObjectAttrs::default();
  for attr in &input.attrs {
    if !attr.path().is_ident("sdk") {
      continue;
    }
    attr.parse_nested_meta(|meta| {
      if meta.path.is_ident("validate") || meta.path.is_ident("validate_at") {
        if attrs.validation.is_some() {
          return Err(meta.error("duplicate validation attribute"));
        }
        let with_offset = meta.path.is_ident("validate_at");
        let value = meta.value()?;
        let path = if value.peek(Lit) {
          match value.parse::<Lit>()? {
            Lit::Str(value) => value.parse()?,
            lit => return Err(syn::Error::new(lit.span(), "validate must be a path")),
          }
        } else {
          value.parse()?
        };
        attrs.validation = Some(ObjectValidation { path, with_offset });
        Ok(())
      } else if meta.path.is_ident("size_prefix") {
        if attrs.size_prefix.is_some() {
          return Err(meta.error("duplicate size_prefix attribute"));
        }
        let value = meta.value()?;
        attrs.size_prefix = Some(if value.peek(Lit) {
          match value.parse::<Lit>()? {
            Lit::Str(value) => value.parse()?,
            lit => {
              return Err(syn::Error::new(
                lit.span(),
                "size_prefix must be u8, u16, u32, or u64",
              ));
            }
          }
        } else {
          value.parse()?
        });
        Ok(())
      } else {
        Err(meta.error("unsupported SdkObject attribute"))
      }
    })?;
  }
  Ok(attrs)
}

fn expand_sdk_enum(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
  let name = &input.ident;
  let repr = sdk_repr(input)?;
  let read_method = format_ident!("read_{repr}");
  let write_method = format_ident!("write_{repr}");
  let size = primitive_size_by_name(&repr)
    .ok_or_else(|| syn::Error::new(input.span(), "SdkEnum repr must be an integer primitive"))?;
  let repr_ident = format_ident!("{repr}");
  let variants = match &input.data {
    Data::Enum(data) => &data.variants,
    _ => return Err(syn::Error::new(input.span(), "SdkEnum requires an enum")),
  };

  let mut from_arms = Vec::new();
  let mut raw_arms = Vec::new();
  for variant in variants {
    if !matches!(variant.fields, Fields::Unit) {
      return Err(syn::Error::new(
        variant.span(),
        "SdkEnum only supports fieldless variants",
      ));
    }
    let ident = &variant.ident;
    let (_, value) = variant.discriminant.as_ref().ok_or_else(|| {
      syn::Error::new(
        variant.span(),
        "SdkEnum variants need explicit discriminants",
      )
    })?;
    from_arms.push(quote! { value if value == (#value as #repr_ident) => Some(Self::#ident) });
    raw_arms.push(quote! { Self::#ident => #value as #repr_ident });
  }

  Ok(quote! {
      impl ::olecfsdk::io::SdkEnumValue for #name {
          type Repr = #repr_ident;
          fn from_raw(value: Self::Repr) -> Option<Self> {
              match value { #(#from_arms,)* _ => None }
          }
          fn raw(self) -> Self::Repr {
              match self { #(#raw_arms,)* }
          }
      }

      impl ::olecfsdk::io::SdkRead for #name {
          fn read_from<R: ::std::io::Read + ::std::io::Seek>(
              reader: &mut ::olecfsdk::io::Reader<R>,
          ) -> ::olecfsdk::Result<Self> {
              let offset = reader.position()?;
              let raw = reader.#read_method()?;
              <Self as ::olecfsdk::io::SdkEnumValue>::from_raw(raw).ok_or_else(|| {
                  ::olecfsdk::Error::invalid(offset, format!(
                      "invalid {} value: {}", stringify!(#name), raw
                  ))
              })
          }
      }

      impl ::olecfsdk::io::SdkWrite for #name {
          fn write_to<W: ::std::io::Write>(
              &self,
              writer: &mut ::olecfsdk::io::Writer<W>,
          ) -> ::olecfsdk::Result<()> {
              writer.#write_method(<Self as ::olecfsdk::io::SdkEnumValue>::raw(*self))
          }
      }

      impl ::olecfsdk::io::SdkSize for #name {
          fn sdk_size(&self) -> u64 { #size }
      }
  })
}

fn sdk_repr(input: &DeriveInput) -> syn::Result<String> {
  let mut repr = None;
  for attr in &input.attrs {
    if !attr.path().is_ident("sdk") {
      continue;
    }
    attr.parse_nested_meta(|meta| {
      if meta.path.is_ident("repr") {
        let value = meta.value()?;
        if value.peek(Lit) {
          match value.parse::<Lit>()? {
            Lit::Str(value) => repr = Some(value.value()),
            lit => {
              return Err(syn::Error::new(
                lit.span(),
                "repr must be an integer primitive",
              ));
            }
          }
        } else {
          let path: Path = value.parse()?;
          repr = path.get_ident().map(ToString::to_string);
        }
        Ok(())
      } else {
        Err(meta.error("unsupported SdkEnum attribute"))
      }
    })?;
  }
  repr.ok_or_else(|| syn::Error::new(input.span(), "SdkEnum requires #[sdk(repr = \"u16\")]"))
}

fn primitive_io(ty: &Type) -> Option<(proc_macro2::Ident, proc_macro2::Ident, u64)> {
  let Type::Path(path) = ty else { return None };
  let ident = path.path.get_ident()?.to_string();
  let size = primitive_size_by_name(&ident)?;
  Some((
    format_ident!("read_{ident}"),
    format_ident!("write_{ident}"),
    size,
  ))
}

fn integer_primitive_io(ty: &Type) -> Option<(proc_macro2::Ident, proc_macro2::Ident, u64)> {
  let Type::Path(path) = ty else { return None };
  let ident = path.path.get_ident()?.to_string();
  matches!(
    ident.as_str(),
    "u8" | "i8" | "u16" | "i16" | "u32" | "i32" | "u64" | "i64"
  )
  .then(|| {
    let size = primitive_size_by_name(&ident).expect("integer primitive has a size");
    (
      format_ident!("read_{ident}"),
      format_ident!("write_{ident}"),
      size,
    )
  })
}

fn unsigned_integer_primitive_io(
  ty: &Type,
) -> Option<(proc_macro2::Ident, proc_macro2::Ident, u64)> {
  let Type::Path(path) = ty else { return None };
  let ident = path.path.get_ident()?.to_string();
  matches!(ident.as_str(), "u8" | "u16" | "u32" | "u64").then(|| {
    let size = primitive_size_by_name(&ident).expect("unsigned integer primitive has a size");
    (
      format_ident!("read_{ident}"),
      format_ident!("write_{ident}"),
      size,
    )
  })
}

fn byte_array_len(ty: &Type) -> Option<&Expr> {
  let Type::Array(array) = ty else { return None };
  let Type::Path(element) = array.elem.as_ref() else {
    return None;
  };
  (element.path.is_ident("u8")).then_some(&array.len)
}

fn primitive_array_io(ty: &Type) -> Option<(proc_macro2::Ident, proc_macro2::Ident, u64, &Expr)> {
  let Type::Array(array) = ty else { return None };
  let Type::Path(element) = array.elem.as_ref() else {
    return None;
  };
  let ident = element.path.get_ident()?.to_string();
  let size = primitive_size_by_name(&ident)?;
  Some((
    format_ident!("read_{ident}"),
    format_ident!("write_{ident}"),
    size,
    &array.len,
  ))
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

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn bitfield_rejects_a_rust_field_narrower_than_its_wire_range() {
    let input: DeriveInput = syn::parse_quote! {
        #[sdk(repr = "u16")]
        struct TooNarrow {
            #[sdk(bits = 0..=11)]
            value: u8,
        }
    };
    let error = expand_sdk_bitfield(&input).unwrap_err();
    assert!(error.to_string().contains("does not fit 8-bit field type"));
  }

  #[test]
  fn optional_remaining_requires_an_option_in_final_position() {
    let non_option: DeriveInput = syn::parse_quote! {
        struct NonOptionTail {
            #[sdk(optional_remaining)]
            value: u32,
        }
    };
    assert!(
      expand_sdk_object(&non_option)
        .unwrap_err()
        .to_string()
        .contains("only supported on Option fields")
    );

    let non_final: DeriveInput = syn::parse_quote! {
        struct NonFinalTail {
            #[sdk(optional_remaining)]
            value: Option<u32>,
            following: u16,
        }
    };
    assert!(
      expand_sdk_object(&non_final)
        .unwrap_err()
        .to_string()
        .contains("must be the final physical field")
    );
  }

  #[test]
  fn remaining_fixed_layout_requires_a_positive_size_on_a_final_object_vec() {
    let zero_size: DeriveInput = syn::parse_quote! {
        struct ZeroSize {
            #[sdk(remaining(element_size = 0))]
            values: Vec<Item>,
        }
    };
    assert!(
      expand_sdk_object(&zero_size)
        .unwrap_err()
        .to_string()
        .contains("must be greater than zero")
    );

    let primitive: DeriveInput = syn::parse_quote! {
        struct PrimitiveSize {
            #[sdk(remaining(element_size = 2))]
            values: Vec<u16>,
        }
    };
    assert!(
      expand_sdk_object(&primitive)
        .unwrap_err()
        .to_string()
        .contains("only needed for non-primitive")
    );

    let non_final: DeriveInput = syn::parse_quote! {
        struct NonFinalObjects {
            #[sdk(remaining(element_size = 4))]
            values: Vec<Item>,
            following: u16,
        }
    };
    assert!(
      expand_sdk_object(&non_final)
        .unwrap_err()
        .to_string()
        .contains("must be the final physical field")
    );
  }

  #[test]
  fn optional_fields_must_be_a_fixed_layout_final_suffix() {
    let non_final: DeriveInput = syn::parse_quote! {
        struct NonFinalOptional {
            #[sdk(optional)]
            value: Option<u16>,
            following: u16,
        }
    };
    assert!(
      expand_sdk_object(&non_final)
        .unwrap_err()
        .to_string()
        .contains("final contiguous physical suffix")
    );

    let variable: DeriveInput = syn::parse_quote! {
        struct VariableOptional {
            #[sdk(optional)]
            value: Option<Vec<u8>>,
        }
    };
    assert!(
      expand_sdk_object(&variable)
        .unwrap_err()
        .to_string()
        .contains("fixed primitive or primitive-array layout")
    );
  }

  #[test]
  fn object_size_prefix_requires_an_unsigned_integer_primitive() {
    for input in [
      syn::parse_quote! {
          #[sdk(size_prefix = "i16")]
          struct SignedSize { value: u16 }
      },
      syn::parse_quote! {
          #[sdk(size_prefix = "usize")]
          struct PlatformSize { value: u16 }
      },
    ] {
      assert!(
        expand_sdk_object(&input)
          .unwrap_err()
          .to_string()
          .contains("size_prefix must be u8, u16, u32, or u64")
      );
    }

    let duplicate: DeriveInput = syn::parse_quote! {
        #[sdk(size_prefix = "u16", size_prefix = "u32")]
        struct DuplicateSize { value: u16 }
    };
    assert!(
      expand_sdk_object(&duplicate)
        .unwrap_err()
        .to_string()
        .contains("duplicate size_prefix")
    );
  }

  #[test]
  fn vector_minimum_element_size_requires_a_positive_count_mode() {
    let zero: DeriveInput = syn::parse_quote! {
        struct ZeroMinimum {
            #[sdk(count_prefix = "u16", min_element_size = 0)]
            values: Vec<Item>,
        }
    };
    assert!(
      expand_sdk_object(&zero)
        .unwrap_err()
        .to_string()
        .contains("must be greater than zero")
    );

    let no_count: DeriveInput = syn::parse_quote! {
        struct MissingCount {
            #[sdk(min_element_size = 4)]
            values: Vec<Item>,
        }
    };
    assert!(
      expand_sdk_object(&no_count)
        .unwrap_err()
        .to_string()
        .contains("requires count or count_prefix")
    );
  }
}
