//! Offline ONNX transformer inference for catalog-installed model packs.
//!
//! The backend is constructed only from bytes that already passed the signed
//! catalog checksum and the model pack's own per-member digests. It performs no
//! network access: the graph and the tokenizer are read from the installed
//! pack, and ONNX Runtime is linked into this executable rather than resolved
//! from an ambient shared library.

use std::sync::Mutex;

use fm_semantic_components::{ModelPack, ModelPackIndex};
use ort::session::Session;
use ort::session::builder::GraphOptimizationLevel;
use ort::value::Value;
use tokenizers::{PaddingParams, PaddingStrategy, Tokenizer, TruncationParams};
use tokio_util::sync::CancellationToken;

use crate::embedding::{CpuEmbeddingBackend, EmbeddingError, EmbeddingModelIdentity};

/// Pack member holding the exported transformer graph.
pub(crate) const GRAPH_MEMBER: &str = "model.onnx";
/// Pack member holding the immutable fast tokenizer.
pub(crate) const TOKENIZER_MEMBER: &str = "tokenizer.json";

const OUTPUT_NAME: &str = "last_hidden_state";
const INPUT_IDS: &str = "input_ids";
const ATTENTION_MASK: &str = "attention_mask";
const TOKEN_TYPE_IDS: &str = "token_type_ids";
const MAX_INFERENCE_THREADS: usize = 4;

/// Mean-pooled transformer embedding backend over an installed model pack.
pub(crate) struct OnnxEmbeddingBackend {
    identity: EmbeddingModelIdentity,
    tokenizer: Tokenizer,
    session: Mutex<Session>,
    expects_token_type_ids: bool,
}

impl OnnxEmbeddingBackend {
    /// Loads the graph and tokenizer from an installed, checksum-verified pack.
    pub(crate) fn load(
        pack: &ModelPack,
        identity: EmbeddingModelIdentity,
    ) -> Result<Self, EmbeddingError> {
        let index: &ModelPackIndex = pack.index();
        let mut tokenizer = Tokenizer::from_bytes(
            pack.read(TOKENIZER_MEMBER)
                .map_err(|error| EmbeddingError::Backend(error.to_string()))?,
        )
        .map_err(|error| EmbeddingError::Backend(format!("tokenizer is unusable: {error}")))?;
        tokenizer
            .with_truncation(Some(TruncationParams {
                max_length: identity.max_input_tokens,
                ..TruncationParams::default()
            }))
            .map_err(|error| {
                EmbeddingError::Backend(format!("tokenizer truncation is unusable: {error}"))
            })?;
        let padding = padding_parameters(&tokenizer)?;
        tokenizer.with_padding(Some(padding));

        let threads = std::thread::available_parallelism()
            .map_or(1, std::num::NonZeroUsize::get)
            .clamp(1, MAX_INFERENCE_THREADS);
        let graph = pack
            .read(GRAPH_MEMBER)
            .map_err(|error| EmbeddingError::Backend(error.to_string()))?;
        let session = Session::builder()
            .map_err(session_error)?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(session_error)?
            .with_intra_threads(threads)
            .map_err(session_error)?
            .commit_from_memory(&graph)
            .map_err(session_error)?;
        drop(graph);

        let outputs = session
            .outputs()
            .iter()
            .map(|outlet| outlet.name())
            .collect::<Vec<_>>();
        if !outputs.contains(&OUTPUT_NAME) {
            return Err(EmbeddingError::Backend(format!(
                "packed graph does not expose {OUTPUT_NAME}"
            )));
        }
        let inputs = session
            .inputs()
            .iter()
            .map(|inlet| inlet.name())
            .collect::<Vec<_>>();
        if !inputs.contains(&INPUT_IDS) || !inputs.contains(&ATTENTION_MASK) {
            return Err(EmbeddingError::Backend(
                "packed graph does not accept token identifiers and an attention mask".into(),
            ));
        }
        let expects_token_type_ids = inputs.contains(&TOKEN_TYPE_IDS);
        if index.dimensions as usize != identity.dimensions {
            return Err(EmbeddingError::ModelIdentityMismatch);
        }

        Ok(Self {
            identity,
            tokenizer,
            session: Mutex::new(session),
            expects_token_type_ids,
        })
    }
}

fn padding_parameters(tokenizer: &Tokenizer) -> Result<PaddingParams, EmbeddingError> {
    let pad_token = "<pad>";
    let pad_id = tokenizer.token_to_id(pad_token).ok_or_else(|| {
        EmbeddingError::Backend("packed tokenizer has no padding token".to_owned())
    })?;
    Ok(PaddingParams {
        strategy: PaddingStrategy::BatchLongest,
        pad_id,
        pad_token: pad_token.to_owned(),
        ..PaddingParams::default()
    })
}

impl CpuEmbeddingBackend for OnnxEmbeddingBackend {
    fn identity(&self) -> &EmbeddingModelIdentity {
        &self.identity
    }

    fn token_count(&self, input: &str) -> Result<usize, EmbeddingError> {
        let encoding = self
            .tokenizer
            .encode(input, true)
            .map_err(|error| EmbeddingError::Backend(format!("tokenization failed: {error}")))?;
        Ok(encoding.len())
    }

    fn embed_batch(
        &self,
        inputs: &[&str],
        cancellation: &CancellationToken,
    ) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        if inputs.is_empty() {
            return Ok(Vec::new());
        }
        if cancellation.is_cancelled() {
            return Err(EmbeddingError::Cancelled);
        }
        let encodings = self
            .tokenizer
            .encode_batch(inputs.to_vec(), true)
            .map_err(|error| EmbeddingError::Backend(format!("tokenization failed: {error}")))?;
        let batch = encodings.len();
        let sequence = encodings
            .iter()
            .map(tokenizers::Encoding::len)
            .max()
            .unwrap_or(0)
            .max(1);
        let mut identifiers = Vec::with_capacity(batch * sequence);
        let mut mask = Vec::with_capacity(batch * sequence);
        for encoding in &encodings {
            let ids = encoding.get_ids();
            let attention = encoding.get_attention_mask();
            for position in 0..sequence {
                identifiers.push(i64::from(ids.get(position).copied().unwrap_or(0)));
                mask.push(i64::from(attention.get(position).copied().unwrap_or(0)));
            }
        }
        if mask.iter().all(|value| *value == 0) {
            return Err(EmbeddingError::Backend(
                "packed tokenizer produced no attended tokens".into(),
            ));
        }

        let shape = [
            i64::try_from(batch).map_err(|_| EmbeddingError::InvalidModelLimits)?,
            i64::try_from(sequence).map_err(|_| EmbeddingError::InvalidModelLimits)?,
        ];
        let mut arguments = vec![
            (
                INPUT_IDS,
                Value::from_array((shape, identifiers)).map_err(tensor_error)?,
            ),
            (
                ATTENTION_MASK,
                Value::from_array((shape, mask.clone())).map_err(tensor_error)?,
            ),
        ];
        if self.expects_token_type_ids {
            arguments.push((
                TOKEN_TYPE_IDS,
                Value::from_array((shape, vec![0_i64; batch * sequence])).map_err(tensor_error)?,
            ));
        }

        if cancellation.is_cancelled() {
            return Err(EmbeddingError::Cancelled);
        }
        let mut session = self
            .session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let outputs = session
            .run(arguments)
            .map_err(|error| EmbeddingError::Backend(format!("ONNX inference failed: {error}")))?;
        let (output_shape, values) = outputs
            .get(OUTPUT_NAME)
            .ok_or_else(|| EmbeddingError::Backend(format!("graph produced no {OUTPUT_NAME}")))?
            .try_extract_tensor::<f32>()
            .map_err(|error| {
                EmbeddingError::Backend(format!("graph output is not float32: {error}"))
            })?;
        if output_shape.len() != 3 {
            return Err(EmbeddingError::Backend(
                "graph output is not a batch of token states".into(),
            ));
        }
        let dimensions =
            usize::try_from(output_shape[2]).map_err(|_| EmbeddingError::InvalidModelLimits)?;
        if dimensions != self.identity.dimensions || values.len() != batch * sequence * dimensions {
            return Err(EmbeddingError::ModelIdentityMismatch);
        }

        let mut vectors = Vec::with_capacity(batch);
        for item in 0..batch {
            let mut pooled = vec![0.0_f32; dimensions];
            let mut attended = 0.0_f32;
            for position in 0..sequence {
                if mask[item * sequence + position] == 0 {
                    continue;
                }
                attended += 1.0;
                let start = (item * sequence + position) * dimensions;
                for (index, value) in pooled.iter_mut().enumerate() {
                    *value += values[start + index];
                }
            }
            if attended == 0.0 {
                return Err(EmbeddingError::Backend(
                    "packed tokenizer produced no attended tokens".into(),
                ));
            }
            for value in &mut pooled {
                *value /= attended;
            }
            vectors.push(pooled);
        }
        Ok(vectors)
    }
}

fn session_error<T>(error: ort::Error<T>) -> EmbeddingError {
    EmbeddingError::Backend(format!("ONNX session failed: {error}"))
}

fn tensor_error(error: ort::Error) -> EmbeddingError {
    EmbeddingError::Backend(format!("input tensor could not be built: {error}"))
}
