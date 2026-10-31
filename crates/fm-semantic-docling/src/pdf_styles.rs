//! Typography of the text layer: which font, size and weight each stretch of text is set in.
//!
//! `docling-pdf` reads this while parsing glyphs but keeps it private, and its word cells carry
//! only geometry. This module walks the page content streams with `lopdf` and records one [`Run`]
//! per text-showing operator: where it starts, how large it is set, its weight and slant (from the
//! font name and descriptor), and its text when the font makes that cheap to decode. Runs are
//! the evidence for heading and footnote detection in `structure`; they are not a text extractor.
//!
//! Coordinates are page points with the origin at the top left, like the word cells.

use std::borrow::Cow;
use std::collections::HashMap;

use lopdf::content::Content;
use lopdf::{Dictionary, Document, Object, ObjectId};

/// One stretch of text set in a single font, size and position.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Run {
    /// Left edge of the first glyph.
    pub x: f32,
    /// Right edge of the last glyph, from the font's glyph widths.
    pub right: f32,
    /// Baseline of the run, measured down from the top of the page.
    pub baseline: f32,
    /// Font size on the page, after the text and page matrices.
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    /// The font's object number: runs with equal values are set in one font.
    pub font: u32,
    /// Mean advance of common lowercase letters, in thousandths of an em (0 when unknown). Bold
    /// and sans faces are wider than the regular text face they sit among.
    pub letter_width: f32,
    /// Decoded text; may be empty when the font has no usable encoding.
    pub text: String,
}

/// The runs of one page and the page size they are measured in.
#[derive(Debug, Clone, Default)]
pub(crate) struct PageStyles {
    pub width: f32,
    pub height: f32,
    pub runs: Vec<Run>,
}

/// Most stream operators interpreted per page; a guard against hostile content.
const MAX_OPERATIONS: usize = 400_000;
/// Deepest nesting of form XObjects followed.
const MAX_FORM_DEPTH: u8 = 4;

type Matrix = [f32; 6];
const IDENTITY: Matrix = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// `a` applied first, then `b` (PDF row-vector convention).
fn multiply(a: Matrix, b: Matrix) -> Matrix {
    [
        a[0] * b[0] + a[1] * b[2],
        a[0] * b[1] + a[1] * b[3],
        a[2] * b[0] + a[3] * b[2],
        a[2] * b[1] + a[3] * b[3],
        a[4] * b[0] + a[5] * b[2] + b[4],
        a[4] * b[1] + a[5] * b[3] + b[5],
    ]
}

fn translation(x: f32, y: f32) -> Matrix {
    [1.0, 0.0, 0.0, 1.0, x, y]
}

/// Typography of every page of `pdf`; empty when `lopdf` cannot read it (encrypted, damaged).
pub(crate) fn page_styles(pdf: &[u8]) -> Vec<PageStyles> {
    let Ok(document) = Document::load_mem(pdf) else {
        return Vec::new();
    };
    let mut fonts: HashMap<ObjectId, FontInfo> = HashMap::new();
    document
        .get_pages()
        .into_values()
        .map(|page| page_runs(&document, page, &mut fonts))
        .collect()
}

fn number(object: &Object) -> Option<f32> {
    object.as_float().ok()
}

fn page_box(document: &Document, mut page: ObjectId) -> [f32; 4] {
    for _ in 0..32 {
        let Ok(dictionary) = document.get_dictionary(page) else {
            break;
        };
        for key in [&b"CropBox"[..], &b"MediaBox"[..]] {
            if let Ok(array) = dictionary
                .get(key)
                .and_then(|object| document.dereference(object).map(|(_, object)| object))
                .and_then(Object::as_array)
            {
                let values: Vec<f32> = array
                    .iter()
                    .filter_map(|object| {
                        document
                            .dereference(object)
                            .ok()
                            .and_then(|(_, object)| number(object))
                    })
                    .collect();
                if let [x0, y0, x1, y1] = values[..] {
                    return [x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)];
                }
            }
        }
        match dictionary.get(b"Parent").and_then(Object::as_reference) {
            Ok(parent) => page = parent,
            Err(_) => break,
        }
    }
    [0.0, 0.0, 612.0, 792.0]
}

struct TextState {
    font: Option<ObjectId>,
    size: f32,
    char_spacing: f32,
    word_spacing: f32,
    scale: f32,
    leading: f32,
    rise: f32,
    matrix: Matrix,
    line_matrix: Matrix,
}

impl TextState {
    fn new() -> Self {
        Self {
            font: None,
            size: 0.0,
            char_spacing: 0.0,
            word_spacing: 0.0,
            scale: 1.0,
            leading: 0.0,
            rise: 0.0,
            matrix: IDENTITY,
            line_matrix: IDENTITY,
        }
    }

    fn move_line(&mut self, x: f32, y: f32) {
        self.line_matrix = multiply(translation(x, y), self.line_matrix);
        self.matrix = self.line_matrix;
    }
}

fn page_runs(
    document: &Document,
    page: ObjectId,
    cache: &mut HashMap<ObjectId, FontInfo>,
) -> PageStyles {
    let [left, bottom, right, top] = page_box(document, page);
    let mut styles = PageStyles {
        width: right - left,
        height: top - bottom,
        runs: Vec::new(),
    };
    let resources = page_resources(document, page);
    let mut budget = MAX_OPERATIONS;
    interpret(
        &Scope {
            document,
            origin: [left, top],
        },
        &document.get_page_content(page),
        &resources,
        IDENTITY,
        0,
        cache,
        &mut styles,
        &mut budget,
    );
    styles
}

struct Scope<'a> {
    document: &'a Document,
    origin: [f32; 2],
}

/// Resource dictionaries a content stream may draw on, innermost first.
fn page_resources(document: &Document, page: ObjectId) -> Vec<Dictionary> {
    let mut found = Vec::new();
    if let Ok((direct, inherited)) = document.get_page_resources(page) {
        found.extend(direct.cloned());
        found.extend(
            inherited
                .into_iter()
                .filter_map(|id| document.get_dictionary(id).ok().cloned()),
        );
    }
    found
}

fn lookup<'a>(
    document: &'a Document,
    resources: &'a [Dictionary],
    category: &[u8],
    name: &[u8],
) -> Option<&'a Object> {
    resources.iter().find_map(|dictionary| {
        let group = dictionary
            .get(category)
            .ok()
            .and_then(|object| document.dereference(object).ok())
            .and_then(|(_, object)| object.as_dict().ok())?;
        group.get(name).ok()
    })
}

#[allow(clippy::too_many_arguments)]
fn interpret(
    scope: &Scope<'_>,
    data: &[u8],
    resources: &[Dictionary],
    start_ctm: Matrix,
    depth: u8,
    cache: &mut HashMap<ObjectId, FontInfo>,
    styles: &mut PageStyles,
    budget: &mut usize,
) {
    let document = scope.document;
    let Ok(content) = Content::decode(data) else {
        return;
    };
    let mut ctm = start_ctm;
    let mut saved: Vec<Matrix> = Vec::new();
    let mut text = TextState::new();
    for operation in &content.operations {
        if *budget == 0 {
            return;
        }
        *budget -= 1;
        let operands = &operation.operands;
        let value = |index: usize| operands.get(index).and_then(number).unwrap_or(0.0);
        let six = || [value(0), value(1), value(2), value(3), value(4), value(5)];
        match operation.operator.as_str() {
            "q" => saved.push(ctm),
            "Q" => ctm = saved.pop().unwrap_or(ctm),
            "cm" if operands.len() == 6 => ctm = multiply(six(), ctm),
            "BT" => {
                text.matrix = IDENTITY;
                text.line_matrix = IDENTITY;
            }
            "Tf" => {
                text.font = operands
                    .first()
                    .and_then(|name| name.as_name().ok())
                    .and_then(|name| lookup(document, resources, b"Font", name))
                    .and_then(|object| object.as_reference().ok());
                text.size = value(1);
            }
            "Tc" => text.char_spacing = value(0),
            "Tw" => text.word_spacing = value(0),
            "Tz" => text.scale = value(0) / 100.0,
            "TL" => text.leading = value(0),
            "Ts" => text.rise = value(0),
            "Td" => text.move_line(value(0), value(1)),
            "TD" => {
                text.leading = -value(1);
                text.move_line(value(0), value(1));
            }
            "Tm" if operands.len() == 6 => {
                text.line_matrix = six();
                text.matrix = text.line_matrix;
            }
            "T*" => text.move_line(0.0, -text.leading),
            "Tj" | "'" | "\"" => {
                if operation.operator != "Tj" {
                    text.move_line(0.0, -text.leading);
                }
                if let Some(Object::String(bytes, _)) = operands.last() {
                    show(
                        document,
                        cache,
                        &mut text,
                        ctm,
                        &[ShowItem::Bytes(bytes)],
                        styles,
                        scope.origin,
                    );
                }
            }
            "TJ" => {
                if let Some(Ok(array)) = operands.first().map(Object::as_array) {
                    let items: Vec<ShowItem<'_>> = array
                        .iter()
                        .filter_map(|object| match object {
                            Object::String(bytes, _) => Some(ShowItem::Bytes(bytes)),
                            other => number(other).map(ShowItem::Adjust),
                        })
                        .collect();
                    show(
                        document,
                        cache,
                        &mut text,
                        ctm,
                        &items,
                        styles,
                        scope.origin,
                    );
                }
            }
            "Do" if depth < MAX_FORM_DEPTH => {
                let Some(form) = operands
                    .first()
                    .and_then(|name| name.as_name().ok())
                    .and_then(|name| lookup(document, resources, b"XObject", name))
                    .and_then(|object| document.dereference(object).ok())
                    .and_then(|(_, object)| object.as_stream().ok())
                else {
                    continue;
                };
                if form.dict.get(b"Subtype").and_then(Object::as_name).ok() != Some(&b"Form"[..]) {
                    continue;
                }
                let matrix = form
                    .dict
                    .get(b"Matrix")
                    .and_then(Object::as_array)
                    .ok()
                    .map(|array| array.iter().filter_map(number).collect::<Vec<f32>>())
                    .and_then(|values| <[f32; 6]>::try_from(values).ok())
                    .unwrap_or(IDENTITY);
                let mut inner = form
                    .dict
                    .get(b"Resources")
                    .ok()
                    .and_then(|object| document.dereference(object).ok())
                    .and_then(|(_, object)| object.as_dict().ok())
                    .map(|dictionary| vec![dictionary.clone()])
                    .unwrap_or_default();
                inner.extend(resources.iter().cloned());
                let bytes = form
                    .decompressed_content()
                    .unwrap_or_else(|_| form.content.clone());
                interpret(
                    scope,
                    &bytes,
                    &inner,
                    multiply(matrix, ctm),
                    depth + 1,
                    cache,
                    styles,
                    budget,
                );
            }
            _ => {}
        }
    }
}

enum ShowItem<'a> {
    Bytes(&'a [u8]),
    /// A `TJ` kerning adjustment in thousandths of an em.
    Adjust(f32),
}

fn show(
    document: &Document,
    cache: &mut HashMap<ObjectId, FontInfo>,
    text: &mut TextState,
    ctm: Matrix,
    items: &[ShowItem<'_>],
    styles: &mut PageStyles,
    origin: [f32; 2],
) {
    let Some(font_id) = text.font else {
        return;
    };
    let font = cache
        .entry(font_id)
        .or_insert_with(|| FontInfo::read(document, font_id));
    let start = multiply(translation(0.0, text.rise), multiply(text.matrix, ctm));
    let mut decoded = String::new();
    let mut advance = 0.0_f32;
    for item in items {
        match item {
            ShowItem::Adjust(amount) => {
                let shift = -amount / 1000.0 * text.size * text.scale;
                advance += shift;
                if *amount < -200.0 && !decoded.ends_with(' ') {
                    decoded.push(' ');
                }
            }
            ShowItem::Bytes(bytes) => {
                for code in font.codes(bytes) {
                    let width = font.width(code) / 1000.0 * text.size;
                    let spacing = text.char_spacing
                        + if code == 32 && !font.composite {
                            text.word_spacing
                        } else {
                            0.0
                        };
                    advance += (width + spacing) * text.scale;
                    if let Some(character) = font.character(code) {
                        decoded.push_str(&character);
                    }
                }
            }
        }
    }
    let scale_y = start[2].hypot(start[3]);
    let scale_x = start[0].hypot(start[1]);
    let size = text.size * scale_y;
    text.matrix = multiply(translation(advance, 0.0), text.matrix);
    if size <= 0.0 || !size.is_finite() {
        return;
    }
    styles.runs.push(Run {
        x: start[4] - origin[0],
        right: start[4] - origin[0] + advance * scale_x,
        baseline: origin[1] - start[5],
        size,
        bold: font.bold,
        italic: font.italic,
        font: font_id.0,
        letter_width: font.letter_width(),
        text: decoded,
    });
}

/// What the interpreter needs to know about one font.
struct FontInfo {
    composite: bool,
    bold: bool,
    italic: bool,
    first_char: u32,
    widths: Vec<f32>,
    missing_width: f32,
    wide: HashMap<u32, f32>,
    default_wide: f32,
    to_unicode: HashMap<u32, String>,
}

impl FontInfo {
    fn read(document: &Document, id: ObjectId) -> Self {
        let mut info = Self {
            composite: false,
            bold: false,
            italic: false,
            first_char: 0,
            widths: Vec::new(),
            missing_width: 500.0,
            wide: HashMap::new(),
            default_wide: 1000.0,
            to_unicode: HashMap::new(),
        };
        let Ok(font) = document.get_dictionary(id) else {
            return info;
        };
        let resolve = |object: &Object| -> Option<Object> {
            document
                .dereference(object)
                .ok()
                .map(|(_, resolved)| resolved.clone())
        };
        let field = |dictionary: &Dictionary, key: &[u8]| -> Option<Object> {
            dictionary.get(key).ok().and_then(&resolve)
        };
        let base_font = field(font, b"BaseFont")
            .and_then(|object| {
                object
                    .as_name()
                    .ok()
                    .map(|name| String::from_utf8_lossy(name).into_owned())
            })
            .unwrap_or_default();
        let (bold, italic) = style_from_name(&base_font);
        info.bold = bold;
        info.italic = italic;
        info.composite = field(font, b"Subtype")
            .and_then(|object| object.as_name().ok().map(<[u8]>::to_vec))
            .is_some_and(|name| name == b"Type0");
        let descriptor_owner = if info.composite {
            field(font, b"DescendantFonts")
                .and_then(|object| {
                    object
                        .as_array()
                        .ok()
                        .and_then(|array| array.first().cloned())
                })
                .and_then(|object| resolve(&object))
                .and_then(|object| object.as_dict().ok().cloned())
        } else {
            Some(font.clone())
        };
        if let Some(owner) = &descriptor_owner {
            if let Some(Ok(descriptor)) =
                field(owner, b"FontDescriptor").map(|object| object.as_dict().cloned())
            {
                let flags = field(&descriptor, b"Flags")
                    .and_then(|object| object.as_i64().ok())
                    .unwrap_or(0);
                // Bit 7 italic, bit 19 force-bold.
                info.italic |= flags & 0x40 != 0;
                info.bold |= flags & 0x4_0000 != 0;
                if let Some(weight) =
                    field(&descriptor, b"FontWeight").and_then(|object| number(&object))
                {
                    info.bold |= weight >= 600.0;
                }
                if let Some(angle) =
                    field(&descriptor, b"ItalicAngle").and_then(|object| number(&object))
                {
                    info.italic |= angle.abs() > 1.0;
                }
                if let Some(width) =
                    field(&descriptor, b"MissingWidth").and_then(|object| number(&object))
                {
                    info.missing_width = width;
                }
            }
            if info.composite {
                info.default_wide = field(owner, b"DW")
                    .and_then(|object| number(&object))
                    .unwrap_or(1000.0);
                if let Some(Ok(array)) = field(owner, b"W").map(|object| object.as_array().cloned())
                {
                    info.wide = parse_wide_widths(&array, &resolve);
                }
            } else {
                info.first_char = field(owner, b"FirstChar")
                    .and_then(|object| object.as_i64().ok())
                    .and_then(|value| u32::try_from(value).ok())
                    .unwrap_or(0);
                if let Some(Ok(array)) =
                    field(owner, b"Widths").map(|object| object.as_array().cloned())
                {
                    info.widths = array
                        .iter()
                        .map(|object| {
                            resolve(object)
                                .and_then(|object| number(&object))
                                .unwrap_or(0.0)
                        })
                        .collect();
                }
            }
        }
        if let Some(stream) = font
            .get(b"ToUnicode")
            .ok()
            .and_then(&resolve)
            .and_then(|object| object.as_stream().ok().cloned())
        {
            let data = stream
                .decompressed_content()
                .unwrap_or_else(|_| stream.content.clone());
            info.to_unicode = parse_to_unicode(&data);
        }
        info
    }

    fn letter_width(&self) -> f32 {
        if self.composite {
            return 0.0;
        }
        let widths: Vec<f32> = b"etaoinshrl"
            .iter()
            .map(|letter| self.width(u32::from(*letter)))
            .collect();
        widths.iter().sum::<f32>() / widths.len() as f32
    }

    /// Character codes of a shown string: two bytes each for composite fonts.
    fn codes(&self, bytes: &[u8]) -> Vec<u32> {
        if self.composite {
            bytes
                .chunks(2)
                .map(|pair| {
                    pair.iter()
                        .fold(0_u32, |code, byte| (code << 8) | u32::from(*byte))
                })
                .collect()
        } else {
            bytes.iter().map(|byte| u32::from(*byte)).collect()
        }
    }

    fn width(&self, code: u32) -> f32 {
        if self.composite {
            return self.wide.get(&code).copied().unwrap_or(self.default_wide);
        }
        code.checked_sub(self.first_char)
            .and_then(|index| self.widths.get(index as usize))
            .copied()
            .filter(|width| *width > 0.0)
            .unwrap_or(self.missing_width)
    }

    /// The text of a code: from the font's ToUnicode map, else ASCII for simple fonts (the
    /// printable range is the same in nearly every simple-font encoding).
    fn character(&self, code: u32) -> Option<Cow<'_, str>> {
        if let Some(text) = self.to_unicode.get(&code) {
            return Some(Cow::Borrowed(text));
        }
        if !self.composite
            && (32..127).contains(&code)
            && let Some(character) = char::from_u32(code)
        {
            return Some(Cow::Owned(character.to_string()));
        }
        None
    }
}

/// Weight and slant spelled in a font name (`ABCDEF+Times-BoldItalic`, `CMBX12`, `NimbusRomNo9L-Medi`).
fn style_from_name(name: &str) -> (bool, bool) {
    // Drop the six-capital subset prefix (`ABCDEF+`), not other plus signs in the name.
    let name = match name.split_once('+') {
        Some((prefix, rest))
            if prefix.len() == 6 && prefix.bytes().all(|b| b.is_ascii_uppercase()) =>
        {
            rest
        }
        _ => name,
    }
    .to_ascii_lowercase();
    let compact: String = name.chars().filter(char::is_ascii_alphanumeric).collect();
    let bold = [
        "bold", "black", "heavy", "semibold", "demibold", "medi", "cmbx", "cmb10",
    ]
    .iter()
    .any(|word| compact.contains(word))
        || name
            .split(['-', ',', '_', ' '])
            .any(|part| matches!(part, "bd" | "bol" | "sb" | "hv" | "bc" | "bk"));
    let suffix = name
        .rsplit('.')
        .next()
        .filter(|_| name.contains('.'))
        .unwrap_or("");
    let (bold, italic_suffix) = (
        bold || suffix == "b" || suffix == "bi",
        suffix == "i" || suffix == "bi",
    );
    let italic = italic_suffix
        || ["italic", "oblique", "slanted", "cmti", "cmsl", "cmmi"]
            .iter()
            .any(|word| compact.contains(word))
        || name
            .split(['-', ',', '_', ' '])
            .any(|part| matches!(part, "it" | "ital" | "obl" | "kursiv"));
    (bold, italic)
}

fn parse_wide_widths(
    array: &[Object],
    resolve: &dyn Fn(&Object) -> Option<Object>,
) -> HashMap<u32, f32> {
    let mut widths = HashMap::new();
    let mut index = 0;
    while index < array.len() {
        let Some(first) = resolve(&array[index]).and_then(|object| object.as_i64().ok()) else {
            break;
        };
        let Ok(first) = u32::try_from(first) else {
            break;
        };
        match array.get(index + 1).and_then(resolve) {
            Some(Object::Array(list)) => {
                for (offset, width) in list.iter().enumerate() {
                    if let Some(width) = resolve(width).and_then(|object| number(&object)) {
                        widths.insert(first + offset as u32, width);
                    }
                }
                index += 2;
            }
            Some(last) => {
                let (Ok(last), Some(width)) = (
                    last.as_i64(),
                    array
                        .get(index + 2)
                        .and_then(resolve)
                        .and_then(|object| number(&object)),
                ) else {
                    break;
                };
                let last = u32::try_from(last)
                    .unwrap_or(first)
                    .min(first.saturating_add(65_535));
                for code in first..=last {
                    widths.insert(code, width);
                }
                index += 3;
            }
            None => break,
        }
    }
    widths
}

/// `bfchar` and `bfrange` entries of a ToUnicode CMap.
fn parse_to_unicode(data: &[u8]) -> HashMap<u32, String> {
    let text = String::from_utf8_lossy(data);
    let mut map = HashMap::new();
    let mut section = "";
    for line in text.lines() {
        let line = line.trim();
        if line.ends_with("beginbfchar") {
            section = "char";
        } else if line.ends_with("beginbfrange") {
            section = "range";
        } else if line.starts_with("end") {
            section = "";
        } else if section == "char" {
            let parts = hex_tokens(line);
            if let [source, target] = parts[..]
                && let Some(code) = hex_code(source)
            {
                map.insert(code, hex_text(target));
            }
        } else if section == "range" {
            let parts = hex_tokens(line);
            if let [low, high, target] = parts[..]
                && let (Some(low), Some(high)) = (hex_code(low), hex_code(high))
            {
                let start = hex_text(target);
                let base = start.chars().last().map_or(0, u32::from);
                let prefix: String = start
                    .chars()
                    .take(start.chars().count().saturating_sub(1))
                    .collect();
                for code in low..=high.min(low.saturating_add(65_535)) {
                    if let Some(character) = char::from_u32(base + (code - low)) {
                        map.insert(code, format!("{prefix}{character}"));
                    }
                }
            }
        }
    }
    map
}

fn hex_tokens(line: &str) -> Vec<&str> {
    line.split(['<', '>'])
        .map(str::trim)
        .filter(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .collect()
}

fn hex_code(token: &str) -> Option<u32> {
    u32::from_str_radix(token, 16).ok()
}

/// UTF-16BE text from a hex token.
fn hex_text(token: &str) -> String {
    let units: Vec<u16> = token
        .as_bytes()
        .chunks(4)
        .filter_map(|chunk| std::str::from_utf8(chunk).ok())
        .filter_map(|digits| u16::from_str_radix(digits, 16).ok())
        .collect();
    String::from_utf16_lossy(&units)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Prints the runs of one page: `PROCYON_PDF=file.pdf PROCYON_PAGE=2 cargo test -p
    /// fm-semantic-docling show_runs -- --ignored --nocapture`.
    #[test]
    #[ignore = "diagnostic"]
    fn show_runs() {
        let path = std::env::var("PROCYON_PDF").expect("PROCYON_PDF");
        let page: usize = std::env::var("PROCYON_PAGE")
            .expect("PROCYON_PAGE")
            .parse()
            .expect("number");
        let styles = page_styles(&std::fs::read(path).expect("read"));
        for run in &styles[page - 1].runs {
            println!(
                "{:6.1} {:6.1} base {:6.1} size {:4.1} {}{} {:?}",
                run.x,
                run.right,
                run.baseline,
                run.size,
                if run.bold { "B" } else { "-" },
                if run.italic { "I" } else { "-" },
                run.text.chars().take(50).collect::<String>()
            );
        }
    }

    /// Checks the runs line up with docling's word cells on real PDFs:
    /// `PROCYON_DUMP_DIR=... cargo test -p fm-semantic-docling calibrate -- --ignored --nocapture`.
    #[test]
    #[ignore = "needs a folder of real PDFs"]
    fn calibrate() {
        let dir = std::env::var("PROCYON_DUMP_DIR").expect("PROCYON_DUMP_DIR");
        let mut files: Vec<_> = std::fs::read_dir(dir)
            .expect("readable")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "pdf"))
            .collect();
        files.sort();
        for path in files {
            let bytes = std::fs::read(&path).expect("read");
            let styles = page_styles(&bytes);
            let words = docling_pdf::textparse::pdf_words(&bytes);
            let (mut total, mut hit, mut size_gap) = (0_usize, 0_usize, 0.0_f32);
            for (page, (width, height, cells)) in words.iter().enumerate() {
                let Some(style) = styles.get(page) else {
                    continue;
                };
                if (style.width - width).abs() > 1.0 || (style.height - height).abs() > 1.0 {
                    size_gap += 1.0;
                }
                for cell in cells {
                    total += 1;
                    if style.runs.iter().any(|run| {
                        run.x <= cell.l + 3.0
                            && run.right + 3.0 >= cell.r
                            && run.baseline >= cell.t - 1.0
                            && run.baseline <= cell.b + 2.0
                    }) {
                        hit += 1;
                    }
                }
            }
            println!(
                "{:60} words {total:6} matched {:5.1}%  pages with size mismatch {size_gap}",
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .chars()
                    .take(60)
                    .collect::<String>(),
                100.0 * hit as f32 / total.max(1) as f32
            );
        }
    }

    #[test]
    fn font_names_spell_weight_and_slant() {
        assert_eq!(style_from_name("ABCDEF+Times-BoldItalic"), (true, true));
        assert_eq!(style_from_name("CMBX12"), (true, false));
        assert_eq!(style_from_name("NimbusRomNo9L-Medi"), (true, false));
        assert_eq!(style_from_name("Bookman-Light"), (false, false));
        assert_eq!(style_from_name("Arial,Italic"), (false, true));
        assert_eq!(style_from_name("FNJMMI+AdvOT3b30f6db.B"), (true, false));
        assert_eq!(style_from_name("FNJMNK+AdvOT7d6df7ab.I"), (false, true));
    }

    #[test]
    fn to_unicode_maps_chars_and_ranges() {
        let cmap = b"1 beginbfchar\n<0003> <0020>\nendbfchar\n1 beginbfrange\n<0010> <0012> <0030>\nendbfrange";
        let map = parse_to_unicode(cmap);
        assert_eq!(map.get(&3).map(String::as_str), Some(" "));
        assert_eq!(map.get(&0x11).map(String::as_str), Some("1"));
    }
}
