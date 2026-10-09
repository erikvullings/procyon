//! Experimental offline CPU text probe; not an installed semantic model.

use std::path::Path;

use lattice_inference::model::embeddinggemma2::{
    EmbeddingGemma2Model, EmbeddingGemma2Task, format_titled_document,
};

/// Retrieval intent determines the upstream model's exact text instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GemmaTextTask {
    /// Ordinary semantic search.
    Search,
    /// Grounded question answering.
    Question,
    /// Natural-language code search.
    Code,
    /// Indexed document or code passage.
    Document,
}

impl GemmaTextTask {
    /// Formats text as the model expects. Only documents may have titles.
    pub fn format(self, text: &str, title: Option<&str>) -> Result<String, GemmaProbeError> {
        if title.is_some() && self != Self::Document {
            return Err(GemmaProbeError::TitleOnQuery);
        }
        Ok(match (self, title) {
            (Self::Search, _) => EmbeddingGemma2Task::Query.format(text),
            (Self::Question, _) => EmbeddingGemma2Task::QuestionAnswering.format(text),
            (Self::Code, _) => EmbeddingGemma2Task::CodeRetrieval.format(text),
            (Self::Document, Some(title)) => format_titled_document(title, text),
            (Self::Document, None) => EmbeddingGemma2Task::Document.format(text),
        })
    }
}

/// Runs the upstream EmbeddingGemma 2 text tower on a local checkpoint only.
pub struct GemmaTextProbe {
    model: EmbeddingGemma2Model,
    dimensions: usize,
}

impl GemmaTextProbe {
    /// Opens locally supplied weights without any model download.
    pub fn open(directory: &Path, dimensions: usize) -> Result<Self, GemmaProbeError> {
        if ![128, 256, 512, 768].contains(&dimensions) {
            return Err(GemmaProbeError::UnsupportedDimensions(dimensions));
        }
        let model = EmbeddingGemma2Model::from_model_dir(directory)?;
        if model.dimensions() != 768 {
            return Err(GemmaProbeError::UnexpectedModelDimensions(
                model.dimensions(),
            ));
        }
        Ok(Self { model, dimensions })
    }

    /// Computes a unit-length vector using the model's text prompt and MRL width.
    pub fn encode(
        &self,
        task: GemmaTextTask,
        text: &str,
        title: Option<&str>,
    ) -> Result<Vec<f32>, GemmaProbeError> {
        let formatted = task.format(text, title)?;
        let vector = self.model.encode(&formatted, Some(self.dimensions))?;
        if vector.len() != self.dimensions || vector.iter().any(|value| !value.is_finite()) {
            return Err(GemmaProbeError::InvalidOutput);
        }
        Ok(vector)
    }
}

/// Rejected probe input.
#[derive(Debug, thiserror::Error)]
pub enum GemmaProbeError {
    /// A title was supplied for a query.
    #[error("titles are only valid for indexed documents")]
    TitleOnQuery,
    /// Unsupported Matryoshka output width.
    #[error("unsupported embedding dimension: {0}")]
    UnsupportedDimensions(usize),
    /// Loaded a checkpoint with the wrong vector width.
    #[error("expected a 768-dimensional EmbeddingGemma 2 checkpoint, got {0}")]
    UnexpectedModelDimensions(usize),
    /// Inference produced a non-finite or incorrectly sized vector.
    #[error("model produced an invalid embedding")]
    InvalidOutput,
    /// Model loading or inference failed.
    #[error(transparent)]
    Inference(#[from] lattice_inference::InferenceError),
}

#[cfg(test)]
mod tests {
    use super::{GemmaProbeError, GemmaTextProbe, GemmaTextTask};

    #[test]
    fn document_with_title_uses_model_document_format() {
        assert_eq!(
            GemmaTextTask::Document
                .format("the body", Some("Example"))
                .expect("document"),
            "title: Example | text: the body"
        );
    }

    #[test]
    fn unsupported_dimension_is_rejected_before_loading_weights() {
        assert!(matches!(
            GemmaTextProbe::open("/nonexistent".as_ref(), 384),
            Err(GemmaProbeError::UnsupportedDimensions(384))
        ));
    }

    #[test]
    fn query_intents_use_distinct_model_instructions() {
        assert_eq!(
            [
                GemmaTextTask::Search,
                GemmaTextTask::Question,
                GemmaTextTask::Code,
                GemmaTextTask::Document,
            ]
            .map(|task| task.format("leaking tap", None).expect("prompt")),
            [
                "task: search result | query: leaking tap",
                "task: question answering | query: leaking tap",
                "task: code retrieval | query: leaking tap",
                "title: none | text: leaking tap",
            ]
        );
        assert!(matches!(
            GemmaTextTask::Search.format("leaking tap", Some("title")),
            Err(GemmaProbeError::TitleOnQuery)
        ));
    }
}
