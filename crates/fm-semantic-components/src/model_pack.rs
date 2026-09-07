//! Deterministic single-file model package format for catalog model artifacts.
//!
//! The managed component lifecycle installs exactly one payload file per
//! catalog artifact, while a real embedding model is a small set of files
//! (graph, tokenizer, and configuration). A model pack concatenates those files
//! behind a bounded, self-describing index so the whole model stays one
//! checksummed, catalog-signed artifact and the worker can load it offline
//! without unpacking it to a second copy on disk.

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::Sha256Digest;

/// Leading bytes of every model pack.
pub const MODEL_PACK_MAGIC: &[u8; 20] = b"PROCYON-MODEL-PACK-1";

/// Largest accepted pack index, keeping header parsing bounded.
const MAX_INDEX_BYTES: u32 = 1024 * 1024;
/// Largest single member file the reader will materialize in memory.
const MAX_MEMBER_BYTES: u64 = 2 * 1024 * 1024 * 1024;

const COPY_BUFFER_BYTES: usize = 1024 * 1024;

/// Inference contract a worker must apply to a packed model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModelPackKind {
    /// Deterministic Unicode token-hashing fixture with no learned parameters.
    DeterministicTokenHashing,
    /// Transformer graph evaluated with ONNX Runtime and mean-pooled.
    OnnxTransformerMeanPool,
}

/// One member file inside a model pack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelPackFile {
    /// Member name, unique within the pack.
    pub name: String,
    /// Offset from the first payload byte.
    pub offset: u64,
    /// Exact member length in bytes.
    pub length: u64,
    /// Digest of the member's exact bytes.
    pub checksum: Sha256Digest,
}

/// Self-describing header of a model pack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelPackIndex {
    /// Pack layout version.
    pub format_version: u32,
    /// How the worker must evaluate the packed model.
    pub kind: ModelPackKind,
    /// Catalog-owned model identifier.
    pub model_id: String,
    /// Immutable upstream model revision.
    pub model_revision: String,
    /// Tokenizer identity including its immutable revision.
    pub tokenizer: String,
    /// Number of values in every produced vector.
    pub dimensions: u32,
    /// Maximum accepted tokens for one input.
    pub max_input_tokens: u32,
    /// Prefix the model expects on search queries, possibly empty.
    pub query_prefix: String,
    /// Prefix the model expects on indexed passages, possibly empty.
    pub passage_prefix: String,
    /// Whether the packed model is an evaluated production model.
    pub production: bool,
    /// Human-readable immutable provenance of the packed bytes.
    pub source: String,
    /// Member files in deterministic order.
    pub files: Vec<ModelPackFile>,
}

impl ModelPackIndex {
    /// Returns the member with the given name.
    #[must_use]
    pub fn file(&self, name: &str) -> Option<&ModelPackFile> {
        self.files.iter().find(|file| file.name == name)
    }

    fn validate(&self) -> Result<(), ModelPackError> {
        if self.format_version != 1 {
            return Err(ModelPackError::UnsupportedFormatVersion {
                version: self.format_version,
            });
        }
        if self.model_id.is_empty() || self.model_revision.is_empty() || self.tokenizer.is_empty() {
            return Err(ModelPackError::InvalidIndex("model identity is incomplete"));
        }
        if self.dimensions == 0 || self.max_input_tokens == 0 {
            return Err(ModelPackError::InvalidIndex(
                "model limits must be positive",
            ));
        }
        let mut expected_offset = 0_u64;
        for (position, file) in self.files.iter().enumerate() {
            if file.name.is_empty()
                || file.name.contains('/')
                || file.name.contains('\\')
                || file.name.contains("..")
            {
                return Err(ModelPackError::InvalidIndex("member name is unsafe"));
            }
            if self.files[..position]
                .iter()
                .any(|other| other.name == file.name)
            {
                return Err(ModelPackError::InvalidIndex("member names must be unique"));
            }
            if file.offset != expected_offset {
                return Err(ModelPackError::InvalidIndex("members must be contiguous"));
            }
            if file.length > MAX_MEMBER_BYTES {
                return Err(ModelPackError::InvalidIndex(
                    "member exceeds the size bound",
                ));
            }
            expected_offset = expected_offset
                .checked_add(file.length)
                .ok_or(ModelPackError::InvalidIndex("member offsets overflow"))?;
        }
        Ok(())
    }
}

/// Immutable model description used when writing a pack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelPackSpec {
    /// How the worker must evaluate the packed model.
    pub kind: ModelPackKind,
    /// Catalog-owned model identifier.
    pub model_id: String,
    /// Immutable upstream model revision.
    pub model_revision: String,
    /// Tokenizer identity including its immutable revision.
    pub tokenizer: String,
    /// Number of values in every produced vector.
    pub dimensions: u32,
    /// Maximum accepted tokens for one input.
    pub max_input_tokens: u32,
    /// Prefix the model expects on search queries, possibly empty.
    pub query_prefix: String,
    /// Prefix the model expects on indexed passages, possibly empty.
    pub passage_prefix: String,
    /// Whether the packed model is an evaluated production model.
    pub production: bool,
    /// Human-readable immutable provenance of the packed bytes.
    pub source: String,
    /// Member files as `(name, source path)`, written in the given order.
    pub files: Vec<(String, PathBuf)>,
}

/// Writes a deterministic model pack, returning the index it recorded.
///
/// The output depends only on the specification and the exact member bytes, so
/// repeated builds from the same inputs produce byte-identical packs.
///
/// # Errors
///
/// Returns a typed error when a member is unreadable, the index is invalid, or
/// the destination cannot be written.
pub fn write_model_pack(
    destination: &Path,
    spec: &ModelPackSpec,
) -> Result<ModelPackIndex, ModelPackError> {
    let mut files = Vec::with_capacity(spec.files.len());
    let mut offset = 0_u64;
    for (name, source) in &spec.files {
        let (length, checksum) = measure(source)?;
        files.push(ModelPackFile {
            name: name.clone(),
            offset,
            length,
            checksum,
        });
        offset = offset
            .checked_add(length)
            .ok_or(ModelPackError::InvalidIndex("member offsets overflow"))?;
    }
    let index = ModelPackIndex {
        format_version: 1,
        kind: spec.kind,
        model_id: spec.model_id.clone(),
        model_revision: spec.model_revision.clone(),
        tokenizer: spec.tokenizer.clone(),
        dimensions: spec.dimensions,
        max_input_tokens: spec.max_input_tokens,
        query_prefix: spec.query_prefix.clone(),
        passage_prefix: spec.passage_prefix.clone(),
        production: spec.production,
        source: spec.source.clone(),
        files,
    };
    index.validate()?;

    let encoded = serde_json_canonicalizer::to_vec(&index)
        .map_err(|error| ModelPackError::Encode(error.to_string()))?;
    let encoded_length = u32::try_from(encoded.len()).map_err(|_| ModelPackError::IndexTooLarge)?;
    if encoded_length > MAX_INDEX_BYTES {
        return Err(ModelPackError::IndexTooLarge);
    }

    let mut writer = BufWriter::new(File::create(destination)?);
    writer.write_all(MODEL_PACK_MAGIC)?;
    writer.write_all(&encoded_length.to_le_bytes())?;
    writer.write_all(&encoded)?;
    for (_, source) in &spec.files {
        let mut reader = BufReader::new(File::open(source)?);
        std::io::copy(&mut reader, &mut writer)?;
    }
    writer.flush()?;
    writer
        .into_inner()
        .map_err(std::io::IntoInnerError::into_error)?
        .sync_all()?;
    Ok(index)
}

fn measure(source: &Path) -> Result<(u64, Sha256Digest), ModelPackError> {
    let mut reader = BufReader::new(File::open(source)?);
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; COPY_BUFFER_BYTES];
    let mut length = 0_u64;
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        length += read as u64;
    }
    Ok((length, Sha256Digest::from_bytes(hasher.finalize().into())))
}

/// A packed model opened for offline reading.
#[derive(Debug)]
pub struct ModelPack {
    path: PathBuf,
    index: ModelPackIndex,
    payload_offset: u64,
}

impl ModelPack {
    /// Opens and validates a model pack header without reading member bytes.
    ///
    /// # Errors
    ///
    /// Returns a typed error for a missing file, a foreign or truncated
    /// header, or an index that fails structural validation.
    pub fn open(path: &Path) -> Result<Self, ModelPackError> {
        let mut file = File::open(path)?;
        let mut magic = [0_u8; MODEL_PACK_MAGIC.len()];
        file.read_exact(&mut magic)
            .map_err(|_| ModelPackError::NotAModelPack)?;
        if &magic != MODEL_PACK_MAGIC {
            return Err(ModelPackError::NotAModelPack);
        }
        let mut length = [0_u8; 4];
        file.read_exact(&mut length)
            .map_err(|_| ModelPackError::NotAModelPack)?;
        let length = u32::from_le_bytes(length);
        if length == 0 || length > MAX_INDEX_BYTES {
            return Err(ModelPackError::IndexTooLarge);
        }
        let mut encoded = vec![0_u8; length as usize];
        file.read_exact(&mut encoded)
            .map_err(|_| ModelPackError::NotAModelPack)?;
        let index: ModelPackIndex = serde_json::from_slice(&encoded)
            .map_err(|error| ModelPackError::Decode(error.to_string()))?;
        index.validate()?;

        let payload_offset = (MODEL_PACK_MAGIC.len() as u64) + 4 + u64::from(length);
        let declared = index.files.iter().try_fold(0_u64, |total, file| {
            total
                .checked_add(file.length)
                .ok_or(ModelPackError::InvalidIndex("member offsets overflow"))
        })?;
        let actual = file.metadata()?.len();
        if actual != payload_offset + declared {
            return Err(ModelPackError::Truncated);
        }
        Ok(Self {
            path: path.to_owned(),
            index,
            payload_offset,
        })
    }

    /// Returns the validated pack header.
    #[must_use]
    pub const fn index(&self) -> &ModelPackIndex {
        &self.index
    }

    /// Returns the pack's own path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Reads and verifies one member's exact bytes.
    ///
    /// # Errors
    ///
    /// Returns a typed error when the member is absent, unreadable, or its
    /// bytes do not match the digest recorded in the index.
    pub fn read(&self, name: &str) -> Result<Vec<u8>, ModelPackError> {
        let member = self
            .index
            .file(name)
            .ok_or_else(|| ModelPackError::MissingMember(name.to_owned()))?;
        let length = usize::try_from(member.length).map_err(|_| ModelPackError::Truncated)?;
        let mut file = File::open(&self.path)?;
        file.seek(SeekFrom::Start(self.payload_offset + member.offset))?;
        let mut bytes = vec![0_u8; length];
        file.read_exact(&mut bytes)
            .map_err(|_| ModelPackError::Truncated)?;
        if Sha256Digest::calculate(&bytes) != member.checksum {
            return Err(ModelPackError::MemberChecksumMismatch(name.to_owned()));
        }
        Ok(bytes)
    }

    /// Streams and verifies one member without materializing it in memory.
    ///
    /// # Errors
    ///
    /// Returns a typed error when the member is absent, unreadable, truncated,
    /// or its payload differs from the digest recorded in the pack index.
    pub fn verify_member(&self, name: &str) -> Result<(), ModelPackError> {
        let member = self
            .index
            .file(name)
            .ok_or_else(|| ModelPackError::MissingMember(name.to_owned()))?;
        let mut file = File::open(&self.path)?;
        file.seek(SeekFrom::Start(self.payload_offset + member.offset))?;
        let mut remaining = member.length;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0_u8; COPY_BUFFER_BYTES];
        while remaining > 0 {
            let wanted = usize::try_from(remaining.min(buffer.len() as u64))
                .map_err(|_| ModelPackError::Truncated)?;
            let read = file.read(&mut buffer[..wanted])?;
            if read == 0 {
                return Err(ModelPackError::Truncated);
            }
            hasher.update(&buffer[..read]);
            remaining -= read as u64;
        }
        let checksum = Sha256Digest::from_bytes(hasher.finalize().into());
        if checksum != member.checksum {
            return Err(ModelPackError::MemberChecksumMismatch(name.to_owned()));
        }
        Ok(())
    }
}

/// A model pack could not be written, opened, or read.
#[derive(Debug, thiserror::Error)]
pub enum ModelPackError {
    /// The file does not begin with the model pack magic.
    #[error("file is not a Procyon model pack")]
    NotAModelPack,
    /// The pack declares an unsupported layout version.
    #[error("unsupported model pack format version {version}")]
    UnsupportedFormatVersion {
        /// Declared layout version.
        version: u32,
    },
    /// The pack index exceeds the bounded header size.
    #[error("model pack index exceeds the bounded header size")]
    IndexTooLarge,
    /// The pack index failed structural validation.
    #[error("model pack index is invalid: {0}")]
    InvalidIndex(&'static str),
    /// The pack payload is shorter than its index declares.
    #[error("model pack payload is truncated")]
    Truncated,
    /// The requested member is not present in the pack.
    #[error("model pack does not contain the member {0}")]
    MissingMember(String),
    /// A member's bytes do not match its recorded digest.
    #[error("model pack member {0} failed its checksum")]
    MemberChecksumMismatch(String),
    /// The index could not be encoded.
    #[error("model pack index could not be encoded: {0}")]
    Encode(String),
    /// The index could not be decoded.
    #[error("model pack index could not be decoded: {0}")]
    Decode(String),
    /// Filesystem access failed.
    #[error("model pack I/O failed: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(directory: &Path) -> ModelPackSpec {
        std::fs::write(directory.join("graph"), b"graph bytes").expect("graph");
        std::fs::write(directory.join("tokenizer"), b"tokenizer bytes").expect("tokenizer");
        ModelPackSpec {
            kind: ModelPackKind::OnnxTransformerMeanPool,
            model_id: "example.model".into(),
            model_revision: "abc123".into(),
            tokenizer: "example-tokenizer".into(),
            dimensions: 384,
            max_input_tokens: 512,
            query_prefix: "query: ".into(),
            passage_prefix: "passage: ".into(),
            production: false,
            source: "test fixture".into(),
            files: vec![
                ("graph".into(), directory.join("graph")),
                ("tokenizer".into(), directory.join("tokenizer")),
            ],
        }
    }

    #[test]
    fn round_trips_members_and_metadata() {
        let directory = tempfile::tempdir().expect("directory");
        let pack_path = directory.path().join("pack");
        let written = write_model_pack(&pack_path, &spec(directory.path())).expect("write");

        let pack = ModelPack::open(&pack_path).expect("open");
        assert_eq!(pack.index(), &written);
        assert_eq!(pack.index().query_prefix, "query: ");
        pack.verify_member("graph").expect("verified graph");
        assert_eq!(pack.read("graph").expect("graph"), b"graph bytes");
        assert_eq!(
            pack.read("tokenizer").expect("tokenizer"),
            b"tokenizer bytes"
        );
        assert!(matches!(
            pack.read("absent"),
            Err(ModelPackError::MissingMember(_))
        ));
    }

    #[test]
    fn writes_byte_identical_packs_for_identical_inputs() {
        let directory = tempfile::tempdir().expect("directory");
        let first = directory.path().join("first");
        let second = directory.path().join("second");
        write_model_pack(&first, &spec(directory.path())).expect("first");
        write_model_pack(&second, &spec(directory.path())).expect("second");

        assert_eq!(
            std::fs::read(&first).expect("first bytes"),
            std::fs::read(&second).expect("second bytes")
        );
    }

    #[test]
    fn rejects_foreign_truncated_and_tampered_packs() {
        let directory = tempfile::tempdir().expect("directory");
        let foreign = directory.path().join("foreign");
        std::fs::write(&foreign, b"{\"not\":\"a pack\"}").expect("foreign");
        assert!(matches!(
            ModelPack::open(&foreign),
            Err(ModelPackError::NotAModelPack)
        ));

        let pack_path = directory.path().join("pack");
        write_model_pack(&pack_path, &spec(directory.path())).expect("write");
        let mut bytes = std::fs::read(&pack_path).expect("bytes");
        bytes.truncate(bytes.len() - 1);
        std::fs::write(&pack_path, &bytes).expect("truncate");
        assert!(matches!(
            ModelPack::open(&pack_path),
            Err(ModelPackError::Truncated)
        ));

        write_model_pack(&pack_path, &spec(directory.path())).expect("rewrite");
        let mut bytes = std::fs::read(&pack_path).expect("bytes");
        let last = bytes.len() - 1;
        bytes[last] ^= 0xff;
        std::fs::write(&pack_path, &bytes).expect("tamper");
        let pack = ModelPack::open(&pack_path).expect("open tampered");
        assert!(matches!(
            pack.read("tokenizer"),
            Err(ModelPackError::MemberChecksumMismatch(_))
        ));
    }
}
