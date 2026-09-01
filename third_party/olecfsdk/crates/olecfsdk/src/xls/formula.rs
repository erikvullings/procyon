use crate::{Error, Result};

use super::XlStringCharacters;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormulaTokenStream {
  pub tokens: Vec<FormulaToken>,
  /// Bounded remainder beginning with an unsupported opcode.
  pub unparsed_tail: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormulaToken {
  /// Exact Ptg opcode, including the operand-class bits.
  pub opcode: u8,
  pub data: FormulaTokenData,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FormulaTokenData {
  UnknownZero,
  Exp {
    row: u16,
    column: u16,
  },
  Table {
    row: u16,
    column: u16,
  },
  Operator(FormulaOperator),
  String {
    flags: u8,
    characters: XlStringCharacters,
  },
  PivotName {
    /// Exact PtgSxName extended discriminator; MS-XLS requires 0x1D.
    extended_opcode: u8,
    name_index: u32,
  },
  /// One of the natural-language formula tokens encoded as PtgElf*.
  NaturalLanguage {
    /// Exact extended Ptg discriminator following the 0x18 opcode.
    extended_opcode: u8,
    value: FormulaNaturalLanguageToken,
  },
  Attribute {
    options: u8,
    data: u16,
    choose_jump_offsets: Vec<u16>,
    choose_function_offset: Option<u16>,
  },
  Error(u8),
  Boolean(u8),
  Integer(u16),
  NumberBits(u64),
  Array {
    reserved0: u32,
    reserved1: u16,
    reserved2: u8,
    values: Option<FormulaArray>,
  },
  Function {
    function_index: u16,
  },
  FunctionVar {
    argument_count: u8,
    function_index: u16,
  },
  Name {
    name_index: u32,
  },
  Reference {
    row: u16,
    column: u16,
  },
  Area {
    first_row: u16,
    last_row: u16,
    first_column: u16,
    last_column: u16,
  },
  MemArea {
    reserved: u32,
    byte_count: u16,
    extra: Option<FormulaMemExtra>,
  },
  MemError {
    reserved: u32,
    byte_count: u16,
  },
  MemNoMem {
    unused: u32,
    byte_count: u16,
  },
  MemFunction {
    byte_count: u16,
  },
  ReferenceError {
    reserved: u32,
  },
  AreaError {
    reserved0: u32,
    reserved1: u32,
  },
  RelativeReference {
    row: u16,
    column: u16,
  },
  RelativeArea {
    first_row: u16,
    last_row: u16,
    first_column: u16,
    last_column: u16,
  },
  ExternalName {
    external_sheet_index: u16,
    name_index: u32,
  },
  Reference3d {
    external_sheet_index: u16,
    row: u16,
    column: u16,
  },
  Area3d {
    external_sheet_index: u16,
    first_row: u16,
    last_row: u16,
    first_column: u16,
    last_column: u16,
  },
  DeletedReference3d {
    external_sheet_index: u16,
    reserved: u32,
  },
  DeletedArea3d {
    external_sheet_index: u16,
    reserved0: u32,
    reserved1: u32,
  },
}

/// Typed payload shared by the MS-XLS PtgElf* natural-language formula tokens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FormulaNaturalLanguageToken {
  /// PtgElfLel or PtgElfRadicalLel.
  DeletedLabel {
    label_index: u16,
    /// Bit 0 is fQuoted; the remaining bits are reserved and retained.
    flags: u16,
  },
  /// A single-cell label location.
  Location(FormulaElfLocation),
  /// A multiple-cell label whose locations are stored in RgbExtra.
  MultipleCell {
    /// Undefined wire value retained exactly as required by MS-XLS.
    unused: u32,
    extra: Option<FormulaElfExtra>,
  },
}

/// RgceElfLoc: the row and packed ColElfU value of a natural-language label.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FormulaElfLocation {
  pub row: u16,
  /// Low 14 bits are the column, bit 14 is fQuoted, and bit 15 is fRelative.
  pub column: u16,
}

/// PtgExtraElf data associated with a multiple-cell PtgElf* token.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormulaElfExtra {
  /// Reserved bit 30 from the count word, retained for compatible round-trip.
  pub reserved: bool,
  /// fRel (bit 31) from the count word.
  pub relative: bool,
  pub locations: Vec<FormulaElfExtraLocation>,
}

/// RgceElfLocExtra: a row and packed ColRelU value in PtgExtraElf.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FormulaElfExtraLocation {
  pub row: u16,
  /// Low 14 bits are the column; the high two relative bits are retained.
  pub column: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormulaOperator {
  Add,
  Subtract,
  Multiply,
  Divide,
  Power,
  Concat,
  LessThan,
  LessEqual,
  Equal,
  GreaterEqual,
  GreaterThan,
  NotEqual,
  Intersection,
  Union,
  Range,
  UnaryPlus,
  UnaryMinus,
  Percent,
  Parenthesis,
  MissingArgument,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormulaArray {
  pub columns: u16,
  pub rows: u16,
  pub values: Vec<BiffConstant>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BiffConstant {
  Empty {
    reserved: u64,
  },
  NumberBits(u64),
  String {
    flags: u8,
    characters: XlStringCharacters,
  },
  Boolean {
    value: u8,
    reserved: [u8; 7],
  },
  Error {
    code: u16,
    reserved0: u16,
    reserved1: u32,
  },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormulaMemExtra {
  pub ranges: Vec<FormulaRange>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FormulaRange {
  pub first_row: u16,
  pub last_row: u16,
  pub first_column: u16,
  pub last_column: u16,
}

impl FormulaTokenStream {
  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    let mut cursor = 0usize;
    let mut tokens = Vec::new();
    while cursor < bytes.len() {
      let start = cursor;
      let opcode = take_u8(bytes, &mut cursor)?;
      let Some(data) = FormulaTokenData::read(opcode, bytes, &mut cursor)? else {
        return Ok(Self {
          tokens,
          unparsed_tail: bytes[start..].to_vec(),
        });
      };
      tokens.push(FormulaToken { opcode, data });
    }
    Ok(Self {
      tokens,
      unparsed_tail: Vec::new(),
    })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    for token in &self.tokens {
      token.write(&mut bytes)?;
    }
    bytes.extend_from_slice(&self.unparsed_tail);
    Ok(bytes)
  }

  pub fn encoded_len(&self) -> Result<usize> {
    Ok(self.to_bytes()?.len())
  }

  pub fn missing_extra_count(&self) -> usize {
    self
      .tokens
      .iter()
      .filter(|token| match &token.data {
        FormulaTokenData::Array { values, .. } => values.is_none(),
        FormulaTokenData::MemArea { extra, .. } => extra.is_none(),
        FormulaTokenData::NaturalLanguage {
          value: FormulaNaturalLanguageToken::MultipleCell { extra, .. },
          ..
        } => extra.is_none(),
        _ => false,
      })
      .count()
  }

  /// Counts parsed tokens whose reserved bits or bounded values violate
  /// the MS-XLS Ptg grammar while retaining their exact wire values.
  pub fn nonconforming_token_count(&self) -> usize {
    self
      .tokens
      .iter()
      .filter(|token| {
        token.opcode & 0x80 != 0
          || matches!(token.data, FormulaTokenData::UnknownZero)
          || match &token.data {
            FormulaTokenData::NaturalLanguage { value, .. } => match value {
              FormulaNaturalLanguageToken::DeletedLabel { flags, .. } => flags & !1 != 0,
              FormulaNaturalLanguageToken::Location(location) => location.column & 0x3fff > 0x00ff,
              FormulaNaturalLanguageToken::MultipleCell { extra, .. } => {
                extra.as_ref().is_some_and(|extra| {
                  extra.reserved
                    || extra
                      .locations
                      .iter()
                      .any(|location| location.column & 0x3fff > 0x00ff)
                })
              }
            },
            _ => false,
          }
      })
      .count()
  }

  pub fn parse_extra_data(&mut self, bytes: &[u8]) -> Result<Vec<u8>> {
    let mut cursor = 0usize;
    for token in &mut self.tokens {
      let start = cursor;
      let result = match &mut token.data {
        FormulaTokenData::Array { values, .. } => {
          FormulaArray::read(bytes, &mut cursor).map(|value| *values = Some(value))
        }
        FormulaTokenData::MemArea { extra, .. } => {
          FormulaMemExtra::read(bytes, &mut cursor).map(|value| *extra = Some(value))
        }
        FormulaTokenData::NaturalLanguage {
          value: FormulaNaturalLanguageToken::MultipleCell { extra, .. },
          ..
        } => FormulaElfExtra::read(bytes, &mut cursor).map(|value| *extra = Some(value)),
        _ => continue,
      };
      match result {
        Ok(()) => {}
        Err(Error::InvalidData { .. }) => return Ok(bytes[start..].to_vec()),
        Err(error) => return Err(error),
      }
    }
    Ok(bytes[cursor..].to_vec())
  }

  pub fn extra_data_to_bytes(&self) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    for token in &self.tokens {
      match &token.data {
        FormulaTokenData::Array { values, .. } => {
          let Some(values) = values else {
            break;
          };
          values.write(&mut bytes)?;
        }
        FormulaTokenData::MemArea { extra, .. } => {
          let Some(extra) = extra else {
            break;
          };
          extra.write(&mut bytes)?;
        }
        FormulaTokenData::NaturalLanguage {
          value: FormulaNaturalLanguageToken::MultipleCell { extra, .. },
          ..
        } => {
          let Some(extra) = extra else {
            break;
          };
          extra.write(&mut bytes)?;
        }
        _ => {}
      }
    }
    Ok(bytes)
  }
}

impl FormulaTokenData {
  fn read(opcode: u8, bytes: &[u8], cursor: &mut usize) -> Result<Option<Self>> {
    if opcode < 0x20 {
      return Ok(Some(match opcode {
        0x00 => Self::UnknownZero,
        0x01 => Self::Exp {
          row: take_u16(bytes, cursor)?,
          column: take_u16(bytes, cursor)?,
        },
        0x02 => Self::Table {
          row: take_u16(bytes, cursor)?,
          column: take_u16(bytes, cursor)?,
        },
        0x03..=0x16 => Self::Operator(FormulaOperator::from_opcode(opcode)),
        0x17 => {
          let count = usize::from(take_u8(bytes, cursor)?);
          let flags = take_u8(bytes, cursor)?;
          let characters = if flags & 1 == 0 {
            XlStringCharacters::Compressed(take(bytes, cursor, count)?.to_vec())
          } else {
            let byte_count = count
              .checked_mul(2)
              .ok_or_else(|| Error::Limit("formula string byte count overflow".into()))?;
            XlStringCharacters::Unicode(
              take(bytes, cursor, byte_count)?
                .chunks_exact(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .collect(),
            )
          };
          Self::String { flags, characters }
        }
        0x18 => {
          let extended_opcode = take_u8(bytes, cursor)?;
          match extended_opcode {
            0x01 | 0x10 => Self::NaturalLanguage {
              extended_opcode,
              value: FormulaNaturalLanguageToken::DeletedLabel {
                label_index: take_u16(bytes, cursor)?,
                flags: take_u16(bytes, cursor)?,
              },
            },
            0x02 | 0x03 | 0x06 | 0x07 | 0x0a => Self::NaturalLanguage {
              extended_opcode,
              value: FormulaNaturalLanguageToken::Location(FormulaElfLocation {
                row: take_u16(bytes, cursor)?,
                column: take_u16(bytes, cursor)?,
              }),
            },
            0x0b | 0x0d | 0x0f => Self::NaturalLanguage {
              extended_opcode,
              value: FormulaNaturalLanguageToken::MultipleCell {
                unused: take_u32(bytes, cursor)?,
                extra: None,
              },
            },
            0x1d => Self::PivotName {
              extended_opcode,
              name_index: take_u32(bytes, cursor)?,
            },
            _ => return Ok(None),
          }
        }
        0x19 => {
          let options = take_u8(bytes, cursor)?;
          let data = take_u16(bytes, cursor)?;
          let (choose_jump_offsets, choose_function_offset) = if options & 0x04 != 0 {
            let mut offsets = Vec::with_capacity(usize::from(data));
            for _ in 0..data {
              offsets.push(take_u16(bytes, cursor)?);
            }
            (offsets, Some(take_u16(bytes, cursor)?))
          } else {
            (Vec::new(), None)
          };
          Self::Attribute {
            options,
            data,
            choose_jump_offsets,
            choose_function_offset,
          }
        }
        0x1c => Self::Error(take_u8(bytes, cursor)?),
        0x1d => Self::Boolean(take_u8(bytes, cursor)?),
        0x1e => Self::Integer(take_u16(bytes, cursor)?),
        0x1f => Self::NumberBits(take_u64(bytes, cursor)?),
        _ => return Ok(None),
      }));
    }

    let base = opcode & 0x1f | 0x20;
    Ok(Some(match base {
      0x20 => Self::Array {
        reserved0: take_u32(bytes, cursor)?,
        reserved1: take_u16(bytes, cursor)?,
        reserved2: take_u8(bytes, cursor)?,
        values: None,
      },
      0x21 => Self::Function {
        function_index: take_u16(bytes, cursor)?,
      },
      0x22 => Self::FunctionVar {
        argument_count: take_u8(bytes, cursor)?,
        function_index: take_u16(bytes, cursor)?,
      },
      0x23 => Self::Name {
        name_index: take_u32(bytes, cursor)?,
      },
      0x24 => Self::Reference {
        row: take_u16(bytes, cursor)?,
        column: take_u16(bytes, cursor)?,
      },
      0x25 => Self::Area {
        first_row: take_u16(bytes, cursor)?,
        last_row: take_u16(bytes, cursor)?,
        first_column: take_u16(bytes, cursor)?,
        last_column: take_u16(bytes, cursor)?,
      },
      0x26 => Self::MemArea {
        reserved: take_u32(bytes, cursor)?,
        byte_count: take_u16(bytes, cursor)?,
        extra: None,
      },
      0x27 => Self::MemError {
        reserved: take_u32(bytes, cursor)?,
        byte_count: take_u16(bytes, cursor)?,
      },
      0x28 => Self::MemNoMem {
        unused: take_u32(bytes, cursor)?,
        byte_count: take_u16(bytes, cursor)?,
      },
      0x29 => Self::MemFunction {
        byte_count: take_u16(bytes, cursor)?,
      },
      0x2a => Self::ReferenceError {
        reserved: take_u32(bytes, cursor)?,
      },
      0x2b => Self::AreaError {
        reserved0: take_u32(bytes, cursor)?,
        reserved1: take_u32(bytes, cursor)?,
      },
      0x2c => Self::RelativeReference {
        row: take_u16(bytes, cursor)?,
        column: take_u16(bytes, cursor)?,
      },
      0x2d => Self::RelativeArea {
        first_row: take_u16(bytes, cursor)?,
        last_row: take_u16(bytes, cursor)?,
        first_column: take_u16(bytes, cursor)?,
        last_column: take_u16(bytes, cursor)?,
      },
      0x39 => Self::ExternalName {
        external_sheet_index: take_u16(bytes, cursor)?,
        name_index: take_u32(bytes, cursor)?,
      },
      0x3a => Self::Reference3d {
        external_sheet_index: take_u16(bytes, cursor)?,
        row: take_u16(bytes, cursor)?,
        column: take_u16(bytes, cursor)?,
      },
      0x3b => Self::Area3d {
        external_sheet_index: take_u16(bytes, cursor)?,
        first_row: take_u16(bytes, cursor)?,
        last_row: take_u16(bytes, cursor)?,
        first_column: take_u16(bytes, cursor)?,
        last_column: take_u16(bytes, cursor)?,
      },
      0x3c => Self::DeletedReference3d {
        external_sheet_index: take_u16(bytes, cursor)?,
        reserved: take_u32(bytes, cursor)?,
      },
      0x3d => Self::DeletedArea3d {
        external_sheet_index: take_u16(bytes, cursor)?,
        reserved0: take_u32(bytes, cursor)?,
        reserved1: take_u32(bytes, cursor)?,
      },
      _ => return Ok(None),
    }))
  }
}

impl FormulaToken {
  fn write(&self, bytes: &mut Vec<u8>) -> Result<()> {
    bytes.push(self.opcode);
    match &self.data {
      FormulaTokenData::UnknownZero | FormulaTokenData::Operator(_) => {}
      FormulaTokenData::Exp { row, column }
      | FormulaTokenData::Table { row, column }
      | FormulaTokenData::Reference { row, column }
      | FormulaTokenData::RelativeReference { row, column } => {
        put_u16(bytes, *row);
        put_u16(bytes, *column);
      }
      FormulaTokenData::String { flags, characters } => {
        let count = match characters {
          XlStringCharacters::Compressed(values) => {
            if flags & 1 != 0 {
              return Err(Error::invalid(
                0,
                "compressed Formula string has UTF-16 flag",
              ));
            }
            values.len()
          }
          XlStringCharacters::Unicode(values) => {
            if flags & 1 == 0 {
              return Err(Error::invalid(
                0,
                "Unicode Formula string lacks UTF-16 flag",
              ));
            }
            values.len()
          }
        };
        bytes.push(
          u8::try_from(count)
            .map_err(|_| Error::Limit("Formula string character count exceeds u8".into()))?,
        );
        bytes.push(*flags);
        match characters {
          XlStringCharacters::Compressed(values) => bytes.extend_from_slice(values),
          XlStringCharacters::Unicode(values) => {
            for value in values {
              put_u16(bytes, *value);
            }
          }
        }
      }
      FormulaTokenData::PivotName {
        extended_opcode,
        name_index,
      } => {
        if *extended_opcode != 0x1d {
          return Err(Error::invalid(0, "PtgSxName extended opcode must be 0x1D"));
        }
        bytes.push(*extended_opcode);
        put_u32(bytes, *name_index);
      }
      FormulaTokenData::NaturalLanguage {
        extended_opcode,
        value,
      } => {
        let valid = matches!(
          (extended_opcode, value),
          (
            0x01 | 0x10,
            FormulaNaturalLanguageToken::DeletedLabel { .. }
          ) | (
            0x02 | 0x03 | 0x06 | 0x07 | 0x0a,
            FormulaNaturalLanguageToken::Location(_)
          ) | (
            0x0b | 0x0d | 0x0f,
            FormulaNaturalLanguageToken::MultipleCell { .. }
          )
        );
        if !valid {
          return Err(Error::invalid(
            0,
            "PtgElf extended opcode does not match its typed payload",
          ));
        }
        bytes.push(*extended_opcode);
        match value {
          FormulaNaturalLanguageToken::DeletedLabel { label_index, flags } => {
            put_u16(bytes, *label_index);
            put_u16(bytes, *flags);
          }
          FormulaNaturalLanguageToken::Location(value) => {
            put_u16(bytes, value.row);
            put_u16(bytes, value.column);
          }
          FormulaNaturalLanguageToken::MultipleCell { unused, .. } => {
            put_u32(bytes, *unused);
          }
        }
      }
      FormulaTokenData::Attribute {
        options,
        data,
        choose_jump_offsets,
        choose_function_offset,
      } => {
        bytes.push(*options);
        put_u16(bytes, *data);
        if *options & 0x04 != 0 {
          if usize::from(*data) != choose_jump_offsets.len() {
            return Err(Error::invalid(0, "Formula choose jump count mismatch"));
          }
          for value in choose_jump_offsets {
            put_u16(bytes, *value);
          }
          put_u16(
            bytes,
            choose_function_offset
              .ok_or_else(|| Error::invalid(0, "Formula choose function offset is missing"))?,
          );
        }
      }
      FormulaTokenData::Error(value) | FormulaTokenData::Boolean(value) => bytes.push(*value),
      FormulaTokenData::Integer(value)
      | FormulaTokenData::Function {
        function_index: value,
      }
      | FormulaTokenData::MemFunction { byte_count: value } => put_u16(bytes, *value),
      FormulaTokenData::NumberBits(value) => put_u64(bytes, *value),
      FormulaTokenData::Array {
        reserved0,
        reserved1,
        reserved2,
        ..
      } => {
        put_u32(bytes, *reserved0);
        put_u16(bytes, *reserved1);
        bytes.push(*reserved2);
      }
      FormulaTokenData::FunctionVar {
        argument_count,
        function_index,
      } => {
        bytes.push(*argument_count);
        put_u16(bytes, *function_index);
      }
      FormulaTokenData::Name { name_index } => put_u32(bytes, *name_index),
      FormulaTokenData::Area {
        first_row,
        last_row,
        first_column,
        last_column,
      }
      | FormulaTokenData::RelativeArea {
        first_row,
        last_row,
        first_column,
        last_column,
      } => {
        put_u16(bytes, *first_row);
        put_u16(bytes, *last_row);
        put_u16(bytes, *first_column);
        put_u16(bytes, *last_column);
      }
      FormulaTokenData::MemArea {
        reserved,
        byte_count,
        ..
      }
      | FormulaTokenData::MemError {
        reserved,
        byte_count,
        ..
      } => {
        put_u32(bytes, *reserved);
        put_u16(bytes, *byte_count);
      }
      FormulaTokenData::MemNoMem { unused, byte_count } => {
        put_u32(bytes, *unused);
        put_u16(bytes, *byte_count);
      }
      FormulaTokenData::ReferenceError { reserved } => put_u32(bytes, *reserved),
      FormulaTokenData::AreaError {
        reserved0,
        reserved1,
      } => {
        put_u32(bytes, *reserved0);
        put_u32(bytes, *reserved1);
      }
      FormulaTokenData::ExternalName {
        external_sheet_index,
        name_index,
      } => {
        put_u16(bytes, *external_sheet_index);
        put_u32(bytes, *name_index);
      }
      FormulaTokenData::Reference3d {
        external_sheet_index,
        row,
        column,
      } => {
        put_u16(bytes, *external_sheet_index);
        put_u16(bytes, *row);
        put_u16(bytes, *column);
      }
      FormulaTokenData::Area3d {
        external_sheet_index,
        first_row,
        last_row,
        first_column,
        last_column,
      } => {
        put_u16(bytes, *external_sheet_index);
        put_u16(bytes, *first_row);
        put_u16(bytes, *last_row);
        put_u16(bytes, *first_column);
        put_u16(bytes, *last_column);
      }
      FormulaTokenData::DeletedReference3d {
        external_sheet_index,
        reserved,
      } => {
        put_u16(bytes, *external_sheet_index);
        put_u32(bytes, *reserved);
      }
      FormulaTokenData::DeletedArea3d {
        external_sheet_index,
        reserved0,
        reserved1,
      } => {
        put_u16(bytes, *external_sheet_index);
        put_u32(bytes, *reserved0);
        put_u32(bytes, *reserved1);
      }
    }
    Ok(())
  }
}

impl FormulaOperator {
  fn from_opcode(opcode: u8) -> Self {
    match opcode {
      0x03 => Self::Add,
      0x04 => Self::Subtract,
      0x05 => Self::Multiply,
      0x06 => Self::Divide,
      0x07 => Self::Power,
      0x08 => Self::Concat,
      0x09 => Self::LessThan,
      0x0a => Self::LessEqual,
      0x0b => Self::Equal,
      0x0c => Self::GreaterEqual,
      0x0d => Self::GreaterThan,
      0x0e => Self::NotEqual,
      0x0f => Self::Intersection,
      0x10 => Self::Union,
      0x11 => Self::Range,
      0x12 => Self::UnaryPlus,
      0x13 => Self::UnaryMinus,
      0x14 => Self::Percent,
      0x15 => Self::Parenthesis,
      0x16 => Self::MissingArgument,
      _ => unreachable!("operator opcode range checked by caller"),
    }
  }
}

impl BiffConstant {
  pub(super) fn read(bytes: &[u8], cursor: &mut usize) -> Result<Self> {
    Ok(match take_u8(bytes, cursor)? {
      0x00 => Self::Empty {
        reserved: take_u64(bytes, cursor)?,
      },
      0x01 => Self::NumberBits(take_u64(bytes, cursor)?),
      0x02 => {
        let count = usize::from(take_u16(bytes, cursor)?);
        let flags = take_u8(bytes, cursor)?;
        let characters = if flags & 1 == 0 {
          XlStringCharacters::Compressed(take(bytes, cursor, count)?.to_vec())
        } else {
          let byte_count = count
            .checked_mul(2)
            .ok_or_else(|| Error::Limit("Formula array string byte count overflow".into()))?;
          XlStringCharacters::Unicode(
            take(bytes, cursor, byte_count)?
              .chunks_exact(2)
              .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
              .collect(),
          )
        };
        Self::String { flags, characters }
      }
      0x04 => {
        let raw = take(bytes, cursor, 8)?;
        Self::Boolean {
          value: raw[0],
          reserved: raw[1..].try_into().expect("seven bytes"),
        }
      }
      0x10 => Self::Error {
        code: take_u16(bytes, cursor)?,
        reserved0: take_u16(bytes, cursor)?,
        reserved1: take_u32(bytes, cursor)?,
      },
      kind => {
        return Err(Error::invalid(
          (*cursor - 1) as u64,
          format!("unknown Formula array constant kind 0x{kind:02x}"),
        ));
      }
    })
  }

  pub(super) fn write(&self, bytes: &mut Vec<u8>) -> Result<()> {
    match self {
      Self::Empty { reserved } => {
        bytes.push(0x00);
        put_u64(bytes, *reserved);
      }
      Self::NumberBits(bits) => {
        bytes.push(0x01);
        put_u64(bytes, *bits);
      }
      Self::String { flags, characters } => {
        bytes.push(0x02);
        let count = match characters {
          XlStringCharacters::Compressed(values) => {
            if flags & 1 != 0 {
              return Err(Error::invalid(
                0,
                "compressed Formula array string has UTF-16 flag",
              ));
            }
            values.len()
          }
          XlStringCharacters::Unicode(values) => {
            if flags & 1 == 0 {
              return Err(Error::invalid(
                0,
                "Unicode Formula array string lacks UTF-16 flag",
              ));
            }
            values.len()
          }
        };
        put_u16(
          bytes,
          u16::try_from(count)
            .map_err(|_| Error::Limit("Formula array string character count exceeds u16".into()))?,
        );
        bytes.push(*flags);
        match characters {
          XlStringCharacters::Compressed(values) => bytes.extend_from_slice(values),
          XlStringCharacters::Unicode(values) => {
            for value in values {
              put_u16(bytes, *value);
            }
          }
        }
      }
      Self::Boolean { value, reserved } => {
        bytes.push(0x04);
        bytes.push(*value);
        bytes.extend_from_slice(reserved);
      }
      Self::Error {
        code,
        reserved0,
        reserved1,
      } => {
        bytes.push(0x10);
        put_u16(bytes, *code);
        put_u16(bytes, *reserved0);
        put_u32(bytes, *reserved1);
      }
    }
    Ok(())
  }
}

impl FormulaArray {
  fn read(bytes: &[u8], cursor: &mut usize) -> Result<Self> {
    let columns = u16::from(take_u8(bytes, cursor)?) + 1;
    let rows = take_u16(bytes, cursor)?
      .checked_add(1)
      .ok_or_else(|| Error::invalid(*cursor as u64, "Formula array row count overflow"))?;
    let count = usize::from(columns)
      .checked_mul(usize::from(rows))
      .ok_or_else(|| Error::Limit("Formula array value count overflow".into()))?;
    if count > bytes.len().saturating_sub(*cursor) / 4 {
      return Err(Error::invalid(
        *cursor as u64,
        "Formula array dimensions exceed the bounded value data",
      ));
    }
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
      values.push(BiffConstant::read(bytes, cursor)?);
    }
    Ok(Self {
      columns,
      rows,
      values,
    })
  }

  fn write(&self, bytes: &mut Vec<u8>) -> Result<()> {
    let expected = usize::from(self.columns)
      .checked_mul(usize::from(self.rows))
      .ok_or_else(|| Error::Limit("Formula array value count overflow".into()))?;
    if self.columns == 0 || self.columns > 256 || self.rows == 0 {
      return Err(Error::invalid(0, "Formula array dimensions are invalid"));
    }
    if self.values.len() != expected {
      return Err(Error::invalid(0, "Formula array value count mismatch"));
    }
    bytes.push((self.columns - 1) as u8);
    put_u16(bytes, self.rows - 1);
    for value in &self.values {
      value.write(bytes)?;
    }
    Ok(())
  }
}

impl FormulaMemExtra {
  fn read(bytes: &[u8], cursor: &mut usize) -> Result<Self> {
    let count = usize::from(take_u16(bytes, cursor)?);
    if count > bytes.len().saturating_sub(*cursor) / 8 {
      return Err(Error::invalid(
        *cursor as u64,
        "Formula memory range count exceeds bounded extra data",
      ));
    }
    let mut ranges = Vec::with_capacity(count);
    for _ in 0..count {
      ranges.push(FormulaRange {
        first_row: take_u16(bytes, cursor)?,
        last_row: take_u16(bytes, cursor)?,
        first_column: take_u16(bytes, cursor)?,
        last_column: take_u16(bytes, cursor)?,
      });
    }
    Ok(Self { ranges })
  }

  fn write(&self, bytes: &mut Vec<u8>) -> Result<()> {
    put_u16(
      bytes,
      u16::try_from(self.ranges.len())
        .map_err(|_| Error::Limit("Formula memory range count exceeds u16".into()))?,
    );
    for range in &self.ranges {
      put_u16(bytes, range.first_row);
      put_u16(bytes, range.last_row);
      put_u16(bytes, range.first_column);
      put_u16(bytes, range.last_column);
    }
    Ok(())
  }
}

impl FormulaElfExtra {
  fn read(bytes: &[u8], cursor: &mut usize) -> Result<Self> {
    let header = take_u32(bytes, cursor)?;
    let count = usize::try_from(header & 0x3fff_ffff)
      .map_err(|_| Error::Limit("PtgExtraElf location count exceeds usize".into()))?;
    if count == 0 {
      return Err(Error::invalid(
        (*cursor - 4) as u64,
        "PtgExtraElf location count is zero",
      ));
    }
    if count > bytes.len().saturating_sub(*cursor) / 4 {
      return Err(Error::invalid(
        *cursor as u64,
        "PtgExtraElf location count exceeds bounded extra data",
      ));
    }
    let mut locations = Vec::with_capacity(count);
    for _ in 0..count {
      locations.push(FormulaElfExtraLocation {
        row: take_u16(bytes, cursor)?,
        column: take_u16(bytes, cursor)?,
      });
    }
    Ok(Self {
      reserved: header & 0x4000_0000 != 0,
      relative: header & 0x8000_0000 != 0,
      locations,
    })
  }

  fn write(&self, bytes: &mut Vec<u8>) -> Result<()> {
    if self.locations.is_empty() || self.locations.len() > 0x3fff_ffff {
      return Err(Error::invalid(
        0,
        "PtgExtraElf location count is outside 1..=0x3FFFFFFF",
      ));
    }
    let mut header = u32::try_from(self.locations.len())
      .map_err(|_| Error::Limit("PtgExtraElf location count exceeds u32".into()))?;
    header |= u32::from(self.reserved) << 30;
    header |= u32::from(self.relative) << 31;
    put_u32(bytes, header);
    for location in &self.locations {
      put_u16(bytes, location.row);
      put_u16(bytes, location.column);
    }
    Ok(())
  }
}

fn take<'a>(bytes: &'a [u8], cursor: &mut usize, len: usize) -> Result<&'a [u8]> {
  let end = cursor
    .checked_add(len)
    .ok_or_else(|| Error::Limit("formula token offset overflow".into()))?;
  let value = bytes
    .get(*cursor..end)
    .ok_or_else(|| Error::invalid(*cursor as u64, "truncated Formula token"))?;
  *cursor = end;
  Ok(value)
}

fn take_u8(bytes: &[u8], cursor: &mut usize) -> Result<u8> {
  Ok(take(bytes, cursor, 1)?[0])
}

fn take_u16(bytes: &[u8], cursor: &mut usize) -> Result<u16> {
  Ok(u16::from_le_bytes(
    take(bytes, cursor, 2)?.try_into().expect("two bytes"),
  ))
}

fn take_u32(bytes: &[u8], cursor: &mut usize) -> Result<u32> {
  Ok(u32::from_le_bytes(
    take(bytes, cursor, 4)?.try_into().expect("four bytes"),
  ))
}

fn take_u64(bytes: &[u8], cursor: &mut usize) -> Result<u64> {
  Ok(u64::from_le_bytes(
    take(bytes, cursor, 8)?.try_into().expect("eight bytes"),
  ))
}

fn put_u16(bytes: &mut Vec<u8>, value: u16) {
  bytes.extend_from_slice(&value.to_le_bytes());
}

fn put_u32(bytes: &mut Vec<u8>, value: u32) {
  bytes.extend_from_slice(&value.to_le_bytes());
}

fn put_u64(bytes: &mut Vec<u8>, value: u64) {
  bytes.extend_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn token_and_extra_data_round_trip_statically() {
    let rgce = [
      0x60, 0, 0, 0, 0, 0, 0, 0, // Array
      0x26, 0, 0, 0, 0, 0, 0, // MemArea
      0x1e, 42, 0,    // integer constant
      0x03, // add
    ];
    let rgcb = [
      0, 0, 0, // one-column, one-row array
      1, 0, 0, 0, 0, 0, 0, 0, 0, // numeric array value
      1, 0, 1, 0, 2, 0, 3, 0, 4, 0, // one MemArea range
    ];
    let mut parsed = FormulaTokenStream::from_bytes(&rgce).unwrap();
    assert!(parsed.unparsed_tail.is_empty());
    assert!(parsed.parse_extra_data(&rgcb).unwrap().is_empty());
    assert_eq!(parsed.missing_extra_count(), 0);
    assert_eq!(parsed.to_bytes().unwrap(), rgce);
    assert_eq!(parsed.extra_data_to_bytes().unwrap(), rgcb);
  }

  #[test]
  fn unsupported_token_is_a_bounded_tail() {
    let parsed = FormulaTokenStream::from_bytes(&[0x1e, 7, 0, 0x18, 4, 2]).unwrap();
    assert_eq!(parsed.tokens.len(), 1);
    assert_eq!(parsed.unparsed_tail, [0x18, 4, 2]);
    assert_eq!(parsed.to_bytes().unwrap(), [0x1e, 7, 0, 0x18, 4, 2]);
  }

  #[test]
  fn natural_language_and_mem_no_mem_tokens_are_fully_typed() {
    let rgce = [
      0x18, 0x03, 5, 0, 7, 0, // PtgElfCol with RgceElfLoc
      0x18, 0x0d, 0xaa, 0xbb, 0xcc, 0xdd, // PtgElfColS
      0x68, 1, 2, 3, 4, 9, 0, // value-class PtgMemNoMem
    ];
    let rgcb = [
      2, 0, 0, 0x80, // two locations with fRel
      1, 0, 2, 0, // first RgceElfLocExtra
      3, 0, 4, 0, // second RgceElfLocExtra
    ];
    let mut parsed = FormulaTokenStream::from_bytes(&rgce).unwrap();
    assert!(parsed.unparsed_tail.is_empty());
    assert_eq!(parsed.missing_extra_count(), 1);
    assert!(parsed.parse_extra_data(&rgcb).unwrap().is_empty());
    assert_eq!(parsed.missing_extra_count(), 0);
    assert_eq!(parsed.to_bytes().unwrap(), rgce);
    assert_eq!(parsed.extra_data_to_bytes().unwrap(), rgcb);
    assert!(matches!(
      parsed.tokens[0].data,
      FormulaTokenData::NaturalLanguage {
        extended_opcode: 0x03,
        value: FormulaNaturalLanguageToken::Location(FormulaElfLocation { row: 5, column: 7 }),
      }
    ));
    assert!(matches!(
      parsed.tokens[2].data,
      FormulaTokenData::MemNoMem {
        unused: 0x0403_0201,
        byte_count: 9,
      }
    ));
  }

  #[test]
  fn pivot_name_token_is_static_and_exact() {
    let bytes = [0x18, 0x1d, 0x78, 0x56, 0x34, 0x12];
    let parsed = FormulaTokenStream::from_bytes(&bytes).unwrap();
    assert_eq!(
      parsed.tokens,
      [FormulaToken {
        opcode: 0x18,
        data: FormulaTokenData::PivotName {
          extended_opcode: 0x1d,
          name_index: 0x1234_5678,
        },
      }]
    );
    assert!(parsed.unparsed_tail.is_empty());
    assert_eq!(parsed.to_bytes().unwrap(), bytes);
  }
}
