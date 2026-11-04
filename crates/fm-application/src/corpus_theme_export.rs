//! Renders discovered corpus themes into shareable files (task 0298).
//!
//! Everything here is pure: it maps an already authorized [`CorpusThemesDto`] onto one
//! intermediate export model and renders that model as Markdown, Word, or SKOS (Turtle, RDF/XML,
//! JSON-LD). Writing the bytes is the caller's job, through the VFS provider.
//!
//! Themes are statistical clusters, not a curated vocabulary. Every format says so, and the
//! default output carries no document text: passages and document titles are opt-in.

use std::fmt::Write as _;

use fm_transport_dto::{
    CorpusThemeDto, CorpusThemeExportFormatDto, CorpusThemesDto, DocumentThemeMixtureDto,
    ThemeExportLabelDto,
};
use uuid::Uuid;

use crate::ApplicationError;

/// Most documents listed under one theme.
const MAX_DOCUMENTS_PER_THEME: usize = 25;
/// Smallest share of a document's passages for it to be listed under a theme.
const MIN_DOCUMENT_SHARE: f32 = 0.05;
/// Most keywords exported per theme.
const MAX_KEYWORDS: usize = 12;
/// Most characters of one passage excerpt.
const MAX_EXCERPT_CHARS: usize = 600;
/// Keywords used for a fallback label.
const LABEL_KEYWORDS: usize = 3;

const GENERATED_NOTE: &str = "Automatically generated from statistical clusters of indexed \
    passages. These are not curated concepts; review them before relying on them.";

/// What to put in an export, beyond the always-present label, keywords and description.
#[derive(Debug, Clone)]
pub struct ExportOptions {
    /// Include example passages (document text).
    pub include_passages: bool,
    /// Include the titles of the documents belonging to each theme.
    pub include_documents: bool,
    /// Base URI for SKOS identifiers; `None` uses deterministic `urn:uuid:` identifiers.
    pub base_uri: Option<String>,
    /// Human label of the analysed scope, for the header.
    pub scope_label: String,
    /// Generation date, `YYYY-MM-DD`.
    pub generated_on: String,
}

/// One exported file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedExport {
    /// The file contents.
    pub bytes: Vec<u8>,
    /// Number of themes in the file.
    pub theme_count: u32,
}

#[derive(Debug, Clone, PartialEq)]
struct ExportPassage {
    title: String,
    section: String,
    excerpt: String,
}

#[derive(Debug, Clone, PartialEq)]
struct ExportDocument {
    title: String,
    share: f32,
}

#[derive(Debug, Clone, PartialEq)]
struct ExportTheme {
    uri: String,
    number: u32,
    label: String,
    description: String,
    keywords: Vec<String>,
    documents: Vec<ExportDocument>,
    passages: Vec<ExportPassage>,
}

#[derive(Debug, Clone, PartialEq)]
struct ExportModel {
    scheme_uri: String,
    scope_label: String,
    generated_on: String,
    chunk_count: u64,
    document_count: u32,
    include_passages: bool,
    include_documents: bool,
    themes: Vec<ExportTheme>,
}

fn invalid(message: impl Into<String>) -> ApplicationError {
    ApplicationError::InvalidRequest(message.into())
}

/// Renders `themes` (restricted to `selected` ids, or all when empty) in `format`.
///
/// # Errors
///
/// Returns an invalid-request error for an unknown theme id, an empty selection, or a malformed
/// base URI, and an error if the Word converter fails.
pub fn render(
    format: CorpusThemeExportFormatDto,
    themes: &CorpusThemesDto,
    selected: &[u32],
    labels: &[ThemeExportLabelDto],
    options: &ExportOptions,
) -> Result<RenderedExport, ApplicationError> {
    let model = build_model(themes, selected, labels, options)?;
    let bytes = match format {
        CorpusThemeExportFormatDto::Markdown => markdown(&model).into_bytes(),
        CorpusThemeExportFormatDto::Docx => docx(&model)?,
        CorpusThemeExportFormatDto::SkosTurtle => turtle(&model).into_bytes(),
        CorpusThemeExportFormatDto::SkosRdfXml => rdf_xml(&model).into_bytes(),
        CorpusThemeExportFormatDto::SkosJsonLd => json_ld(&model)?,
    };
    Ok(RenderedExport {
        bytes,
        theme_count: u32::try_from(model.themes.len()).unwrap_or(u32::MAX),
    })
}

fn build_model(
    themes: &CorpusThemesDto,
    selected: &[u32],
    labels: &[ThemeExportLabelDto],
    options: &ExportOptions,
) -> Result<ExportModel, ApplicationError> {
    let base = normalized_base(options.base_uri.as_deref())?;
    for id in selected {
        if !themes.themes.iter().any(|theme| theme.id == *id) {
            return Err(invalid(format!("unknown theme {id}")));
        }
    }
    let chosen: Vec<&CorpusThemeDto> = themes
        .themes
        .iter()
        .filter(|theme| selected.is_empty() || selected.contains(&theme.id))
        .collect();
    if chosen.is_empty() {
        return Err(invalid("there are no themes to export"));
    }
    let scheme_uri = match &base {
        Some(base) => format!("{base}scheme"),
        None => urn(&themes.fingerprint, "scheme"),
    };
    let exported = chosen
        .into_iter()
        .map(|theme| {
            let keywords: Vec<String> = theme
                .keywords
                .iter()
                .map(|keyword| clean_text(&keyword.term))
                .filter(|term| usable_keyword(term))
                .take(MAX_KEYWORDS)
                .collect();
            let label = labels
                .iter()
                .find(|label| label.theme == theme.id)
                .map(|label| clean_text(&label.label))
                .filter(|label| !label.is_empty())
                .unwrap_or_else(|| fallback_label(&keywords, theme.id));
            ExportTheme {
                uri: match &base {
                    Some(base) => format!("{base}theme-{}", theme.id),
                    None => urn(&themes.fingerprint, &theme.id.to_string()),
                },
                number: theme.id.saturating_add(1),
                description: description(theme, &keywords),
                documents: if options.include_documents {
                    documents_for(&themes.document_mixtures, theme.id)
                } else {
                    Vec::new()
                },
                passages: if options.include_passages {
                    theme
                        .passages
                        .iter()
                        .map(|passage| ExportPassage {
                            title: clean_title(&passage.title),
                            section: passage
                                .section_path
                                .iter()
                                .map(|part| clean_text(part))
                                .collect::<Vec<_>>()
                                .join(" › "),
                            excerpt: excerpt(&passage.excerpt),
                        })
                        .collect()
                } else {
                    Vec::new()
                },
                label,
                keywords,
            }
        })
        .collect();
    Ok(ExportModel {
        scheme_uri,
        scope_label: clean_text(&options.scope_label),
        generated_on: options.generated_on.clone(),
        chunk_count: themes.chunk_count,
        document_count: themes.document_count,
        include_passages: options.include_passages,
        include_documents: options.include_documents,
        themes: exported,
    })
}

/// A base URI normalised to end with `/` or `#`, or an error if it is not an absolute URI.
fn normalized_base(base: Option<&str>) -> Result<Option<String>, ApplicationError> {
    let Some(raw) = base.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let scheme_end = raw
        .find(':')
        .ok_or_else(|| invalid("the base URI must be absolute"))?;
    let scheme = &raw[..scheme_end];
    let valid_scheme = scheme.len() > 1
        && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    let forbidden = |c: char| c.is_whitespace() || c.is_control() || "<>\"{}|\\^`".contains(c);
    if !valid_scheme || raw.len() == scheme_end + 1 || raw.chars().any(forbidden) {
        return Err(invalid("the base URI is not a valid absolute URI"));
    }
    Ok(Some(if raw.ends_with(['/', '#']) {
        raw.to_owned()
    } else {
        format!("{raw}/")
    }))
}

/// A deterministic `urn:uuid:` for one exported resource: stable across exports of one analysis.
fn urn(fingerprint: &str, name: &str) -> String {
    let uuid = Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("urn:procyon:themes:{fingerprint}:{name}").as_bytes(),
    );
    format!("urn:uuid:{uuid}")
}

fn usable_keyword(term: &str) -> bool {
    term.chars().count() >= 3 && !term.chars().all(|c| c.is_ascii_digit())
}

fn fallback_label(keywords: &[String], id: u32) -> String {
    if keywords.is_empty() {
        format!("Theme {}", id.saturating_add(1))
    } else {
        keywords
            .iter()
            .take(LABEL_KEYWORDS)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn description(theme: &CorpusThemeDto, keywords: &[String]) -> String {
    let mut text = format!(
        "{} passages in {} documents ({} of the analysed passages)",
        theme.chunk_count,
        theme.document_count,
        percent(theme.share),
    );
    if !keywords.is_empty() {
        let leading = keywords
            .iter()
            .take(6)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        let _ = write!(text, "; leading terms: {leading}");
    }
    text.push('.');
    text
}

fn percent(share: f32) -> String {
    let value = share * 100.0;
    if value > 0.0 && value < 1.0 {
        "<1%".to_owned()
    } else {
        format!("{value:.0}%")
    }
}

fn documents_for(mixtures: &[DocumentThemeMixtureDto], theme: u32) -> Vec<ExportDocument> {
    let mut rows: Vec<(u32, f32, String, String)> = mixtures
        .iter()
        .filter_map(|mixture| {
            let slice = mixture
                .shares
                .iter()
                .find(|share| share.theme == Some(theme))?;
            (slice.share >= MIN_DOCUMENT_SHARE).then(|| {
                (
                    slice.chunk_count,
                    slice.share,
                    clean_title(&mixture.title),
                    mixture.source_id.clone(),
                )
            })
        })
        .collect();
    // Realistic input is unsorted and interleaved across themes: order is fully determined here.
    rows.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then(right.1.total_cmp(&left.1))
            .then(left.2.cmp(&right.2))
            .then(left.3.cmp(&right.3))
    });
    rows.into_iter()
        .take(MAX_DOCUMENTS_PER_THEME)
        .map(|(_, share, title, _)| ExportDocument { title, share })
        .collect()
}

/// Writes `bytes` to `destination` through the provider's audited `WRITE` path.
///
/// # Errors
///
/// Returns an error if the provider cannot be resolved, lacks `WRITE`, the file exists and
/// `overwrite` is false, or the write fails.
pub(crate) async fn write_export(
    providers: &fm_vfs::ProviderRegistry,
    destination: &fm_domain::Location,
    overwrite: bool,
    bytes: &[u8],
) -> Result<(), ApplicationError> {
    use tokio::io::AsyncWriteExt as _;

    let provider = providers
        .resolve(destination)
        .map_err(ApplicationError::from)?;
    provider
        .capabilities_for(destination)
        .map_err(ApplicationError::from)?
        .require(fm_vfs::ProviderCapabilities::WRITE)
        .map_err(ApplicationError::from)?;
    let mut writer = provider
        .open_write(
            destination,
            fm_vfs::WriteOptions { overwrite },
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .map_err(ApplicationError::from)?;
    writer
        .write_all(bytes)
        .await
        .map_err(|error| invalid(error.to_string()))?;
    writer
        .shutdown()
        .await
        .map_err(|error| invalid(error.to_string()))?;
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Text cleaning
// ---------------------------------------------------------------------------------------------

/// Decodes the HTML entities the indexer leaves in titles and passages.
fn decode_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        let decoded = tail.find(';').filter(|end| *end <= 10).and_then(|end| {
            let body = &tail[1..end];
            let character = match body {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some(' '),
                _ => body.strip_prefix('#').and_then(|number| {
                    let code = match number.strip_prefix(['x', 'X']) {
                        Some(hex) => u32::from_str_radix(hex, 16).ok(),
                        None => number.parse().ok(),
                    }?;
                    char::from_u32(code).filter(|c| *c != '\0')
                }),
            }?;
            Some((character, end))
        });
        match decoded {
            Some((character, end)) => {
                out.push(character);
                rest = &tail[end + 1..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Entity-decoded text with control characters dropped and whitespace collapsed.
fn clean_text(text: &str) -> String {
    decode_entities(text)
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// File names arrive percent-encoded and entity-escaped.
fn clean_title(title: &str) -> String {
    let decoded = percent_encoding::percent_decode_str(title)
        .decode_utf8()
        .map_or_else(|_| title.to_owned(), std::borrow::Cow::into_owned);
    clean_text(&decoded)
}

fn excerpt(text: &str) -> String {
    let cleaned = clean_text(text);
    if cleaned.chars().count() <= MAX_EXCERPT_CHARS {
        return cleaned;
    }
    let mut cut: String = cleaned.chars().take(MAX_EXCERPT_CHARS).collect();
    cut.push('…');
    cut
}

// ---------------------------------------------------------------------------------------------
// Markdown and Word
// ---------------------------------------------------------------------------------------------

fn markdown_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        if matches!(
            character,
            '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '#' | '|'
        ) {
            out.push('\\');
        }
        out.push(character);
    }
    out
}

fn markdown(model: &ExportModel) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "# Corpus themes: {}\n",
        markdown_escape(&model.scope_label)
    );
    let _ = writeln!(
        out,
        "Generated {} · {} themes · {} passages analysed in {} documents\n",
        model.generated_on,
        model.themes.len(),
        model.chunk_count,
        model.document_count,
    );
    let _ = writeln!(out, "> {GENERATED_NOTE}\n");
    for theme in &model.themes {
        let _ = writeln!(
            out,
            "## {}. {}\n",
            theme.number,
            markdown_escape(&theme.label)
        );
        let _ = writeln!(out, "{}\n", markdown_escape(&theme.description));
        if !theme.keywords.is_empty() {
            let keywords = theme
                .keywords
                .iter()
                .map(|keyword| markdown_escape(keyword))
                .collect::<Vec<_>>()
                .join(", ");
            let _ = writeln!(out, "**Keywords:** {keywords}\n");
        }
        if !theme.documents.is_empty() {
            let _ = writeln!(out, "### Documents\n");
            for document in &theme.documents {
                let _ = writeln!(
                    out,
                    "- {} ({})",
                    markdown_escape(&document.title),
                    percent(document.share)
                );
            }
            out.push('\n');
        }
        if !theme.passages.is_empty() {
            let _ = writeln!(out, "### Example passages\n");
            for passage in &theme.passages {
                let _ = writeln!(out, "> {}\n>", markdown_escape(&passage.excerpt));
                let location = if passage.section.is_empty() {
                    markdown_escape(&passage.title)
                } else {
                    format!(
                        "{}, {}",
                        markdown_escape(&passage.title),
                        markdown_escape(&passage.section)
                    )
                };
                let _ = writeln!(out, "> — *{location}*\n");
            }
        }
    }
    out
}

fn html_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            other => out.push(other),
        }
    }
    out
}

fn html(model: &ExportModel) -> String {
    let mut out = String::from("<html><body>");
    let _ = write!(
        out,
        "<h1>Corpus themes: {}</h1>",
        html_escape(&model.scope_label)
    );
    let _ = write!(
        out,
        "<p>Generated {} · {} themes · {} passages analysed in {} documents</p>",
        html_escape(&model.generated_on),
        model.themes.len(),
        model.chunk_count,
        model.document_count,
    );
    let _ = write!(
        out,
        "<blockquote><p>{}</p></blockquote>",
        html_escape(GENERATED_NOTE)
    );
    for theme in &model.themes {
        let _ = write!(
            out,
            "<h2>{}. {}</h2><p>{}</p>",
            theme.number,
            html_escape(&theme.label),
            html_escape(&theme.description),
        );
        if !theme.keywords.is_empty() {
            let _ = write!(
                out,
                "<p><strong>Keywords:</strong> {}</p>",
                html_escape(&theme.keywords.join(", "))
            );
        }
        if !theme.documents.is_empty() {
            out.push_str("<h3>Documents</h3><ul>");
            for document in &theme.documents {
                let _ = write!(
                    out,
                    "<li>{} ({})</li>",
                    html_escape(&document.title),
                    percent(document.share)
                );
            }
            out.push_str("</ul>");
        }
        if !theme.passages.is_empty() {
            out.push_str("<h3>Example passages</h3>");
            for passage in &theme.passages {
                let location = if passage.section.is_empty() {
                    passage.title.clone()
                } else {
                    format!("{}, {}", passage.title, passage.section)
                };
                let _ = write!(
                    out,
                    "<blockquote><p>{}</p><p><em>— {}</em></p></blockquote>",
                    html_escape(&passage.excerpt),
                    html_escape(&location)
                );
            }
        }
    }
    out.push_str("</body></html>");
    out
}

fn docx(model: &ExportModel) -> Result<Vec<u8>, ApplicationError> {
    let document = ferrodoc::parse(html(model).as_bytes(), ferrodoc::Format::Html)
        .map_err(|error| invalid(format!("could not build the Word document: {error}")))?;
    ferrodoc::render(&document, ferrodoc::Format::Docx)
        .map_err(|error| invalid(format!("could not write the Word document: {error}")))
}

// ---------------------------------------------------------------------------------------------
// SKOS
// ---------------------------------------------------------------------------------------------

fn scheme_title(model: &ExportModel) -> String {
    format!("Corpus themes: {}", model.scope_label)
}

/// Literals of one concept, in a fixed order shared by every serialisation.
struct ConceptLiterals<'a> {
    pref_label: &'a str,
    alt_labels: &'a [String],
    definition: &'a str,
    examples: Vec<String>,
}

fn literals<'a>(theme: &'a ExportTheme, model: &ExportModel) -> ConceptLiterals<'a> {
    let mut examples = Vec::new();
    if model.include_passages {
        for passage in &theme.passages {
            examples.push(passage.excerpt.clone());
        }
    }
    ConceptLiterals {
        pref_label: &theme.label,
        alt_labels: &theme.keywords,
        definition: &theme.description,
        examples,
    }
}

fn turtle_literal(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for character in text.chars() {
        match character {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

fn turtle(model: &ExportModel) -> String {
    let mut out = String::from(
        "@prefix skos: <http://www.w3.org/2004/02/skos/core#> .\n\
         @prefix dct: <http://purl.org/dc/terms/> .\n\
         @prefix xsd: <http://www.w3.org/2001/XMLSchema#> .\n\n",
    );
    let top = model
        .themes
        .iter()
        .map(|theme| format!("<{}>", theme.uri))
        .collect::<Vec<_>>()
        .join(", ");
    let _ = write!(
        out,
        "<{}> a skos:ConceptScheme ;\n    dct:title {} ;\n    dct:created \"{}\"^^xsd:date ;\n    skos:note {} ;\n    skos:hasTopConcept {} .\n\n",
        model.scheme_uri,
        turtle_literal(&scheme_title(model)),
        model.generated_on,
        turtle_literal(GENERATED_NOTE),
        top,
    );
    for theme in &model.themes {
        let concept = literals(theme, model);
        let _ = write!(
            out,
            "<{}> a skos:Concept ;\n    skos:inScheme <{}> ;\n    skos:topConceptOf <{}> ;\n    skos:prefLabel {}",
            theme.uri,
            model.scheme_uri,
            model.scheme_uri,
            turtle_literal(concept.pref_label),
        );
        for alt in concept.alt_labels {
            let _ = write!(out, " ;\n    skos:altLabel {}", turtle_literal(alt));
        }
        let _ = write!(
            out,
            " ;\n    skos:definition {}",
            turtle_literal(concept.definition)
        );
        for example in &concept.examples {
            let _ = write!(out, " ;\n    skos:example {}", turtle_literal(example));
        }
        out.push_str(" .\n\n");
    }
    out
}

fn xml_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            other => out.push(other),
        }
    }
    out
}

fn rdf_xml(model: &ExportModel) -> String {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"\n\
         \x20        xmlns:skos=\"http://www.w3.org/2004/02/skos/core#\"\n\
         \x20        xmlns:dct=\"http://purl.org/dc/terms/\">\n",
    );
    let _ = writeln!(
        out,
        "  <skos:ConceptScheme rdf:about=\"{}\">\n    <dct:title>{}</dct:title>\n    <dct:created>{}</dct:created>\n    <skos:note>{}</skos:note>",
        xml_escape(&model.scheme_uri),
        xml_escape(&scheme_title(model)),
        xml_escape(&model.generated_on),
        xml_escape(GENERATED_NOTE),
    );
    for theme in &model.themes {
        let _ = writeln!(
            out,
            "    <skos:hasTopConcept rdf:resource=\"{}\"/>",
            xml_escape(&theme.uri)
        );
    }
    out.push_str("  </skos:ConceptScheme>\n");
    for theme in &model.themes {
        let concept = literals(theme, model);
        let _ = writeln!(
            out,
            "  <skos:Concept rdf:about=\"{}\">\n    <skos:inScheme rdf:resource=\"{}\"/>\n    <skos:topConceptOf rdf:resource=\"{}\"/>\n    <skos:prefLabel>{}</skos:prefLabel>",
            xml_escape(&theme.uri),
            xml_escape(&model.scheme_uri),
            xml_escape(&model.scheme_uri),
            xml_escape(concept.pref_label),
        );
        for alt in concept.alt_labels {
            let _ = writeln!(
                out,
                "    <skos:altLabel>{}</skos:altLabel>",
                xml_escape(alt)
            );
        }
        let _ = writeln!(
            out,
            "    <skos:definition>{}</skos:definition>",
            xml_escape(concept.definition)
        );
        for example in &concept.examples {
            let _ = writeln!(
                out,
                "    <skos:example>{}</skos:example>",
                xml_escape(example)
            );
        }
        out.push_str("  </skos:Concept>\n");
    }
    out.push_str("</rdf:RDF>\n");
    out
}

fn json_ld(model: &ExportModel) -> Result<Vec<u8>, ApplicationError> {
    use serde_json::{Value, json};

    let mut graph = vec![json!({
        "@id": model.scheme_uri,
        "@type": "skos:ConceptScheme",
        "dct:title": scheme_title(model),
        "dct:created": { "@value": model.generated_on, "@type": "xsd:date" },
        "skos:note": GENERATED_NOTE,
        "skos:hasTopConcept": model
            .themes
            .iter()
            .map(|theme| json!({ "@id": theme.uri }))
            .collect::<Vec<Value>>(),
    })];
    for theme in &model.themes {
        let concept = literals(theme, model);
        let mut node = json!({
            "@id": theme.uri,
            "@type": "skos:Concept",
            "skos:inScheme": { "@id": model.scheme_uri },
            "skos:topConceptOf": { "@id": model.scheme_uri },
            "skos:prefLabel": concept.pref_label,
            "skos:definition": concept.definition,
        });
        if let Some(object) = node.as_object_mut() {
            if !concept.alt_labels.is_empty() {
                object.insert("skos:altLabel".to_owned(), json!(concept.alt_labels));
            }
            if !concept.examples.is_empty() {
                object.insert("skos:example".to_owned(), json!(concept.examples));
            }
        }
        graph.push(node);
    }
    let document = json!({
        "@context": {
            "skos": "http://www.w3.org/2004/02/skos/core#",
            "dct": "http://purl.org/dc/terms/",
            "xsd": "http://www.w3.org/2001/XMLSchema#",
        },
        "@graph": graph,
    });
    serde_json::to_vec_pretty(&document).map_err(|error| invalid(error.to_string()))
}

#[cfg(test)]
mod tests {
    use std::io::Read as _;

    use fm_transport_dto::{NoThemeDto, ThemeKeywordDto, ThemePassageDto, ThemeShareDto};

    use super::*;

    fn keyword(term: &str, score: f32) -> ThemeKeywordDto {
        ThemeKeywordDto {
            term: term.to_owned(),
            score,
        }
    }

    fn passage(title: &str, excerpt: &str) -> ThemePassageDto {
        ThemePassageDto {
            chunk_id: format!("chunk-{title}"),
            source_id: format!("source-{title}"),
            title: title.to_owned(),
            source_position: 0,
            section_path: vec!["Intro".to_owned()],
            provenance: String::new(),
            excerpt: excerpt.to_owned(),
        }
    }

    fn theme(id: u32, terms: &[&str], passages: Vec<ThemePassageDto>) -> CorpusThemeDto {
        CorpusThemeDto {
            id,
            chunk_count: 40,
            document_count: 4,
            share: 0.4,
            keywords: terms.iter().map(|term| keyword(term, 1.0)).collect(),
            passages,
            concepts: Vec::new(),
        }
    }

    fn mixture(
        source: &str,
        title: &str,
        shares: &[(Option<u32>, u32, f32)],
    ) -> DocumentThemeMixtureDto {
        DocumentThemeMixtureDto {
            source_id: source.to_owned(),
            title: title.to_owned(),
            chunk_count: shares.iter().map(|share| share.1).sum(),
            shares: shares
                .iter()
                .map(|(theme, chunk_count, share)| ThemeShareDto {
                    theme: *theme,
                    chunk_count: *chunk_count,
                    share: *share,
                })
                .collect(),
        }
    }

    /// Two themes with interleaved, unsorted document mixtures across them.
    fn analysis() -> CorpusThemesDto {
        CorpusThemesDto {
            index_revision: "rev".to_owned(),
            fingerprint: "fp-1".to_owned(),
            algorithm_version: "v1".to_owned(),
            model_id: "model".to_owned(),
            chunk_count: 100,
            document_count: 8,
            sampled: false,
            incremental: false,
            themes: vec![
                theme(
                    0,
                    &["adolescence", "crime", "12", "ab", "delinquency"],
                    vec![passage(
                        "a%20paper.pdf",
                        "Youth &amp; crime <rates> rose.\n\nSecond   line.",
                    )],
                ),
                theme(
                    1,
                    &["triz", "contradiction"],
                    vec![passage("triz.md", "Inventive principles.")],
                ),
            ],
            no_theme: NoThemeDto {
                chunk_count: 0,
                document_count: 0,
                share: 0.0,
                passages: Vec::new(),
            },
            document_mixtures: vec![
                mixture("s3", "triz.md", &[(Some(1), 9, 0.9), (Some(0), 1, 0.1)]),
                mixture(
                    "s1",
                    "crime%20study.pdf",
                    &[(Some(0), 12, 0.8), (Some(1), 1, 0.02)],
                ),
                mixture("s2", "youth.pdf", &[(Some(1), 3, 0.3), (Some(0), 7, 0.7)]),
                mixture("s4", "tiny.pdf", &[(Some(0), 1, 0.01), (None, 99, 0.99)]),
            ],
            mixtures_truncated: false,
            map_points: Vec::new(),
            map_sampled: false,
        }
    }

    fn options() -> ExportOptions {
        ExportOptions {
            include_passages: false,
            include_documents: false,
            base_uri: None,
            scope_label: "Research & notes".to_owned(),
            generated_on: "2026-11-04".to_owned(),
        }
    }

    fn text(
        format: CorpusThemeExportFormatDto,
        selected: &[u32],
        options: &ExportOptions,
    ) -> String {
        let rendered = render(format, &analysis(), selected, &[], options).expect("must render");
        String::from_utf8(rendered.bytes).expect("text formats are UTF-8")
    }

    #[test]
    fn markdown_has_labels_keywords_and_the_generated_notice_but_no_document_text_by_default() {
        let output = text(CorpusThemeExportFormatDto::Markdown, &[], &options());
        assert!(output.starts_with("# Corpus themes: Research & notes"));
        assert!(output.contains("## 1. adolescence, crime, delinquency"));
        assert!(output.contains("## 2. triz, contradiction"));
        assert!(output.contains("**Keywords:** adolescence, crime, delinquency"));
        assert!(output.contains("not curated concepts"));
        assert!(!output.contains("Youth"), "passages are opt-in");
        assert!(
            !output.contains("crime study"),
            "document titles are opt-in"
        );
        // Short and numeric keywords never become labels or keywords.
        assert!(!output.contains(", 12") && !output.contains("ab,"));
    }

    #[test]
    fn markdown_includes_cleaned_passages_and_ordered_documents_when_asked() {
        let mut opts = options();
        opts.include_passages = true;
        opts.include_documents = true;
        let output = text(CorpusThemeExportFormatDto::Markdown, &[0], &opts);
        assert!(output.contains("Youth & crime \\<rates\\> rose. Second line."));
        assert!(
            output.contains("a paper.pdf, Intro"),
            "title is percent-decoded"
        );
        let crime = output.find("crime study.pdf").expect("listed");
        let youth = output.find("youth.pdf").expect("listed");
        assert!(
            crime < youth,
            "more passages first, regardless of input order"
        );
        assert!(!output.contains("tiny.pdf"), "below the minimum share");
        assert!(
            !output.contains("triz.md (90%)"),
            "theme 1 was not selected"
        );
        assert!(!output.contains("## 2."));
    }

    #[test]
    fn documents_are_selected_per_theme_from_interleaved_mixtures() {
        let mut opts = options();
        opts.include_documents = true;
        let all = text(CorpusThemeExportFormatDto::Markdown, &[], &opts);
        let second = all.split("## 2.").nth(1).expect("second theme");
        assert!(second.contains("triz.md (90%)"));
        assert!(second.contains("youth.pdf (30%)"));
        assert!(
            !second.contains("crime study.pdf"),
            "only 2% of that document"
        );
    }

    #[test]
    fn supplied_labels_win_and_unknown_or_empty_selections_are_rejected() {
        let labels = [ThemeExportLabelDto {
            theme: 1,
            label: "TRIZ &amp; friends".to_owned(),
        }];
        let rendered = render(
            CorpusThemeExportFormatDto::Markdown,
            &analysis(),
            &[1],
            &labels,
            &options(),
        )
        .expect("renders");
        assert!(
            String::from_utf8(rendered.bytes)
                .expect("utf8")
                .contains("## 2. TRIZ & friends")
        );
        assert_eq!(rendered.theme_count, 1);
        assert!(
            render(
                CorpusThemeExportFormatDto::Markdown,
                &analysis(),
                &[9],
                &[],
                &options()
            )
            .is_err()
        );
        let mut empty = analysis();
        empty.themes.clear();
        assert!(
            render(
                CorpusThemeExportFormatDto::Markdown,
                &empty,
                &[],
                &[],
                &options()
            )
            .is_err()
        );
    }

    #[test]
    fn turtle_is_a_concept_scheme_with_stable_urn_identifiers() {
        let first = text(CorpusThemeExportFormatDto::SkosTurtle, &[], &options());
        let second = text(CorpusThemeExportFormatDto::SkosTurtle, &[], &options());
        assert_eq!(first, second, "identifiers are deterministic");
        assert!(first.contains("a skos:ConceptScheme"));
        assert_eq!(first.matches("a skos:Concept ;").count(), 2);
        assert!(first.contains("skos:prefLabel \"adolescence, crime, delinquency\""));
        assert!(first.contains("skos:altLabel \"triz\""));
        assert!(first.contains("skos:definition \"40 passages in 4 documents (40% of the analysed passages); leading terms:"));
        assert!(first.contains("<urn:uuid:"));
        assert!(first.contains("skos:topConceptOf"));
        assert!(
            !first.contains("skos:broader"),
            "flat clusters have no hierarchy"
        );
        assert!(!first.contains("skos:example"), "passages are opt-in");
    }

    #[test]
    fn a_base_uri_replaces_urns_and_is_validated() {
        let mut opts = options();
        opts.base_uri = Some(" https://example.org/themes ".to_owned());
        let output = text(CorpusThemeExportFormatDto::SkosTurtle, &[], &opts);
        assert!(output.contains("<https://example.org/themes/scheme> a skos:ConceptScheme"));
        assert!(output.contains("<https://example.org/themes/theme-0> a skos:Concept"));
        assert!(!output.contains("urn:uuid"));
        for bad in [
            "not a uri",
            "example.org/x",
            "https://a b/",
            "x:",
            "https://e.org/<>",
        ] {
            let mut opts = options();
            opts.base_uri = Some(bad.to_owned());
            assert!(
                render(
                    CorpusThemeExportFormatDto::SkosTurtle,
                    &analysis(),
                    &[],
                    &[],
                    &opts
                )
                .is_err(),
                "{bad} must be rejected"
            );
        }
    }

    #[test]
    fn passages_reach_skos_only_through_skos_example_when_included() {
        let mut opts = options();
        opts.include_passages = true;
        let output = text(CorpusThemeExportFormatDto::SkosTurtle, &[0], &opts);
        assert!(output.contains("skos:example \"Youth & crime <rates> rose. Second line.\""));
    }

    #[test]
    fn turtle_escapes_quotes_backslashes_and_control_characters() {
        let mut data = analysis();
        data.themes[0].keywords = vec![keyword("say \"hi\"\\now", 1.0)];
        let rendered = render(
            CorpusThemeExportFormatDto::SkosTurtle,
            &data,
            &[0],
            &[],
            &options(),
        )
        .expect("renders");
        let output = String::from_utf8(rendered.bytes).expect("utf8");
        assert!(output.contains("skos:altLabel \"say \\\"hi\\\"\\\\now\""));
    }

    #[test]
    fn rdf_xml_is_well_formed_and_escaped() {
        let mut opts = options();
        opts.include_passages = true;
        let output = text(CorpusThemeExportFormatDto::SkosRdfXml, &[], &opts);
        assert!(output.starts_with("<?xml"));
        assert!(
            output.contains("<skos:prefLabel>adolescence, crime, delinquency</skos:prefLabel>")
        );
        assert!(output.contains("Youth &amp; crime &lt;rates&gt; rose."));
        let mut reader = quick_xml::Reader::from_str(&output);
        let mut depth_ok = true;
        loop {
            match reader.read_event() {
                Ok(quick_xml::events::Event::Eof) => break,
                Ok(_) => {}
                Err(_) => {
                    depth_ok = false;
                    break;
                }
            }
        }
        assert!(depth_ok, "the document parses as XML");
    }

    #[test]
    fn json_ld_parses_and_lists_every_concept() {
        let output = text(CorpusThemeExportFormatDto::SkosJsonLd, &[], &options());
        let value: serde_json::Value = serde_json::from_str(&output).expect("valid JSON");
        let graph = value["@graph"].as_array().expect("graph");
        assert_eq!(graph.len(), 3, "one scheme and two concepts");
        assert_eq!(graph[0]["@type"], "skos:ConceptScheme");
        assert_eq!(
            graph[1]["skos:prefLabel"],
            "adolescence, crime, delinquency"
        );
        assert_eq!(
            graph[0]["skos:hasTopConcept"]
                .as_array()
                .expect("tops")
                .len(),
            2
        );
    }

    /// Text content of an XML document: tags dropped, the five entities decoded.
    fn xml_text(xml: &str) -> String {
        let mut out = String::new();
        let mut in_tag = false;
        for character in xml.chars() {
            match character {
                '<' => in_tag = true,
                '>' => in_tag = false,
                other if !in_tag => out.push(other),
                _ => {}
            }
        }
        out.replace("&amp;", "&")
    }

    #[test]
    fn docx_is_a_word_package_carrying_the_report_text() {
        let mut opts = options();
        opts.include_passages = true;
        let rendered = render(
            CorpusThemeExportFormatDto::Docx,
            &analysis(),
            &[],
            &[],
            &opts,
        )
        .expect("renders");
        let mut archive =
            zip::ZipArchive::new(std::io::Cursor::new(rendered.bytes)).expect("a zip package");
        let mut xml = String::new();
        archive
            .by_name("word/document.xml")
            .expect("document part")
            .read_to_string(&mut xml)
            .expect("utf8");
        let plain = xml_text(&xml);
        assert!(plain.contains("Corpus themes: Research & notes"));
        assert!(plain.contains("adolescence, crime, delinquency"));
        assert!(plain.contains("Inventive principles."));
    }

    #[test]
    fn entities_decode_without_touching_stray_ampersands() {
        assert_eq!(
            clean_text("a &amp; b &#65;&#x42; &bogus; & c"),
            "a & b AB &bogus; & c"
        );
        assert_eq!(clean_title("caf%C3%A9%20menu.pdf"), "café menu.pdf");
    }
}
