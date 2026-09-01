use crate::{ConversionCode, SourceLocation};

/// Error returned while mapping a legacy Office file into an OOXML package.
#[derive(Debug, thiserror::Error)]
pub enum Error {
  #[error(transparent)]
  Source(#[from] olecfsdk::Error),
  #[error(transparent)]
  Target(#[from] ooxmlsdk::common::SdkError),
  #[error("conversion would lose {code:?} at {location:?}")]
  Unsupported {
    code: ConversionCode,
    location: SourceLocation,
  },
}

pub type Result<T> = std::result::Result<T, Error>;
