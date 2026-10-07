//! Experimental checkpoint-backed Gemma4 audio encoder for already prepared 128-band features.

use std::fs::File;
use std::path::Path;

use lattice_inference::weights::{SafetensorsFile, TensorSource};

const BANDS: usize = 128;
const WIDTH: usize = 1024;
const HEADS: usize = 8;
const HEAD_WIDTH: usize = 128;
const CHUNK: usize = 12;
const LEFT: usize = 12;
const MAX_FRAMES: usize = 3000;

/// Invalid or unsupported checkpoint and audio feature input.
#[derive(Debug, thiserror::Error)]
pub enum GemmaAudioError {
    /// Local checkpoint or configuration could not be read.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// Checkpoint configuration is not valid JSON.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// Checkpoint tensor loading failed.
    #[error(transparent)]
    Checkpoint(#[from] lattice_inference::InferenceError),
    /// The checkpoint audio architecture differs from the supported one.
    #[error("incompatible Gemma4 audio configuration")]
    Config,
    /// FP16 or another unsafe weight dtype was supplied.
    #[error("audio tensor {0} has unsupported dtype {1:?}; expected BF16 or F32")]
    Dtype(String, Option<String>),
    /// The tensor does not have the expected shape.
    #[error("audio tensor {0} has unexpected shape {1:?}")]
    Shape(String, Vec<usize>),
    /// Non-finite feature or checkpoint weight.
    #[error("non-finite audio tensor or feature")]
    NonFinite,
    /// Feature layout or frame count is invalid.
    #[error("invalid audio feature dimensions or frame count")]
    Features,
}

type Result<T> = std::result::Result<T, GemmaAudioError>;

struct Linear {
    weight: Vec<f32>,
    input: usize,
    output: usize,
    bounds: [f32; 4],
}

impl Linear {
    fn load(
        checkpoint: &mut SafetensorsFile,
        prefix: &str,
        input: usize,
        output: usize,
    ) -> Result<Self> {
        let weight = tensor(
            checkpoint,
            &format!("{prefix}.linear.weight"),
            &[output, input],
        )?;
        let mut bounds = [0.0; 4];
        for (slot, suffix) in
            bounds
                .iter_mut()
                .zip(["input_min", "input_max", "output_min", "output_max"])
        {
            *slot = tensor(checkpoint, &format!("{prefix}.{suffix}"), &[])?[0];
        }
        Ok(Self {
            weight,
            input,
            output,
            bounds,
        })
    }

    fn apply(&self, values: &[f32]) -> Vec<f32> {
        let mut output = vec![0.0; self.output];
        for (row, value) in self.weight.chunks_exact(self.input).zip(&mut output) {
            *value = dot_clipped(row, values, self.bounds[0], self.bounds[1])
                .clamp(self.bounds[2], self.bounds[3]);
        }
        output
    }
}

fn dot_clipped(weights: &[f32], values: &[f32], min: f32, max: f32) -> f32 {
    weights
        .iter()
        .zip(values)
        .map(|(w, v)| w * v.clamp(min, max))
        .sum()
}

fn tensor(checkpoint: &mut SafetensorsFile, name: &str, expected: &[usize]) -> Result<Vec<f32>> {
    let dtype = checkpoint.tensor_dtype(name)?;
    if !matches!(dtype.as_deref(), Some("BF16" | "F32")) {
        return Err(GemmaAudioError::Dtype(name.to_owned(), dtype));
    }
    let (values, shape) = checkpoint.get_f32_tensor_owned(name)?;
    if shape != expected {
        return Err(GemmaAudioError::Shape(name.to_owned(), shape));
    }
    if values.iter().any(|v| !v.is_finite()) {
        return Err(GemmaAudioError::NonFinite);
    }
    Ok(values)
}

fn rms(values: &mut [f32], scale: &[f32]) {
    let mean = values.iter().map(|v| v * v).sum::<f32>() / values.len() as f32;
    let factor = (mean + 1e-6).powf(-0.5);
    for (v, weight) in values.iter_mut().zip(scale) {
        *v *= factor * weight;
    }
}

fn silu(value: f32) -> f32 {
    value / (1.0 + (-value).exp())
}

struct FeedForward {
    first: Linear,
    second: Linear,
    pre: Vec<f32>,
    post: Vec<f32>,
}

impl FeedForward {
    fn load(checkpoint: &mut SafetensorsFile, prefix: &str) -> Result<Self> {
        Ok(Self {
            first: Linear::load(
                checkpoint,
                &format!("{prefix}.ffw_layer_1"),
                WIDTH,
                WIDTH * 4,
            )?,
            second: Linear::load(
                checkpoint,
                &format!("{prefix}.ffw_layer_2"),
                WIDTH * 4,
                WIDTH,
            )?,
            pre: tensor(
                checkpoint,
                &format!("{prefix}.pre_layer_norm.weight"),
                &[WIDTH],
            )?,
            post: tensor(
                checkpoint,
                &format!("{prefix}.post_layer_norm.weight"),
                &[WIDTH],
            )?,
        })
    }

    fn run(&self, tokens: &mut [Vec<f32>]) {
        for token in tokens {
            let mut normalized = token.clone();
            rms(&mut normalized, &self.pre);
            let mut middle = self.first.apply(&normalized);
            middle.iter_mut().for_each(|v| *v = silu(*v));
            let mut output = self.second.apply(&middle);
            rms(&mut output, &self.post);
            for (value, delta) in token.iter_mut().zip(output) {
                *value += delta * 0.5;
            }
        }
    }
}

struct Attention {
    query: Linear,
    key: Linear,
    value: Linear,
    post: Linear,
    relative_key: Vec<f32>,
    per_dim_scale: Vec<f32>,
}

impl Attention {
    fn load(checkpoint: &mut SafetensorsFile, prefix: &str) -> Result<Self> {
        Ok(Self {
            query: Linear::load(checkpoint, &format!("{prefix}.q_proj"), WIDTH, WIDTH)?,
            key: Linear::load(checkpoint, &format!("{prefix}.k_proj"), WIDTH, WIDTH)?,
            value: Linear::load(checkpoint, &format!("{prefix}.v_proj"), WIDTH, WIDTH)?,
            post: Linear::load(checkpoint, &format!("{prefix}.post"), WIDTH, WIDTH)?,
            relative_key: tensor(
                checkpoint,
                &format!("{prefix}.relative_k_proj.weight"),
                &[WIDTH, WIDTH],
            )?,
            per_dim_scale: tensor(
                checkpoint,
                &format!("{prefix}.per_dim_scale"),
                &[HEAD_WIDTH],
            )?,
        })
    }

    fn run(&self, tokens: &[Vec<f32>], valid: usize, positions: &[Vec<f32>]) -> Vec<Vec<f32>> {
        let length = tokens.len();
        let queries: Vec<_> = tokens.iter().map(|v| self.query.apply(v)).collect();
        let keys: Vec<_> = tokens.iter().map(|v| self.key.apply(v)).collect();
        let values: Vec<_> = tokens.iter().map(|v| self.value.apply(v)).collect();
        let relative: Vec<_> = positions
            .iter()
            .map(|p| {
                self.relative_key
                    .as_chunks::<WIDTH>()
                    .0
                    .iter()
                    .map(|row| dot_clipped(row, p, f32::NEG_INFINITY, f32::INFINITY))
                    .collect::<Vec<_>>()
            })
            .collect();
        let q_scale = (HEAD_WIDTH as f32).powf(-0.5) / 2.0_f32.ln();
        let k_scale = (1.0 + std::f32::consts::E).ln() / 2.0_f32.ln();
        let mut output = Vec::with_capacity(length);
        for (i, query) in queries.iter().enumerate() {
            let mut combined = vec![0.0; WIDTH];
            for head in 0..HEADS {
                let start = head * HEAD_WIDTH;
                let q = &query[start..start + HEAD_WIDTH];
                let block_start = (i / CHUNK) * CHUNK;
                let context_start = block_start as isize - LEFT as isize;
                let mut logits = [f32::NEG_INFINITY; CHUNK + LEFT];
                for (j, logit) in logits.iter_mut().enumerate() {
                    let index = context_start + j as isize;
                    let relative_index = j as isize - (i % CHUNK) as isize;
                    if index < 0
                        || index >= length as isize
                        || index >= valid as isize
                        || relative_index < 0
                        || relative_index > LEFT as isize
                    {
                        continue;
                    }
                    let qk = q
                        .iter()
                        .zip(&keys[index as usize][start..start + HEAD_WIDTH])
                        .zip(&self.per_dim_scale)
                        .map(|((a, b), s)| a * q_scale * (1.0 + s.exp()).ln() * b * k_scale)
                        .sum::<f32>();
                    let rel = q
                        .iter()
                        .zip(&relative[relative_index as usize][start..start + HEAD_WIDTH])
                        .zip(&self.per_dim_scale)
                        .map(|((a, b), s)| a * q_scale * (1.0 + s.exp()).ln() * b)
                        .sum::<f32>();
                    *logit = ((qk + rel) / 50.0).tanh() * 50.0;
                }
                let maximum = logits.iter().copied().fold(f32::NEG_INFINITY, f32::max);
                if !maximum.is_finite() {
                    continue;
                }
                let exp: Vec<_> = logits.iter().map(|v| (v - maximum).exp()).collect();
                let sum: f32 = exp.iter().sum();
                for (j, score) in exp.iter().enumerate() {
                    let index = context_start + j as isize;
                    if index < 0 || index >= length as isize {
                        continue;
                    }
                    for (dst, src) in combined[start..start + HEAD_WIDTH]
                        .iter_mut()
                        .zip(&values[index as usize][start..start + HEAD_WIDTH])
                    {
                        *dst += score / sum * src;
                    }
                }
            }
            output.push(self.post.apply(&combined));
        }
        output
    }
}

struct LightConv {
    start: Linear,
    end: Linear,
    depthwise: Vec<f32>,
    pre: Vec<f32>,
    norm: Vec<f32>,
}

impl LightConv {
    fn load(checkpoint: &mut SafetensorsFile, prefix: &str) -> Result<Self> {
        Ok(Self {
            start: Linear::load(
                checkpoint,
                &format!("{prefix}.linear_start"),
                WIDTH,
                WIDTH * 2,
            )?,
            end: Linear::load(checkpoint, &format!("{prefix}.linear_end"), WIDTH, WIDTH)?,
            depthwise: tensor(
                checkpoint,
                &format!("{prefix}.depthwise_conv1d.weight"),
                &[WIDTH, 1, 5],
            )?,
            pre: tensor(
                checkpoint,
                &format!("{prefix}.pre_layer_norm.weight"),
                &[WIDTH],
            )?,
            norm: tensor(checkpoint, &format!("{prefix}.conv_norm.weight"), &[WIDTH])?,
        })
    }

    fn run(&self, tokens: &mut [Vec<f32>]) {
        let gated: Vec<_> = tokens
            .iter()
            .map(|token| {
                let mut normalized = token.clone();
                rms(&mut normalized, &self.pre);
                let output = self.start.apply(&normalized);
                (0..WIDTH)
                    .map(|i| output[i] / (1.0 + (-output[i + WIDTH]).exp()))
                    .collect::<Vec<f32>>()
            })
            .collect();
        for (i, token) in tokens.iter_mut().enumerate() {
            let mut conv = vec![0.0; WIDTH];
            for channel in 0..WIDTH {
                let kernel = &self.depthwise[channel * 5..channel * 5 + 5];
                for (offset, weight) in kernel.iter().enumerate() {
                    if i + offset >= 4 {
                        conv[channel] += weight * gated[i + offset - 4][channel];
                    }
                }
            }
            rms(&mut conv, &self.norm);
            conv.iter_mut().for_each(|v| *v = silu(*v));
            let result = self.end.apply(&conv);
            for (v, delta) in token.iter_mut().zip(result) {
                *v += delta;
            }
        }
    }
}

struct Layer {
    ff1: FeedForward,
    ff2: FeedForward,
    attention: Attention,
    convolution: LightConv,
    pre_attention: Vec<f32>,
    post_attention: Vec<f32>,
    out: Vec<f32>,
}

impl Layer {
    fn load(checkpoint: &mut SafetensorsFile, index: usize) -> Result<Self> {
        let p = format!("audio_tower.layers.{index}");
        Ok(Self {
            ff1: FeedForward::load(checkpoint, &format!("{p}.feed_forward1"))?,
            ff2: FeedForward::load(checkpoint, &format!("{p}.feed_forward2"))?,
            attention: Attention::load(checkpoint, &format!("{p}.self_attn"))?,
            convolution: LightConv::load(checkpoint, &format!("{p}.lconv1d"))?,
            pre_attention: tensor(checkpoint, &format!("{p}.norm_pre_attn.weight"), &[WIDTH])?,
            post_attention: tensor(checkpoint, &format!("{p}.norm_post_attn.weight"), &[WIDTH])?,
            out: tensor(checkpoint, &format!("{p}.norm_out.weight"), &[WIDTH])?,
        })
    }

    fn run(&self, tokens: &mut [Vec<f32>], valid: usize, positions: &[Vec<f32>]) {
        self.ff1.run(tokens);
        let mut normalized = tokens.to_vec();
        for token in &mut normalized {
            rms(token, &self.pre_attention);
        }
        let mut attn = self.attention.run(&normalized, valid, positions);
        for (delta, token) in attn.iter_mut().zip(tokens.iter_mut()) {
            rms(delta, &self.post_attention);
            for (v, d) in token.iter_mut().zip(delta.iter()) {
                *v += d;
            }
        }
        self.convolution.run(tokens);
        self.ff2.run(tokens);
        for token in tokens {
            rms(token, &self.out);
        }
    }
}

struct Conv2 {
    weights: Vec<f32>,
    norm: Vec<f32>,
    in_channels: usize,
    out_channels: usize,
}

impl Conv2 {
    fn load(
        checkpoint: &mut SafetensorsFile,
        index: usize,
        input: usize,
        output: usize,
    ) -> Result<Self> {
        let p = format!("audio_tower.subsample_conv_projection.layer{index}");
        Ok(Self {
            weights: tensor(
                checkpoint,
                &format!("{p}.conv.weight"),
                &[output, input, 3, 3],
            )?,
            norm: tensor(checkpoint, &format!("{p}.norm.weight"), &[output])?,
            in_channels: input,
            out_channels: output,
        })
    }

    fn run(
        &self,
        input: &[f32],
        frames: usize,
        bands: usize,
        valid: usize,
    ) -> (Vec<f32>, usize, usize) {
        let rows = frames.div_ceil(2);
        let cols = bands.div_ceil(2);
        let mut output = vec![0.0; rows * cols * self.out_channels];
        for row in 0..rows {
            for col in 0..cols {
                let cell = &mut output[(row * cols + col) * self.out_channels
                    ..(row * cols + col + 1) * self.out_channels];
                for (channel, dst) in cell.iter_mut().enumerate() {
                    let mut total = 0.0;
                    for previous in 0..self.in_channels {
                        for kr in 0..3 {
                            let r = (row * 2 + kr) as isize - 1;
                            if r < 0 || r >= frames as isize || r >= valid as isize {
                                continue;
                            }
                            for kc in 0..3 {
                                let c = (col * 2 + kc) as isize - 1;
                                if c < 0 || c >= bands as isize {
                                    continue;
                                }
                                let weight = self.weights
                                    [((channel * self.in_channels + previous) * 3 + kr) * 3 + kc];
                                total += input[((r as usize * bands + c as usize)
                                    * self.in_channels)
                                    + previous]
                                    * weight;
                            }
                        }
                    }
                    *dst = total;
                }
                let mean = cell.iter().sum::<f32>() / self.out_channels as f32;
                let variance =
                    cell.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / self.out_channels as f32;
                let inv = (variance + 1e-6).sqrt().recip();
                for (value, scale) in cell.iter_mut().zip(&self.norm) {
                    *value = ((*value - mean) * inv * scale).max(0.0);
                }
            }
        }
        (output, rows, cols)
    }
}

/// Offline native FP32 Gemma4 audio tower; inputs are preprocessed 16-kHz 128-band features.
pub struct GemmaAudioTower {
    conv0: Conv2,
    conv1: Conv2,
    input_projection: Vec<f32>,
    layers: Vec<Layer>,
    output_projection: Vec<f32>,
    output_bias: Vec<f32>,
}

impl GemmaAudioTower {
    /// Load the local BF16/F32 audio encoder weights for FP32 CPU inference.
    pub fn open(directory: &Path) -> Result<Self> {
        let config: serde_json::Value =
            serde_json::from_reader(File::open(directory.join("config.json"))?)?;
        let audio = &config["audio_config"];
        if config["model_type"] != "embedding_gemma2"
            || audio["model_type"] != "gemma4_audio"
            || audio["hidden_size"] != WIDTH
            || audio["num_hidden_layers"] != 12
            || audio["num_attention_heads"] != HEADS
            || audio["output_proj_dims"] != 1536
            || audio["subsampling_conv_channels"] != serde_json::json!([128, 32])
            || audio["attention_chunk_size"] != CHUNK
            || audio["attention_context_left"] != 13
            || audio["attention_context_right"] != 0
            || audio["conv_kernel_size"] != 5
            || audio["rms_norm_eps"] != 1e-6
            || audio["hidden_act"] != "silu"
            || audio["residual_weight"] != 0.5
            || audio["attention_logit_cap"] != 50.0
            || audio["use_clipped_linears"] != true
            || audio["gradient_clipping"] != 1e10
        {
            return Err(GemmaAudioError::Config);
        }

        let mut checkpoint = SafetensorsFile::open(&directory.join("model.safetensors"))?;
        Ok(Self {
            conv0: Conv2::load(&mut checkpoint, 0, 1, 128)?,
            conv1: Conv2::load(&mut checkpoint, 1, 128, 32)?,
            input_projection: tensor(
                &mut checkpoint,
                "audio_tower.subsample_conv_projection.input_proj_linear.weight",
                &[WIDTH, WIDTH],
            )?,
            layers: (0..12)
                .map(|i| Layer::load(&mut checkpoint, i))
                .collect::<Result<Vec<_>>>()?,
            output_projection: tensor(
                &mut checkpoint,
                "audio_tower.output_proj.weight",
                &[1536, WIDTH],
            )?,
            output_bias: tensor(&mut checkpoint, "audio_tower.output_proj.bias", &[1536])?,
        })
    }

    /// Returns only soft tokens corresponding to valid input frames; never returns padded tokens.
    pub fn encode_features(
        &self,
        features: &[f32],
        frame_count: usize,
        valid_frames: usize,
    ) -> Result<Vec<Vec<f32>>> {
        if frame_count == 0
            || frame_count > MAX_FRAMES
            || valid_frames == 0
            || valid_frames > frame_count
            || features.len() != frame_count * BANDS
        {
            return Err(GemmaAudioError::Features);
        }
        if features.iter().any(|v| !v.is_finite()) {
            return Err(GemmaAudioError::NonFinite);
        }
        let (first, rows, bands) = self.conv0.run(features, frame_count, BANDS, valid_frames);
        let (second, rows, bands) = self
            .conv1
            .run(&first, rows, bands, valid_frames.div_ceil(2));
        let mut tokens = (0..rows)
            .map(|i| {
                let input = &second[i * bands * 32..(i + 1) * bands * 32];
                self.input_projection
                    .as_chunks::<WIDTH>()
                    .0
                    .iter()
                    .map(|w| dot_clipped(w, input, f32::NEG_INFINITY, f32::INFINITY))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let valid = valid_frames.div_ceil(4);
        let positions = (0..=CHUNK)
            .rev()
            .map(|position| {
                let mut embedding = Vec::with_capacity(WIDTH);
                for sine in [true, false] {
                    for i in 0..WIDTH / 2 {
                        let angle = position as f32
                            * (-((10000.0_f32).ln()) * i as f32 / (WIDTH / 2 - 1) as f32).exp();
                        embedding.push(if sine { angle.sin() } else { angle.cos() });
                    }
                }
                embedding
            })
            .collect::<Vec<_>>();
        for layer in &self.layers {
            layer.run(&mut tokens, valid, &positions);
        }
        let result = tokens
            .into_iter()
            .take(valid)
            .map(|token| {
                self.output_projection
                    .as_chunks::<WIDTH>()
                    .0
                    .iter()
                    .zip(&self.output_bias)
                    .map(|(row, bias)| {
                        dot_clipped(row, &token, f32::NEG_INFINITY, f32::INFINITY) + bias
                    })
                    .collect::<Vec<f32>>()
            })
            .collect::<Vec<_>>();
        if result.iter().flatten().any(|v| !v.is_finite()) {
            return Err(GemmaAudioError::NonFinite);
        }
        Ok(result)
    }
}
