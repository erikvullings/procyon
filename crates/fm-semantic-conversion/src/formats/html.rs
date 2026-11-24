//! HTML conversion on `html5ever` (through `dom_query`), the parser browsers' standard describes.
//!
//! Script, style and similar content, comments, unclosed and misnested elements are handled as a
//! browser does; nothing is fetched and no external entity is resolved. A standalone web page is
//! first reduced to its main content with `dom_smoothie` (a port of Mozilla's Readability), so
//! menus and other site chrome do not fill the text; when no main content can be told apart, the
//! whole page is read without its navigation, header, footer and asides, and a warning says so.
//! EPUB chapters are read whole.
//!
//! The tree is walked iteratively; element nesting is charged against the nesting budget before
//! any library code walks the tree, so a pathologically deep document is refused rather than
//! recursed into. Units carry line ranges located in the source on a best-effort basis (the text
//! of a unit is searched for after the previous one), not a character-level source map.

use dom_query::{Document, NodeRef};
use dom_smoothie::{Config, Readability};

use crate::budget::Stop;
use crate::builder::{DocumentBuilder, UnitDraft};
use crate::model::{ConversionWarning, Provenance, TopLevelBoundary, UnitKind};

#[derive(Debug, Clone, Copy)]
pub(crate) enum HtmlContext {
    Standalone,
    EpubSpine { spine_index: u32 },
}

/// Elements whose content is never text of the document.
const SKIPPED_ELEMENTS: &[&str] = &[
    "head", "script", "style", "noscript", "template", "svg", "math", "iframe", "object",
];

/// Site chrome left out of a web page read whole: always for `nav` and `aside`, and for `header`
/// and `footer` outside an `article` or `main` (where they hold the article's own title or notes).
const CHROME_ELEMENTS: &[&str] = &["nav", "aside"];
const PAGE_CHROME_ELEMENTS: &[&str] = &["header", "footer"];
const CONTENT_ELEMENTS: &[&str] = &["article", "main"];

/// Readability's main content is used only when it has at least this many characters...
const MIN_ARTICLE_CHARS: usize = 500;
/// ...and at least this share (1 in N) of the page's visible text; a smaller pick is a fragment.
const MIN_ARTICLE_SHARE: usize = 5;
/// ...and is not mostly link text (a menu, not an article): links at most 1 in N of its text.
const MAX_ARTICLE_LINK_SHARE: usize = 2;
/// Most elements Readability looks at; the byte budget bounds the page before this.
const MAX_READABILITY_ELEMENTS: usize = 100_000;

/// Elements that end the current text block.
fn block_kind(name: &str) -> Option<UnitKind> {
    match name {
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => Some(UnitKind::Heading),
        "p" | "div" | "section" | "article" | "header" | "footer" | "main" | "pre"
        | "figcaption" | "dd" | "dt" => Some(UnitKind::Paragraph),
        "li" => Some(UnitKind::ListItem),
        "blockquote" => Some(UnitKind::Quote),
        "table" => Some(UnitKind::Table),
        "title" => Some(UnitKind::Heading),
        _ => None,
    }
}

fn heading_level(name: &str) -> Option<u32> {
    match name {
        "h1" => Some(1),
        "h2" => Some(2),
        "h3" => Some(3),
        "h4" => Some(4),
        "h5" => Some(5),
        "h6" => Some(6),
        "title" => Some(0),
        _ => None,
    }
}

/// An element's local name: lower case and without a namespace prefix (`xhtml:h1` is `h1`), as
/// EPUB chapters may write it.
fn local_name(name: &str) -> String {
    name.rsplit(':')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Converts bounded HTML into headings, paragraphs, list items and tables.
pub(crate) fn convert(builder: &mut DocumentBuilder<'_, '_>, source: &str) -> Result<(), Stop> {
    convert_with_context(builder, source, HtmlContext::Standalone)
}

pub(crate) fn convert_with_context(
    builder: &mut DocumentBuilder<'_, '_>,
    source: &str,
    context: HtmlContext,
) -> Result<(), Stop> {
    builder.checkpoint()?;
    let document = Document::from(source);
    let root = document.root();
    // Depth is charged here, iteratively, before Readability or any other library walk.
    let page = survey(builder, &root, ReadMode::WholePage)?;
    let mut lines = LineLocator::new(source);
    let title = normalize(&document.select("title").text());

    if matches!(context, HtmlContext::Standalone) && page.chars >= MIN_ARTICLE_CHARS {
        let config = Config {
            max_elements_to_parse: MAX_READABILITY_ELEMENTS,
            ..Config::default()
        };
        let extracted = Readability::with_document(Document::from(source), None, Some(config))
            .ok()
            .and_then(|readability| readability.extract().ok());
        if let Some(extracted) = extracted {
            let article = extracted.root();
            let found = survey(builder, &article, ReadMode::Article)?;
            if found.chars >= MIN_ARTICLE_CHARS
                && found.chars.saturating_mul(MIN_ARTICLE_SHARE) >= page.chars
                && found.link_chars.saturating_mul(MAX_ARTICLE_LINK_SHARE) <= found.chars
            {
                let article_title = normalize(&extracted.metadata.title);
                let title = if article_title.is_empty() {
                    title
                } else {
                    article_title
                };
                let mut walker = Walker::new(builder, context, &mut lines);
                walker.title(&title)?;
                return walker.walk(&article, ReadMode::Article);
            }
        }
        builder.warn(ConversionWarning::MainContentNotFound);
    }
    let mut walker = Walker::new(builder, context, &mut lines);
    walker.title(&title)?;
    let mode = match context {
        HtmlContext::Standalone => ReadMode::WholePage,
        HtmlContext::EpubSpine { .. } => ReadMode::Chapter,
    };
    walker.walk(&root, mode)
}

/// What a walk leaves out besides [`SKIPPED_ELEMENTS`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReadMode {
    /// A web page read whole: its site chrome is left out.
    WholePage,
    /// Readability's main content: already free of chrome.
    Article,
    /// An EPUB chapter: every element is content.
    Chapter,
}

/// Whether an element and everything in it is left out. `inside_content` says whether an
/// `article` or `main` encloses it.
fn left_out(name: &str, mode: ReadMode, inside_content: bool) -> bool {
    SKIPPED_ELEMENTS.contains(&name)
        || (mode == ReadMode::WholePage
            && (CHROME_ELEMENTS.contains(&name)
                || (!inside_content && PAGE_CHROME_ELEMENTS.contains(&name))))
}

/// How much visible text a tree holds.
#[derive(Debug, Clone, Copy, Default)]
struct TextAmount {
    /// Visible characters, whitespace excluded.
    chars: usize,
    /// The part of `chars` inside links.
    link_chars: usize,
}

/// The visible text under `root`, charging element nesting against the depth budget.
fn survey(
    builder: &mut DocumentBuilder<'_, '_>,
    root: &NodeRef<'_>,
    mode: ReadMode,
) -> Result<TextAmount, Stop> {
    let mut amount = TextAmount::default();
    // (node, depth, inside an article or main, inside a link)
    let mut stack = vec![(*root, 0_u32, false, false)];
    while let Some((node, depth, inside_content, inside_link)) = stack.pop() {
        if node.is_text() {
            let chars = node.text().chars().filter(|c| !c.is_whitespace()).count();
            amount.chars += chars;
            if inside_link {
                amount.link_chars += chars;
            }
            continue;
        }
        let mut depth = depth;
        let mut inside_content = inside_content;
        let mut inside_link = inside_link;
        if let Some(name) = node.node_name() {
            let name = local_name(&name);
            depth += 1;
            builder.tracker().charge_depth(depth)?;
            if left_out(&name, mode, inside_content) {
                continue;
            }
            inside_content |= CONTENT_ELEMENTS.contains(&name.as_str());
            inside_link |= name == "a";
        }
        for child in node.children_it(true) {
            stack.push((child, depth, inside_content, inside_link));
        }
    }
    Ok(amount)
}

enum Step<'a> {
    Enter(NodeRef<'a>, u32, bool),
    Exit(Option<String>),
}

/// Builds units from a tree: one per block element, text between blocks going to the enclosing
/// block, so no text is lost around nested blocks.
struct Walker<'w, 'b, 'c, 's> {
    builder: &'w mut DocumentBuilder<'b, 'c>,
    context: HtmlContext,
    lines: &'w mut LineLocator<'s>,
    headings: Vec<(u32, String)>,
    /// Open block elements, innermost last.
    blocks: Vec<(UnitKind, String)>,
    text: String,
    pending_cell_separator: bool,
}

impl<'w, 'b, 'c, 's> Walker<'w, 'b, 'c, 's> {
    fn new(
        builder: &'w mut DocumentBuilder<'b, 'c>,
        context: HtmlContext,
        lines: &'w mut LineLocator<'s>,
    ) -> Self {
        Self {
            builder,
            context,
            lines,
            headings: Vec::new(),
            blocks: Vec::new(),
            text: String::new(),
            pending_cell_separator: false,
        }
    }

    /// The document's title, as the top-level heading.
    fn title(&mut self, title: &str) -> Result<(), Stop> {
        if title.is_empty() {
            return Ok(());
        }
        self.text = title.to_owned();
        self.flush(UnitKind::Heading, "title")
    }

    fn walk(mut self, root: &NodeRef<'_>, mode: ReadMode) -> Result<(), Stop> {
        let mut stack = vec![Step::Enter(*root, 0, false)];
        while let Some(step) = stack.pop() {
            self.builder.checkpoint()?;
            if self.builder.is_saturated() {
                return Ok(());
            }
            match step {
                Step::Exit(Some(name)) => {
                    let kind = self
                        .blocks
                        .pop()
                        .map_or(UnitKind::Paragraph, |(kind, _)| kind);
                    self.flush(kind, &name)?;
                }
                Step::Exit(None) => {}
                Step::Enter(node, depth, inside_content) => {
                    if node.is_text() {
                        self.text.push_str(&node.text());
                        continue;
                    }
                    let Some(name) = node.node_name() else {
                        // The document node and fragments: walk their children.
                        for child in node.children_it(true) {
                            stack.push(Step::Enter(child, depth, inside_content));
                        }
                        continue;
                    };
                    let name = local_name(&name);
                    let depth = depth + 1;
                    self.builder.tracker().charge_depth(depth)?;
                    if left_out(&name, mode, inside_content) {
                        continue;
                    }
                    let inside_content =
                        inside_content || CONTENT_ELEMENTS.contains(&name.as_str());
                    match name.as_str() {
                        "br" => self.text.push('\n'),
                        "td" | "th" => {
                            if self.pending_cell_separator {
                                self.text.push_str(" | ");
                            }
                            self.pending_cell_separator = true;
                        }
                        "tr" => {
                            self.text.push('\n');
                            self.pending_cell_separator = false;
                        }
                        _ => {}
                    }
                    let block = block_kind(&name);
                    if let Some(kind) = block {
                        // Text so far belongs to the enclosing block.
                        let (outer_kind, outer_name) = self
                            .blocks
                            .last()
                            .cloned()
                            .unwrap_or((UnitKind::Paragraph, String::new()));
                        self.flush(outer_kind, &outer_name)?;
                        self.blocks.push((kind, name.clone()));
                    }
                    stack.push(Step::Exit(block.map(|_| name)));
                    for child in node.children_it(true) {
                        stack.push(Step::Enter(child, depth, inside_content));
                    }
                }
            }
        }
        // Text after the last block.
        let (kind, name) = self
            .blocks
            .pop()
            .unwrap_or((UnitKind::Paragraph, String::new()));
        self.flush(kind, &name)
    }

    /// Emits the text gathered so far as one unit of `kind`; nothing when it is blank.
    fn flush(&mut self, kind: UnitKind, name: &str) -> Result<(), Stop> {
        let raw = std::mem::take(&mut self.text);
        let text = raw
            .lines()
            .map(normalize)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        if text.is_empty() {
            return Ok(());
        }
        let level = heading_level(name);
        if let Some(level) = level {
            while self.headings.last().is_some_and(|(open, _)| *open >= level) {
                self.headings.pop();
            }
        }
        let section_path = self
            .headings
            .iter()
            .map(|(_, title)| title.clone())
            .collect::<Vec<_>>();
        let boundary = match self.context {
            HtmlContext::Standalone => match (level, self.headings.first()) {
                (_, Some((_, title))) => TopLevelBoundary::Section(title.clone()),
                (Some(_), None) => TopLevelBoundary::Section(text.clone()),
                (None, None) => TopLevelBoundary::Document,
            },
            HtmlContext::EpubSpine { spine_index } => TopLevelBoundary::EpubSpine(spine_index),
        };
        let (start_line, end_line) = self.lines.locate(&text);
        let provenance = match self.context {
            HtmlContext::Standalone => Provenance::TextLines {
                start_line,
                end_line,
            },
            HtmlContext::EpubSpine { spine_index } => Provenance::EpubText {
                spine_index,
                start_line,
                end_line,
            },
        };
        self.builder.push(UnitDraft {
            kind,
            section_path,
            text: text.clone(),
            provenance,
            boundary,
            source_offset: None,
        })?;
        if let Some(level) = level {
            self.headings.push((level, text));
        }
        Ok(())
    }
}

/// Finds a unit's lines in the source, best effort: the unit's most distinctive word near its
/// start is searched for after the previous unit, and its last such word after that. A unit whose
/// words cannot be found (entities, markup inside a word) takes the previous unit's last line.
struct LineLocator<'s> {
    source: &'s str,
    /// Byte position after the last located unit, and its line.
    position: usize,
    line: u32,
}

/// How many words at a unit's start (or end) are candidates for locating it.
const LOCATING_WORDS: usize = 5;

impl<'s> LineLocator<'s> {
    fn new(source: &'s str) -> Self {
        Self {
            source,
            position: 0,
            line: 1,
        }
    }

    fn locate(&mut self, text: &str) -> (u32, u32) {
        let words: Vec<&str> = text
            .split(|c: char| !c.is_alphanumeric())
            .filter(|word| word.chars().count() >= 3)
            .collect();
        let pick = |candidates: &[&'_ str]| -> Option<String> {
            candidates
                .iter()
                .max_by_key(|word| word.len())
                .map(|word| (*word).to_owned())
        };
        let first = pick(&words[..words.len().min(LOCATING_WORDS)]);
        let last = pick(&words[words.len().saturating_sub(LOCATING_WORDS)..]);
        let Some(start) = first.and_then(|word| self.find(&word, self.position)) else {
            return (self.line, self.line);
        };
        let start_line = self.advance(start);
        let end = last
            .and_then(|word| self.find(&word, start).map(|at| at + word.len()))
            .unwrap_or(start);
        let end_line = self.advance(end);
        (start_line, end_line.max(start_line))
    }

    fn find(&self, word: &str, from: usize) -> Option<usize> {
        self.source.get(from..)?.find(word).map(|at| from + at)
    }

    /// Moves to `to` (not backwards) and returns its line.
    fn advance(&mut self, to: usize) -> u32 {
        if to > self.position {
            let newlines = self.source[self.position..to].matches('\n').count();
            self.line += u32::try_from(newlines).unwrap_or(u32::MAX);
            self.position = to;
        }
        self.line
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::budget::{BudgetTracker, ConversionBudgets, ManualClock};
    use crate::cancellation::Cancellation;
    use crate::model::{ComponentVersion, ConvertedDocument, FormatKind};

    fn convert_html(source: &str) -> ConvertedDocument {
        convert_html_with(source, ConversionBudgets::default())
    }

    fn convert_html_with(source: &str, budgets: ConversionBudgets) -> ConvertedDocument {
        let cancellation = Cancellation::none();
        let clock = ManualClock::new();
        let mut tracker = BudgetTracker::new(&budgets, &cancellation, &clock);
        let mut builder = DocumentBuilder::new(
            ComponentVersion::new("baseline", 1),
            FormatKind::Html,
            &mut tracker,
        );
        convert(&mut builder, source).expect("conversion");
        builder.finish()
    }

    const SAMPLE: &str = "<html><head><title>Doc</title><style>p{color:red}</style></head><body>\n<h1>Heading</h1>\n<p>First &amp; only paragraph.</p>\n<script>alert('x')</script>\n<ul><li>alpha</li><li>beta</li></ul>\n<table><tr><td>1</td><td>2</td></tr></table>\n</body></html>";

    #[test]
    fn script_and_style_content_is_discarded() {
        let document = convert_html(SAMPLE);
        let texts: Vec<&str> = document
            .units()
            .iter()
            .map(|unit| unit.text.as_str())
            .collect();
        assert!(!texts.iter().any(|text| text.contains("alert")));
        assert!(!texts.iter().any(|text| text.contains("color:red")));
    }

    #[test]
    fn nested_skipped_elements_cannot_leak_text_into_an_enclosing_block() {
        let document = convert_html(
            "<p>before<noscript><noscript>inner</noscript>still hidden</noscript>after</p>",
        );

        assert_eq!(document.units()[0].text, "beforeafter");
    }

    #[test]
    fn script_content_is_raw_text_that_ends_at_its_end_tag() {
        // Markup-like code and stray quotes inside a script must not swallow the page after it.
        let document = convert_html(
            "<p>before<script>var s = '<div>'; if (a<b && it's) { x = \"<span\" }</SCRIPT >after</p>\
             <style>p::after{content:'<p>'}</style><p>later paragraph</p>",
        );
        let texts: Vec<&str> = document
            .units()
            .iter()
            .map(|unit| unit.text.as_str())
            .collect();

        assert_eq!(texts, ["beforeafter", "later paragraph"]);
    }

    #[test]
    fn an_unclosed_script_discards_only_what_follows_it() {
        let document = convert_html("<p>kept</p><script>never closed <p>lost</p>");

        let texts: Vec<&str> = document
            .units()
            .iter()
            .map(|unit| unit.text.as_str())
            .collect();
        assert_eq!(texts, ["kept"]);
    }

    #[test]
    fn skipped_elements_still_count_toward_the_nesting_budget() {
        let budgets = ConversionBudgets {
            max_nesting_depth: 2,
            ..ConversionBudgets::default()
        };
        let cancellation = Cancellation::none();
        let clock = ManualClock::new();
        let mut tracker = BudgetTracker::new(&budgets, &cancellation, &clock);
        let mut builder = DocumentBuilder::new(
            ComponentVersion::new("baseline", 1),
            FormatKind::Html,
            &mut tracker,
        );

        let outcome = convert(&mut builder, "<html><svg><g>hidden</g></svg></html>");

        assert!(matches!(
            outcome,
            Err(Stop::OverBudget {
                kind: crate::budget::BudgetKind::NestingDepth,
                limit: 2
            })
        ));
    }

    #[test]
    fn headings_paragraphs_lists_and_tables_are_extracted_with_entities_decoded() {
        let document = convert_html(SAMPLE);
        let paragraph = document
            .units()
            .iter()
            .find(|unit| unit.kind == UnitKind::Paragraph)
            .expect("paragraph");
        assert_eq!(paragraph.text, "First & only paragraph.");
        assert_eq!(paragraph.section_path, ["Doc", "Heading"]);
        let items: Vec<&str> = document
            .units()
            .iter()
            .filter(|unit| unit.kind == UnitKind::ListItem)
            .map(|unit| unit.text.as_str())
            .collect();
        assert_eq!(items, ["alpha", "beta"]);
        let table = document
            .units()
            .iter()
            .find(|unit| unit.kind == UnitKind::Table)
            .expect("table");
        assert_eq!(table.text, "1 | 2");
    }

    #[test]
    fn line_ranges_track_the_source() {
        let document = convert_html(SAMPLE);
        let paragraph = document
            .units()
            .iter()
            .find(|unit| unit.kind == UnitKind::Paragraph)
            .expect("paragraph");
        assert_eq!(
            paragraph.provenance,
            Provenance::TextLines {
                start_line: 3,
                end_line: 3
            }
        );
    }

    #[test]
    fn excessive_nesting_is_refused_by_the_depth_budget() {
        let budgets = ConversionBudgets {
            max_nesting_depth: 8,
            ..ConversionBudgets::default()
        };
        let deep = "<div>".repeat(64) + "text" + &"</div>".repeat(64);
        let cancellation = Cancellation::none();
        let clock = ManualClock::new();
        let mut tracker = BudgetTracker::new(&budgets, &cancellation, &clock);
        let mut builder = DocumentBuilder::new(
            ComponentVersion::new("baseline", 1),
            FormatKind::Html,
            &mut tracker,
        );
        let outcome = convert(&mut builder, &deep);
        assert!(matches!(
            outcome,
            Err(Stop::OverBudget {
                kind: crate::budget::BudgetKind::NestingDepth,
                limit: 8
            })
        ));
        drop(convert_html_with("<p>ok</p>", ConversionBudgets::default()));
    }

    #[test]
    fn malformed_markup_still_yields_the_visible_text() {
        let document = convert_html("<p>unclosed paragraph<div>and a div");
        let texts: Vec<&str> = document
            .units()
            .iter()
            .map(|unit| unit.text.as_str())
            .collect();
        assert_eq!(texts, ["unclosed paragraph", "and a div"]);
    }

    /// A news-style page: site chrome, scripts holding markup-like strings, and one article
    /// long enough for Readability. Synthetic: real pages are copyrighted.
    fn news_page() -> String {
        let paragraphs: String = (1..=6)
            .map(|n| {
                format!(
                    "<p>Paragraph {n} of the article explains how decoys draw fire away from \
                     real positions, why cheap inflatable vehicles still work, and what units \
                     learned about hiding heat and radio signatures in the field.</p>\n"
                )
            })
            .collect();
        format!(
            "<!DOCTYPE html><html><head><title>Decoy Warfare | Proceedings</title>\
             <script>var tpl = '<div class=\"x\">'; if (a<b) {{ x = \"it's\"; }}</script></head>\
             <body><header><a href=\"/\">Donate</a> <a href=\"/cart\">Cart</a> Join or Log In</header>\
             <nav><ul><li>Main navigation</li><li>About Us</li><li>Membership</li></ul></nav>\
             <main><article><h1>Decoy Warfare</h1>\n{paragraphs}</article></main>\
             <aside>Related: other stories</aside>\
             <footer>Copyright and privacy policy</footer>\
             <script>document.write('<p>injected</p>')</script></body></html>"
        )
    }

    fn texts(document: &ConvertedDocument) -> Vec<&str> {
        document
            .units()
            .iter()
            .map(|unit| unit.text.as_str())
            .collect()
    }

    #[test]
    fn a_web_page_is_reduced_to_its_main_content() {
        let document = convert_html(&news_page());
        let texts = texts(&document);
        let all = texts.join("\n");

        assert!(all.contains("Paragraph 1 of the article"), "{all}");
        assert!(all.contains("Paragraph 6 of the article"), "{all}");
        for chrome in [
            "Main navigation",
            "Donate",
            "Join or Log In",
            "Related: other stories",
            "privacy policy",
            "injected",
            "class=",
        ] {
            assert!(!all.contains(chrome), "{chrome} leaked: {all}");
        }
        assert_eq!(document.units()[0].kind, UnitKind::Heading);
        assert!(document.warnings().is_empty(), "{:?}", document.warnings());
    }

    #[test]
    fn a_page_without_main_content_is_read_whole_without_its_chrome_and_says_so() {
        // A landing page: mostly menus, one short abstract inside the article.
        let menus: String = (1..=40)
            .map(|n| format!("<li><a href=\"/section/{n}\">Journal section number {n}</a></li>"))
            .collect();
        let page = format!(
            "<html><head><title>Fake objects</title></head><body>\
             <header>Site header</header><nav><ul>{menus}</ul></nav>\
             <div><ul>{menus}</ul></div>\
             <article><header><h1>Fake military objects</h1></header>\
             <p>A short abstract.</p></article><footer>Site footer</footer></body></html>"
        );
        let document = convert_html(&page);
        let all = texts(&document).join("\n");

        assert!(
            document
                .warnings()
                .contains(&ConversionWarning::MainContentNotFound),
            "{:?}",
            document.warnings()
        );
        assert!(all.contains("A short abstract."), "{all}");
        assert!(
            all.contains("Fake military objects"),
            "an article's own header is kept: {all}"
        );
        assert!(
            !all.contains("Site header") && !all.contains("Site footer"),
            "{all}"
        );
    }

    #[test]
    fn a_short_page_is_read_whole_without_a_warning() {
        let document = convert_html(SAMPLE);
        assert!(document.warnings().is_empty(), "{:?}", document.warnings());
    }

    #[test]
    fn text_around_a_nested_block_stays_with_its_block() {
        let document = convert_html("<div>before<p>inner</p>after</div>");
        assert_eq!(texts(&document), ["before", "inner", "after"]);
    }

    #[test]
    fn entities_decode_including_numeric_forms() {
        let document = convert_html("<p>a&amp;b&#65;c&#x42;d, bare & ampersand</p>");
        assert_eq!(document.units()[0].text, "a&bAcBd, bare & ampersand");
    }
}
