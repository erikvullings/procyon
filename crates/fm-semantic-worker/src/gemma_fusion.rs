//! Experimental CPU encoder for text with projected image, audio or video soft tokens.

use std::fs::File;
use std::path::Path;

use lattice_inference::forward::cpu::{elementwise_mul, matmul_bt};
use lattice_inference::model::embeddinggemma2_config::{
    EmbeddingGemma2Config, EmbeddingGemma2LayerKind,
};
use lattice_inference::model::gemma4_ops::{
    gemma4_apply_rope, gemma4_geglu_mlp, gemma4_gelu_tanh, gemma4_qk_norm_v_unscaled,
    gemma4_rms_norm, gemma4_rope_cos_sin, gemma4_rope_inv_freq,
};
use lattice_inference::weights::{SafetensorsFile, TensorSource};

const MAX_TOKENS: usize = 8192;

struct Layer {
    input_norm: Vec<f32>,
    attention_norm: Vec<f32>,
    feed_norm: Vec<f32>,
    feed_output_norm: Vec<f32>,
    q: Vec<f32>,
    k: Vec<f32>,
    v: Vec<f32>,
    o: Vec<f32>,
    q_norm: Vec<f32>,
    k_norm: Vec<f32>,
    gate: Vec<f32>,
    up: Vec<f32>,
    down: Vec<f32>,
    ple_gate: Vec<f32>,
    ple_projection: Vec<f32>,
    ple_output_norm: Vec<f32>,
    residual_scale: f32,
}

/// Bidirectional EmbeddingGemma 2 language model with soft-token replacement.
pub struct GemmaFusionEncoder {
    config: EmbeddingGemma2Config,
    embeddings: Vec<f32>,
    ple_projection: Vec<f32>,
    ple_norm: Vec<f32>,
    layers: Vec<Layer>,
    output_norm: Vec<f32>,
    output_projection: Vec<f32>,
    media_ids: [u32; 3],
}

fn tensor(
    file: &mut SafetensorsFile,
    name: &str,
    expected: &[usize],
) -> Result<Vec<f32>, GemmaFusionError> {
    let dtype = TensorSource::tensor_dtype(file, name)?;
    if !matches!(dtype.as_deref(), Some("BF16" | "F32")) {
        return Err(GemmaFusionError::UnsupportedDtype(name.to_owned(), dtype));
    }
    if TensorSource::tensor_shape(file, name)?.as_deref() != Some(expected) {
        return Err(GemmaFusionError::InvalidCheckpoint(name.to_owned()));
    }
    let (values, shape) = file.get_f32_tensor_owned(name)?;
    if shape != expected || values.iter().any(|v| !v.is_finite()) {
        return Err(GemmaFusionError::InvalidCheckpoint(name.to_owned()));
    }
    Ok(values)
}

impl GemmaFusionEncoder {
    /// Reads validated BF16/F32 text weights from a local checkpoint.
    pub fn open(directory: &Path) -> Result<Self, GemmaFusionError> {
        let config = EmbeddingGemma2Config::from_model_dir(directory)?;
        let root: serde_json::Value =
            serde_json::from_reader(File::open(directory.join("config.json"))?)?;
        let media_ids = [
            root["image_token_id"].as_u64(),
            root["audio_token_id"].as_u64(),
            root["video_token_id"].as_u64(),
        ]
        .map(|value| value.and_then(|id| u32::try_from(id).ok()));
        let [Some(image), Some(audio), Some(video)] = media_ids else {
            return Err(GemmaFusionError::InvalidCheckpoint(
                "media token ids".into(),
            ));
        };
        if config.hidden_size != 512 || config.embedding_dim != 768 {
            return Err(GemmaFusionError::InvalidCheckpoint("text geometry".into()));
        }
        let mut file = SafetensorsFile::open(&directory.join("model.safetensors"))?;
        let hidden = config.hidden_size;
        let ple = config.hidden_size_per_layer_input;
        let layers = config.num_hidden_layers;
        let embeddings = tensor(
            &mut file,
            "language_model.embed_tokens.weight",
            &[config.vocab_size, hidden],
        )?;
        let ple_projection = tensor(
            &mut file,
            "language_model.ple.per_layer_model_projection.weight",
            &[layers * ple, hidden],
        )?;
        let ple_norm = tensor(
            &mut file,
            "language_model.ple.per_layer_projection_norm.weight",
            &[ple],
        )?;
        let mut weights = Vec::with_capacity(layers);
        for index in 0..layers {
            let prefix = format!("language_model.layers.{index}.");
            let shape = config.layer_shapes[index];
            let query = config.num_attention_heads * shape.head_dim;
            let key_value = shape.num_key_value_heads * shape.head_dim;
            let ff = config.intermediate_size;
            let mut get =
                |name: &str, shape: &[usize]| tensor(&mut file, &format!("{prefix}{name}"), shape);
            let residual_scale = get("layer_scalar", &[1])?[0];
            weights.push(Layer {
                input_norm: get("input_layernorm.weight", &[hidden])?,
                attention_norm: get("post_attention_layernorm.weight", &[hidden])?,
                feed_norm: get("pre_feedforward_layernorm.weight", &[hidden])?,
                feed_output_norm: get("post_feedforward_layernorm.weight", &[hidden])?,
                q: get("self_attn.q_proj.weight", &[query, hidden])?,
                k: get("self_attn.k_proj.weight", &[key_value, hidden])?,
                v: get("self_attn.v_proj.weight", &[key_value, hidden])?,
                o: get("self_attn.o_proj.weight", &[hidden, query])?,
                q_norm: get("self_attn.q_norm.weight", &[shape.head_dim])?,
                k_norm: get("self_attn.k_norm.weight", &[shape.head_dim])?,
                gate: get("mlp.gate_proj.weight", &[ff, hidden])?,
                up: get("mlp.up_proj.weight", &[ff, hidden])?,
                down: get("mlp.down_proj.weight", &[hidden, ff])?,
                ple_gate: get("ple_block.per_layer_input_gate.weight", &[ple, hidden])?,
                ple_projection: get("ple_block.per_layer_projection.weight", &[hidden, ple])?,
                ple_output_norm: get("ple_block.post_per_layer_input_norm.weight", &[hidden])?,
                residual_scale,
            });
        }
        let output_norm = tensor(&mut file, "language_model.norm.weight", &[hidden])?;
        let output_projection = tensor(
            &mut file,
            "language_model.embedding_projection.weight",
            &[config.embedding_dim, hidden],
        )?;
        Ok(Self {
            config,
            embeddings,
            ple_projection,
            ple_norm,
            layers: weights,
            output_norm,
            output_projection,
            media_ids: [image, audio, video],
        })
    }

    /// Encodes token IDs with every media placeholder replaced by its projected soft token.
    pub fn encode(
        &self,
        ids: &[u32],
        replacements: &[(usize, &[f32])],
        dimensions: usize,
    ) -> Result<Vec<f32>, GemmaFusionError> {
        if ids.is_empty() || ids.len() > MAX_TOKENS || ![128, 256, 512, 768].contains(&dimensions) {
            return Err(GemmaFusionError::InvalidInput(
                "sequence length or dimension",
            ));
        }
        let cfg = &self.config;
        let width = cfg.hidden_size;
        let length = ids.len();
        let mut states = vec![0.0; length * width];
        let mut replaced = vec![false; length];
        for &(position, token) in replacements {
            if position >= length
                || !self.media_ids.contains(&ids[position])
                || replaced[position]
                || token.len() != width
                || token.iter().any(|v| !v.is_finite())
            {
                return Err(GemmaFusionError::InvalidInput("media token replacement"));
            }
            replaced[position] = true;
            states[position * width..(position + 1) * width].copy_from_slice(token);
        }
        for (position, &id) in ids.iter().enumerate() {
            if self.media_ids.contains(&id) {
                if !replaced[position] {
                    return Err(GemmaFusionError::InvalidInput("missing media soft token"));
                }
            } else {
                let id = id as usize;
                if id >= cfg.vocab_size {
                    return Err(GemmaFusionError::InvalidInput(
                        "token id outside vocabulary",
                    ));
                }
                for (dst, weight) in states[position * width..(position + 1) * width]
                    .iter_mut()
                    .zip(&self.embeddings[id * width..(id + 1) * width])
                {
                    *dst = weight * cfg.embed_scale;
                }
            }
        }
        let base = states.clone();
        for (index, layer) in self.layers.iter().enumerate() {
            self.apply_layer(index, layer, &base, &mut states);
        }
        gemma4_rms_norm(&mut states, &self.output_norm, width, cfg.rms_norm_eps);
        let mut projected = vec![0.0; length * cfg.embedding_dim];
        matmul_bt(
            &states,
            &self.output_projection,
            &mut projected,
            length,
            width,
            cfg.embedding_dim,
        );
        let mut sum = vec![0.0_f64; dimensions];
        for row in projected.chunks_exact(cfg.embedding_dim) {
            for (dst, value) in sum.iter_mut().zip(row) {
                *dst += f64::from(*value);
            }
        }
        let norm = sum.iter().map(|value| value * value).sum::<f64>().sqrt();
        if norm == 0.0 || !norm.is_finite() {
            return Err(GemmaFusionError::InvalidOutput);
        }
        let vector: Vec<f32> = sum.into_iter().map(|value| (value / norm) as f32).collect();
        if vector.iter().any(|value| !value.is_finite()) {
            return Err(GemmaFusionError::InvalidOutput);
        }
        Ok(vector)
    }

    fn apply_layer(&self, index: usize, layer: &Layer, base: &[f32], states: &mut [f32]) {
        let cfg = &self.config;
        let n = states.len() / cfg.hidden_size;
        let h = cfg.hidden_size;
        let shape = cfg.layer_shapes[index];
        let q_dim = cfg.num_attention_heads * shape.head_dim;
        let kv_dim = shape.num_key_value_heads * shape.head_dim;
        let eps = cfg.rms_norm_eps;
        let mut normalized = states.to_vec();
        gemma4_rms_norm(&mut normalized, &layer.input_norm, h, eps);
        let mut q = vec![0.0; n * q_dim];
        let mut k = vec![0.0; n * kv_dim];
        let mut v = vec![0.0; n * kv_dim];
        matmul_bt(&normalized, &layer.q, &mut q, n, h, q_dim);
        matmul_bt(&normalized, &layer.k, &mut k, n, h, kv_dim);
        matmul_bt(&normalized, &layer.v, &mut v, n, h, kv_dim);
        gemma4_qk_norm_v_unscaled(
            &mut q,
            &mut k,
            &mut v,
            &layer.q_norm,
            &layer.k_norm,
            shape.head_dim,
            eps,
        );
        let theta = if cfg.layer_types[index] == EmbeddingGemma2LayerKind::Full {
            cfg.rope_theta_full
        } else {
            cfg.rope_theta_sliding
        };
        let frequencies = gemma4_rope_inv_freq(shape.head_dim, theta, None);
        let positions: Vec<u32> = (0..n as u32).collect();
        let (cos, sin) = gemma4_rope_cos_sin(&frequencies, &positions);
        gemma4_apply_rope(
            &mut q,
            &cos,
            &sin,
            n,
            cfg.num_attention_heads,
            shape.head_dim,
        );
        gemma4_apply_rope(
            &mut k,
            &cos,
            &sin,
            n,
            shape.num_key_value_heads,
            shape.head_dim,
        );
        let window = (cfg.layer_types[index] == EmbeddingGemma2LayerKind::Sliding)
            .then_some(cfg.sliding_window);
        let attended = attention(
            &q,
            &k,
            &v,
            n,
            cfg.num_attention_heads,
            shape.num_key_value_heads,
            shape.head_dim,
            window,
        );
        let mut result = vec![0.0; n * h];
        matmul_bt(&attended, &layer.o, &mut result, n, q_dim, h);
        gemma4_rms_norm(&mut result, &layer.attention_norm, h, eps);
        add(states, &result);

        normalized.copy_from_slice(states);
        gemma4_rms_norm(&mut normalized, &layer.feed_norm, h, eps);
        let ff = cfg.intermediate_size;
        let mut feed = vec![0.0; n * h];
        gemma4_geglu_mlp(
            &normalized,
            &layer.gate,
            &layer.up,
            &layer.down,
            n,
            h,
            ff,
            &mut vec![0.0; n * ff],
            &mut vec![0.0; n * ff],
            &mut feed,
        );
        gemma4_rms_norm(&mut feed, &layer.feed_output_norm, h, eps);
        add(states, &feed);

        let ple_width = cfg.hidden_size_per_layer_input;
        let mut ple = vec![0.0; n * ple_width];
        let start = index * ple_width * h;
        matmul_bt(
            base,
            &self.ple_projection[start..start + ple_width * h],
            &mut ple,
            n,
            h,
            ple_width,
        );
        let scale = (h as f64).powf(-0.5) as f32;
        for value in &mut ple {
            *value *= scale;
        }
        gemma4_rms_norm(&mut ple, &self.ple_norm, ple_width, eps);
        let mut gate = vec![0.0; n * ple_width];
        matmul_bt(states, &layer.ple_gate, &mut gate, n, h, ple_width);
        gemma4_gelu_tanh(&mut gate);
        elementwise_mul(&mut gate, &ple);
        let mut input = vec![0.0; n * h];
        matmul_bt(&gate, &layer.ple_projection, &mut input, n, ple_width, h);
        gemma4_rms_norm(&mut input, &layer.ple_output_norm, h, eps);
        add(states, &input);
        for value in states {
            *value *= layer.residual_scale;
        }
    }
}

fn add(left: &mut [f32], right: &[f32]) {
    for (l, r) in left.iter_mut().zip(right) {
        *l += r;
    }
}

#[allow(clippy::too_many_arguments)]
fn attention(
    q: &[f32],
    k: &[f32],
    v: &[f32],
    tokens: usize,
    heads: usize,
    kv_heads: usize,
    head_dim: usize,
    window: Option<usize>,
) -> Vec<f32> {
    let mut output = vec![0.0; q.len()];
    let mut scores = vec![0.0; tokens];
    let group = heads / kv_heads;
    for position in 0..tokens {
        let start = window.map_or(0, |radius| position.saturating_sub(radius));
        let end = window.map_or(tokens, |radius| (position + radius + 1).min(tokens));
        for head in 0..heads {
            let key_head = head / group;
            let query =
                &q[(position * heads + head) * head_dim..(position * heads + head + 1) * head_dim];
            let mut maximum = f32::NEG_INFINITY;
            for key in start..end {
                let other = &k[(key * kv_heads + key_head) * head_dim
                    ..(key * kv_heads + key_head + 1) * head_dim];
                let score = query.iter().zip(other).map(|(a, b)| a * b).sum();
                scores[key] = score;
                maximum = maximum.max(score);
            }
            let total: f32 = scores[start..end]
                .iter_mut()
                .map(|score| {
                    *score = (*score - maximum).exp();
                    *score
                })
                .sum();
            let destination = &mut output
                [(position * heads + head) * head_dim..(position * heads + head + 1) * head_dim];
            for key in start..end {
                let source = &v[(key * kv_heads + key_head) * head_dim
                    ..(key * kv_heads + key_head + 1) * head_dim];
                let weight = scores[key] / total;
                for (out, value) in destination.iter_mut().zip(source) {
                    *out += weight * value;
                }
            }
        }
    }
    output
}

/// Invalid checkpoint or soft-token fusion input.
#[derive(Debug, thiserror::Error)]
pub enum GemmaFusionError {
    /// Input sequence or replacement is unsupported.
    #[error("invalid Gemma fusion input: {0}")]
    InvalidInput(&'static str),
    /// Checkpoint metadata or weight dimensions are incompatible.
    #[error("invalid Gemma fusion checkpoint: {0}")]
    InvalidCheckpoint(String),
    /// FP16 weights cannot safely run this model.
    #[error("unsupported tensor {0} dtype: {1:?}; expected BF16 or F32")]
    UnsupportedDtype(String, Option<String>),
    /// Inference produced a non-finite or zero embedding.
    #[error("invalid Gemma fusion output")]
    InvalidOutput,
    /// Failed to read checkpoint data.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// Failed to parse checkpoint metadata.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// Native tensor loading failed.
    #[error(transparent)]
    Inference(#[from] lattice_inference::InferenceError),
}
