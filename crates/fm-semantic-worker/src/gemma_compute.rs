use lattice_inference::forward::cpu::matmul_bt;

#[derive(Clone, Default)]
pub(crate) enum GemmaCompute {
    #[default]
    Cpu,
    #[cfg(all(target_os = "macos", feature = "gemma-metal"))]
    Metal(std::sync::Arc<std::sync::atomic::AtomicUsize>),
}

impl GemmaCompute {
    #[cfg(all(target_os = "macos", feature = "gemma-metal"))]
    pub(crate) fn metal() -> Option<Self> {
        lattice_inference::forward::metal_gemm::is_available()
            .then(|| Self::Metal(Default::default()))
    }

    pub(crate) fn dispatches(&self) -> usize {
        match self {
            Self::Cpu => 0,
            #[cfg(all(target_os = "macos", feature = "gemma-metal"))]
            Self::Metal(count) => count.load(std::sync::atomic::Ordering::Relaxed),
        }
    }

    pub(crate) fn matmul_bt(
        &self,
        input: &[f32],
        weights: &[f32],
        result: &mut [f32],
        rows: usize,
        cols: usize,
        output: usize,
    ) {
        #[cfg(all(target_os = "macos", feature = "gemma-metal"))]
        if let Self::Metal(count) = self
            && lattice_inference::forward::metal_gemm::metal_matmul_bt(
                input, weights, result, rows, cols, output,
            )
        {
            count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            return;
        }

        matmul_bt(input, weights, result, rows, cols, output);
    }
}

#[cfg(test)]
mod tests {
    use super::GemmaCompute;

    #[test]
    fn cpu_is_the_default_backend() {
        let compute = GemmaCompute::default();
        let mut output = [0.0; 2];
        compute.matmul_bt(&[1.0, 2.0], &[3.0, 4.0, 5.0, 6.0], &mut output, 1, 2, 2);
        assert_eq!(output, [11.0, 17.0]);
        assert_eq!(compute.dispatches(), 0);
    }

    #[cfg(all(target_os = "macos", feature = "gemma-metal"))]
    #[test]
    fn explicit_metal_backend_dispatches_fp32_gemm() {
        let compute = GemmaCompute::metal().expect("Apple Metal device");
        let input = vec![0.25; 64 * 64];
        let weights = vec![0.5; 64 * 64];
        let mut output = vec![0.0; 64 * 64];
        compute.matmul_bt(&input, &weights, &mut output, 64, 64, 64);
        assert!(output.iter().all(|value| (*value - 8.0).abs() < 1e-5));
        assert_eq!(compute.dispatches(), 1);
    }
}
