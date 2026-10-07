//! Offline FP32 Gemma 4 vision encoder for already patchified images and video frames.

use std::{fs::File, path::Path};

use lattice_inference::{
    forward::cpu::{matmul_bt, rms_norm},
    weights::{SafetensorsFile, TensorSource},
};
use tokio_util::sync::CancellationToken;

const WIDTH: usize = 768;
const HEAD: usize = 64;
const HEADS: usize = 12;
const INTERMEDIATE: usize = 3072;
const PATCH_PIXELS: usize = 16 * 16 * 3;
const MAX_PATCHES: usize = 2520;
const POSITION_SIZE: usize = 10240;
const LAYERS: usize = 16;
const EPS: f32 = 1e-6;

/// Invalid checkpoint, input patches, or vision output.
#[derive(Debug, thiserror::Error)]
pub enum GemmaVisionError {
    /// The owning job no longer permits inference.
    #[error("Gemma vision inference cancelled")]
    Cancelled,
    /// Local checkpoint or configuration could not be read.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// Checkpoint configuration is not valid JSON.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// Checkpoint tensor loading failed.
    #[error(transparent)]
    Checkpoint(#[from] lattice_inference::InferenceError),
    /// The checkpoint vision architecture differs from the supported one.
    #[error("unsupported Gemma 4 vision configuration")]
    Configuration,
    /// FP16 or another unsafe weight dtype was supplied.
    #[error("unsupported vision tensor {0} dtype: {1:?}; expected BF16 or F32")]
    Dtype(String, Option<String>),
    /// The tensor does not have the expected shape.
    #[error("vision tensor {0} has unexpected shape {1:?}")]
    Shape(String, Vec<usize>),
    /// Patch layout, positions, or output is invalid.
    #[error("invalid vision patches or positions: {0}")]
    Input(&'static str),
}

struct Layer {
    input_norm: Vec<f32>,
    post_attn_norm: Vec<f32>,
    pre_ff_norm: Vec<f32>,
    post_ff_norm: Vec<f32>,
    q_norm: Vec<f32>,
    k_norm: Vec<f32>,
    q: Vec<f32>,
    k: Vec<f32>,
    v: Vec<f32>,
    o: Vec<f32>,
    gate: Vec<f32>,
    up: Vec<f32>,
    down: Vec<f32>,
}

/// Holds only vision weights; the caller supplies patches and valid positions.
pub struct GemmaVisionTower {
    patch_projection: Vec<f32>,
    position_table: Vec<f32>,
    layers: Vec<Layer>,
}

fn tensor(
    checkpoint: &mut SafetensorsFile,
    name: &str,
    shape: &[usize],
) -> Result<Vec<f32>, GemmaVisionError> {
    let dtype = checkpoint.tensor_dtype(name)?;
    if !matches!(dtype.as_deref(), Some("BF16" | "F32")) {
        return Err(GemmaVisionError::Dtype(name.to_owned(), dtype));
    }
    let actual_shape = SafetensorsFile::tensor_shape(checkpoint, name).unwrap_or_default();
    if actual_shape != shape {
        return Err(GemmaVisionError::Shape(
            name.to_owned(),
            actual_shape.to_vec(),
        ));
    }
    let (values, _) = checkpoint.get_f32_tensor_owned(name)?;
    if values.iter().any(|v| !v.is_finite()) {
        return Err(GemmaVisionError::Input("non-finite checkpoint weight"));
    }
    Ok(values)
}

impl GemmaVisionTower {
    /// Load the pinned architecture from a locally verified checkpoint directory.
    pub fn open(directory: &Path) -> Result<Self, GemmaVisionError> {
        Self::open_files(
            &directory.join("config.json"),
            &directory.join("model.safetensors"),
        )
    }

    /// Load the original config and vision weights from independent verified paths.
    pub fn open_files(config_path: &Path, weights_path: &Path) -> Result<Self, GemmaVisionError> {
        let config: serde_json::Value = serde_json::from_reader(File::open(config_path)?)?;
        let c = &config["vision_config"];
        if config["model_type"] != "embedding_gemma2"
            || c["model_type"] != "gemma4_vision"
            || c["hidden_size"] != WIDTH
            || c["intermediate_size"] != INTERMEDIATE
            || c["num_hidden_layers"] != LAYERS
            || c["num_attention_heads"] != HEADS
            || c["num_key_value_heads"] != HEADS
            || c["head_dim"] != HEAD
            || c["patch_size"] != 16
            || c["pooling_kernel_size"] != 3
            || c["position_embedding_size"] != POSITION_SIZE
            || c["rms_norm_eps"] != EPS
            || c["hidden_activation"] != "gelu_pytorch_tanh"
            || c["rope_parameters"]["rope_type"] != "axial"
            || c["rope_parameters"]["rope_theta"] != 100.0
            || c["standardize"] != false
            || c["use_clipped_linears"] != false
            || c["attention_bias"] != false
        {
            return Err(GemmaVisionError::Configuration);
        }
        let mut ckpt = SafetensorsFile::open(weights_path)?;
        let prefix = "vision_tower.";
        let patch_projection = tensor(
            &mut ckpt,
            &format!("{prefix}patch_embedder.input_proj.weight"),
            &[WIDTH, PATCH_PIXELS],
        )?;
        let position_table = tensor(
            &mut ckpt,
            &format!("{prefix}patch_embedder.position_embedding_table"),
            &[2, POSITION_SIZE, WIDTH],
        )?;
        let mut layers = Vec::with_capacity(LAYERS);
        for i in 0..LAYERS {
            let base = format!("{prefix}encoder.layers.{i}.");
            let norm = |ckpt: &mut SafetensorsFile, name: &str, n| {
                tensor(ckpt, &format!("{base}{name}.weight"), &[n])
            };
            let linear = |ckpt: &mut SafetensorsFile, name: &str, rows, cols| {
                tensor(ckpt, &format!("{base}{name}.linear.weight"), &[rows, cols])
            };
            layers.push(Layer {
                input_norm: norm(&mut ckpt, "input_layernorm", WIDTH)?,
                post_attn_norm: norm(&mut ckpt, "post_attention_layernorm", WIDTH)?,
                pre_ff_norm: norm(&mut ckpt, "pre_feedforward_layernorm", WIDTH)?,
                post_ff_norm: norm(&mut ckpt, "post_feedforward_layernorm", WIDTH)?,
                q_norm: norm(&mut ckpt, "self_attn.q_norm", HEAD)?,
                k_norm: norm(&mut ckpt, "self_attn.k_norm", HEAD)?,
                q: linear(&mut ckpt, "self_attn.q_proj", WIDTH, WIDTH)?,
                k: linear(&mut ckpt, "self_attn.k_proj", WIDTH, WIDTH)?,
                v: linear(&mut ckpt, "self_attn.v_proj", WIDTH, WIDTH)?,
                o: linear(&mut ckpt, "self_attn.o_proj", WIDTH, WIDTH)?,
                gate: linear(&mut ckpt, "mlp.gate_proj", INTERMEDIATE, WIDTH)?,
                up: linear(&mut ckpt, "mlp.up_proj", INTERMEDIATE, WIDTH)?,
                down: linear(&mut ckpt, "mlp.down_proj", WIDTH, INTERMEDIATE)?,
            });
        }
        Ok(Self {
            patch_projection,
            position_table,
            layers,
        })
    }

    /// Encode one frame to unprojected 768-wide soft tokens, in spatial order.
    ///
    /// `patches` are RGB patches in HWC order, normalized to [0, 1] by the caller;
    /// valid positions form a complete rectangular grid with dimensions divisible
    /// by three. Padding, if present, is a suffix of `[-1, -1]` positions.
    pub fn encode_patches(
        &self,
        patches: &[f32],
        positions: &[[i32; 2]],
        valid_patches: usize,
    ) -> Result<Vec<Vec<f32>>, GemmaVisionError> {
        self.encode_patches_cancellable(
            patches,
            positions,
            valid_patches,
            &CancellationToken::new(),
        )
    }

    /// Encodes a frame while honoring the owning job's cancellation.
    pub fn encode_patches_cancellable(
        &self,
        patches: &[f32],
        positions: &[[i32; 2]],
        valid_patches: usize,
        cancellation: &CancellationToken,
    ) -> Result<Vec<Vec<f32>>, GemmaVisionError> {
        if cancellation.is_cancelled() {
            return Err(GemmaVisionError::Cancelled);
        }
        let n = positions.len();
        if n == 0
            || n > MAX_PATCHES
            || !n.is_multiple_of(9)
            || patches.len() != n * PATCH_PIXELS
            || valid_patches == 0
            || valid_patches > n
            || !valid_patches.is_multiple_of(9)
        {
            return Err(GemmaVisionError::Input("patch count or width"));
        }
        if patches
            .iter()
            .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        {
            return Err(GemmaVisionError::Input("patch pixel range"));
        }
        let grid_width = positions[valid_patches - 1][0] as usize + 1;
        if grid_width == 0
            || !grid_width.is_multiple_of(3)
            || !(valid_patches / grid_width).is_multiple_of(3)
            || !valid_patches.is_multiple_of(grid_width)
            || positions.iter().enumerate().any(|(idx, &pos)| {
                if idx < valid_patches {
                    pos != [(idx % grid_width) as i32, (idx / grid_width) as i32]
                } else {
                    pos != [-1, -1]
                }
            })
            || grid_width > POSITION_SIZE
            || valid_patches / grid_width > POSITION_SIZE
        {
            return Err(GemmaVisionError::Input("non-rectangular patch positions"));
        }
        // The attention mask excludes padded keys. Valid queries therefore
        // cannot depend on padding; skip padded rows in every dense layer.
        let n = valid_patches;
        let scaled: Vec<f32> = patches[..n * PATCH_PIXELS]
            .iter()
            .map(|&v| 2.0 * (v - 0.5))
            .collect();
        let mut hidden = linear(&scaled, &self.patch_projection, n, PATCH_PIXELS, WIDTH);
        for (i, pos) in positions.iter().take(valid_patches).enumerate() {
            for d in 0..WIDTH {
                hidden[i * WIDTH + d] += self.position_table[pos[0] as usize * WIDTH + d]
                    + self.position_table[(POSITION_SIZE + pos[1] as usize) * WIDTH + d];
            }
        }
        for layer in &self.layers {
            if cancellation.is_cancelled() {
                return Err(GemmaVisionError::Cancelled);
            }
            let mut x = hidden.clone();
            rms_norm(&mut x, &layer.input_norm, WIDTH, EPS);
            let mut q = linear(&x, &layer.q, n, WIDTH, WIDTH);
            let mut k = linear(&x, &layer.k, n, WIDTH, WIDTH);
            let mut v = linear(&x, &layer.v, n, WIDTH, WIDTH);
            for (t, &position) in positions.iter().take(n).enumerate() {
                for h in 0..HEADS {
                    let start = t * WIDTH + h * HEAD;
                    rms_norm(&mut q[start..start + HEAD], &layer.q_norm, HEAD, EPS);
                    rms_norm(&mut k[start..start + HEAD], &layer.k_norm, HEAD, EPS);
                    let variance =
                        v[start..start + HEAD].iter().map(|a| a * a).sum::<f32>() / HEAD as f32;
                    let scale = (variance + EPS).sqrt().recip();
                    for a in &mut v[start..start + HEAD] {
                        *a *= scale;
                    }
                    rotary(&mut q[start..start + HEAD], position);
                    rotary(&mut k[start..start + HEAD], position);
                }
            }
            let mut context = vec![0.0; n * WIDTH];
            for h in 0..HEADS {
                for t in 0..valid_patches {
                    if cancellation.is_cancelled() {
                        return Err(GemmaVisionError::Cancelled);
                    }
                    let qt = &q[t * WIDTH + h * HEAD..t * WIDTH + (h + 1) * HEAD];
                    let mut logits = Vec::with_capacity(valid_patches);
                    for j in 0..valid_patches {
                        let kj = &k[j * WIDTH + h * HEAD..j * WIDTH + (h + 1) * HEAD];
                        logits.push(qt.iter().zip(kj).map(|(a, b)| a * b).sum::<f32>());
                    }
                    let max = logits.iter().copied().fold(f32::NEG_INFINITY, f32::max);
                    for score in &mut logits {
                        *score = (*score - max).exp();
                    }
                    let total: f32 = logits.iter().sum();
                    let out = &mut context[t * WIDTH + h * HEAD..t * WIDTH + (h + 1) * HEAD];
                    for (j, score) in logits.iter().enumerate() {
                        let value = &v[j * WIDTH + h * HEAD..j * WIDTH + (h + 1) * HEAD];
                        for d in 0..HEAD {
                            out[d] += (score / total) * value[d];
                        }
                    }
                }
            }
            let mut attention = linear(&context, &layer.o, n, WIDTH, WIDTH);
            rms_norm(&mut attention, &layer.post_attn_norm, WIDTH, EPS);
            for (dst, update) in hidden.iter_mut().zip(attention) {
                *dst += update;
            }
            let mut x = hidden.clone();
            rms_norm(&mut x, &layer.pre_ff_norm, WIDTH, EPS);
            let mut gate = linear(&x, &layer.gate, n, WIDTH, INTERMEDIATE);
            let up = linear(&x, &layer.up, n, WIDTH, INTERMEDIATE);
            for (g, u) in gate.iter_mut().zip(up) {
                let z = *g;
                *g = 0.5 * z * (1.0 + (0.797_884_6 * (z + 0.044_715 * z.powi(3))).tanh()) * u;
            }
            let mut ff = linear(&gate, &layer.down, n, INTERMEDIATE, WIDTH);
            rms_norm(&mut ff, &layer.post_ff_norm, WIDTH, EPS);
            for (dst, update) in hidden.iter_mut().zip(ff) {
                *dst += update;
            }
        }
        if cancellation.is_cancelled() {
            return Err(GemmaVisionError::Cancelled);
        }
        let pool_width = grid_width / 3;
        let mut pooled = vec![vec![0.0; WIDTH]; valid_patches / 9];
        for (i, row) in hidden
            .as_chunks::<WIDTH>()
            .0
            .iter()
            .take(valid_patches)
            .enumerate()
        {
            let index = (i / grid_width / 3) * pool_width + (i % grid_width / 3);
            for (dst, value) in pooled[index].iter_mut().zip(row) {
                *dst += value / 9.0;
            }
        }
        let scale = (WIDTH as f32).sqrt();
        for row in &mut pooled {
            for v in row {
                *v *= scale;
                if !v.is_finite() {
                    return Err(GemmaVisionError::Input("non-finite output"));
                }
            }
        }
        Ok(pooled)
    }
}

fn linear(input: &[f32], weights: &[f32], rows: usize, cols: usize, output: usize) -> Vec<f32> {
    let mut result = vec![0.0; rows * output];
    matmul_bt(input, weights, &mut result, rows, cols, output);
    result
}

fn rotary(vector: &mut [f32], position: [i32; 2]) {
    for (axis, coordinate) in position.iter().enumerate() {
        for i in 0..16 {
            let angle = *coordinate as f32 / 100.0_f32.powf(i as f32 / 16.0);
            let (sin, cos) = angle.sin_cos();
            let i = axis * 32 + i;
            let j = i + 16;
            let a = vector[i];
            let b = vector[j];
            vector[i] = a * cos - b * sin;
            vector[j] = b * cos + a * sin;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fp16_weights_are_rejected() {
        let header = serde_json::json!({
            "vision_tower.test": {
                "dtype": "F16",
                "shape": [1],
                "data_offsets": [0, 2],
            }
        })
        .to_string();
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&(header.len() as u64).to_le_bytes());
        bytes.extend_from_slice(header.as_bytes());
        bytes.extend_from_slice(&[0, 0]);
        let mut checkpoint = SafetensorsFile::from_bytes(bytes).expect("synthetic checkpoint");
        assert!(matches!(
            tensor(&mut checkpoint, "vision_tower.test", &[1]),
            Err(GemmaVisionError::Dtype(_, Some(dtype))) if dtype == "F16"
        ));
    }

    #[test]
    fn invalid_patches_and_coordinates_are_rejected() {
        let tower = GemmaVisionTower {
            patch_projection: Vec::new(),
            position_table: Vec::new(),
            layers: Vec::new(),
        };
        assert!(matches!(
            tower.encode_patches(&[], &[], 0),
            Err(GemmaVisionError::Input(_))
        ));
        let positions: Vec<_> = (0..3).flat_map(|y| (0..3).map(move |x| [x, y])).collect();
        let pixels = vec![0.5; 9 * PATCH_PIXELS];
        assert!(matches!(
            tower.encode_patches(&pixels, &positions, 10),
            Err(GemmaVisionError::Input(_))
        ));
        let mut bad = positions.clone();
        bad[3] = [3, 0];
        assert!(matches!(
            tower.encode_patches(&pixels, &bad, 9),
            Err(GemmaVisionError::Input(_))
        ));
        let mut bad_pixels = pixels;
        bad_pixels[0] = f32::NAN;
        assert!(matches!(
            tower.encode_patches(&bad_pixels, &positions, 9),
            Err(GemmaVisionError::Input(_))
        ));
    }
}
