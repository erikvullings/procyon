//! PresentationML conversion.
//!
//! Slides are discovered from the part names `ppt/slides/slideN.xml` and
//! sorted **explicitly** by their numeric suffix. ZIP entries arrive in
//! whatever order the writer chose, and relationship parts are deliberately
//! not read (see [`crate::formats::package`]), so the numeric suffix is the
//! ordering evidence available - a slide whose name does not carry a number is
//! reported as an omission rather than given a guessed position.

use quick_xml::events::Event;

use crate::builder::{DocumentBuilder, UnitDraft};
use crate::formats::package::{self, Package, PackageError};
use crate::model::{Omission, Provenance, TopLevelBoundary, UnitKind};

/// Maximum bytes read from one slide part.
const MAX_SLIDE_PART_BYTES: u64 = 16 * 1024 * 1024;

/// Elements that delimit one shape's text.
const SHAPE_ELEMENTS: &[&str] = &["sp", "graphicFrame", "pic", "cxnSp"];

/// Slide part names paired with their slide number, sorted by number.
fn slide_parts(archive: &Package<'_>) -> (Vec<(u32, String)>, Vec<String>) {
    let mut numbered = Vec::new();
    let mut unnumbered = Vec::new();
    for name in package::part_names(archive) {
        let Some(rest) = name.strip_prefix("ppt/slides/slide") else {
            continue;
        };
        let Some(digits) = rest.strip_suffix(".xml") else {
            continue;
        };
        match digits.parse::<u32>() {
            Ok(number) => numbered.push((number, name)),
            Err(_) => unnumbered.push(name),
        }
    }
    numbered.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    unnumbered.sort();
    (numbered, unnumbered)
}

/// Converts a PPTX package into one unit per shape, slide by slide.
pub(crate) fn convert(
    builder: &mut DocumentBuilder<'_, '_>,
    archive: &mut Package<'_>,
) -> Result<(), PackageError> {
    let (slides, unnumbered) = slide_parts(archive);
    if slides.is_empty() && unnumbered.is_empty() {
        return Err(PackageError::Malformed(
            "the package contains no slide parts".to_owned(),
        ));
    }
    builder
        .tracker()
        .charge_items(slides.len() as u64)
        .map_err(PackageError::from)?;
    for name in &unnumbered {
        builder.omit(Omission::UnreadablePart {
            detail: format!("slide part '{name}' has no slide number and was not ordered"),
        });
    }

    for (number, name) in slides {
        builder.checkpoint().map_err(PackageError::from)?;
        if builder.is_saturated() {
            break;
        }
        let Some(part) = package::read_part(archive, &name, MAX_SLIDE_PART_BYTES)? else {
            builder.omit(Omission::UnreadablePart {
                detail: format!("slide part '{name}' disappeared from the package"),
            });
            continue;
        };
        convert_slide(builder, &part, number, &name)?;
    }
    Ok(())
}

fn convert_slide(
    builder: &mut DocumentBuilder<'_, '_>,
    part: &[u8],
    slide_number: u32,
    name: &str,
) -> Result<(), PackageError> {
    let mut reader = package::xml_reader(part);
    let mut buffer = Vec::new();
    let mut depth = 0_u32;
    let mut shape_index = 0_u32;
    let mut shape_stack: Vec<u32> = Vec::new();
    let mut text = String::new();

    loop {
        builder.checkpoint().map_err(PackageError::from)?;
        if builder.is_saturated() {
            break;
        }
        let event = reader.read_event_into(&mut buffer).map_err(|error| {
            PackageError::Malformed(format!("{name} is not well formed: {error}"))
        })?;
        package::inspect_event(&event, &mut depth, builder.tracker())?;
        match &event {
            Event::Eof => break,
            Event::Start(start) => {
                let local = package::local_name(start.name().as_ref());
                if SHAPE_ELEMENTS.contains(&local.as_str()) {
                    shape_stack.push(depth);
                }
            }
            Event::End(end) => {
                let local = package::local_name(end.name().as_ref());
                match local.as_str() {
                    "p" => text.push('\n'),
                    "tc" => text.push_str(" | "),
                    _ => {}
                }
                if SHAPE_ELEMENTS.contains(&local.as_str())
                    && shape_stack.pop().is_some()
                    && shape_stack.is_empty()
                {
                    emit_shape(builder, &mut text, slide_number, shape_index)?;
                    shape_index += 1;
                }
            }
            Event::Text(value) => {
                let decoded = value.decode().map_err(|error| {
                    PackageError::Malformed(format!("{name} has invalid text: {error}"))
                })?;
                if !decoded.trim().is_empty() {
                    text.push_str(&decoded);
                }
            }
            _ => {}
        }
        buffer.clear();
    }
    if !builder.is_saturated() {
        package::ensure_balanced(depth, name)?;
    }
    emit_shape(builder, &mut text, slide_number, shape_index)?;
    Ok(())
}

fn emit_shape(
    builder: &mut DocumentBuilder<'_, '_>,
    text: &mut String,
    slide_number: u32,
    shape_index: u32,
) -> Result<(), PackageError> {
    let collected = std::mem::take(text);
    let normalized = collected
        .replace(" | \n", "\n")
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if normalized.is_empty() {
        return Ok(());
    }
    builder
        .push(UnitDraft {
            kind: UnitKind::Paragraph,
            section_path: Vec::new(),
            text: normalized,
            provenance: Provenance::Slide {
                slide_number,
                shape_index,
            },
            boundary: TopLevelBoundary::Slide(slide_number),
            source_offset: None,
        })
        .map_err(PackageError::from)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::budget::{BudgetTracker, ConversionBudgets, ManualClock};
    use crate::cancellation::Cancellation;
    use crate::formats::package::tests::package;
    use crate::model::{ComponentVersion, ConvertedDocument, FormatKind};

    fn slide(title: &str, body: &str) -> Vec<u8> {
        format!(
            r#"<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
  <p:cSld><p:spTree>
    <p:sp><p:txBody><a:p><a:r><a:t>{title}</a:t></a:r></a:p></p:txBody></p:sp>
    <p:sp><p:txBody><a:p><a:r><a:t>{body}</a:t></a:r></a:p></p:txBody></p:sp>
  </p:spTree></p:cSld>
</p:sld>"#
        )
        .into_bytes()
    }

    fn presentation(entries: Vec<(String, Vec<u8>)>) -> Vec<u8> {
        let mut all: Vec<(String, Vec<u8>)> = vec![(
            "ppt/presentation.xml".to_owned(),
            b"<p:presentation/>".to_vec(),
        )];
        all.extend(entries);
        let borrowed: Vec<(&str, &[u8])> = all
            .iter()
            .map(|(name, data)| (name.as_str(), data.as_slice()))
            .collect();
        package(&borrowed)
    }

    fn convert_package(bytes: &[u8]) -> Result<ConvertedDocument, PackageError> {
        let budgets = ConversionBudgets::default();
        let cancellation = Cancellation::none();
        let clock = ManualClock::new();
        let mut tracker = BudgetTracker::new(&budgets, &cancellation, &clock);
        let mut archive = package::preflight(bytes, &mut tracker)?;
        let mut builder = DocumentBuilder::new(
            ComponentVersion::new("baseline", 1),
            FormatKind::Pptx,
            &mut tracker,
        );
        convert(&mut builder, &mut archive)?;
        Ok(builder.finish())
    }

    #[test]
    fn slides_are_sorted_numerically_even_when_the_archive_interleaves_them() {
        let bytes = presentation(vec![
            ("ppt/slides/slide10.xml".to_owned(), slide("Ten", "body 10")),
            ("ppt/slides/slide2.xml".to_owned(), slide("Two", "body 2")),
            ("ppt/slides/slide1.xml".to_owned(), slide("One", "body 1")),
        ]);
        let document = convert_package(&bytes).expect("conversion");
        let slides: Vec<u32> = document
            .units()
            .iter()
            .map(|unit| match unit.provenance {
                Provenance::Slide { slide_number, .. } => slide_number,
                ref other => panic!("unexpected provenance {other:?}"),
            })
            .collect();
        assert_eq!(slides, [1, 1, 2, 2, 10, 10]);
        assert_eq!(document.units()[0].text, "One");
        assert_eq!(document.units()[4].text, "Ten");
        assert_eq!(document.units()[4].boundary, TopLevelBoundary::Slide(10));
    }

    #[test]
    fn each_shape_becomes_its_own_unit_with_a_shape_index() {
        let bytes = presentation(vec![(
            "ppt/slides/slide1.xml".to_owned(),
            slide("Title", "Bullet"),
        )]);
        let document = convert_package(&bytes).expect("conversion");
        assert_eq!(
            document.units()[0].provenance,
            Provenance::Slide {
                slide_number: 1,
                shape_index: 0
            }
        );
        assert_eq!(
            document.units()[1].provenance,
            Provenance::Slide {
                slide_number: 1,
                shape_index: 1
            }
        );
    }

    #[test]
    fn the_item_budget_refuses_a_presentation_with_too_many_slides() {
        let bytes = presentation(vec![
            ("ppt/slides/slide1.xml".to_owned(), slide("One", "body 1")),
            ("ppt/slides/slide2.xml".to_owned(), slide("Two", "body 2")),
        ]);
        let budgets = ConversionBudgets {
            max_items: 1,
            ..ConversionBudgets::default()
        };
        let cancellation = Cancellation::none();
        let clock = ManualClock::new();
        let mut tracker = BudgetTracker::new(&budgets, &cancellation, &clock);
        let mut archive = package::preflight(&bytes, &mut tracker).expect("preflight");
        let mut builder = DocumentBuilder::new(
            ComponentVersion::new("baseline", 1),
            FormatKind::Pptx,
            &mut tracker,
        );
        assert!(matches!(
            convert(&mut builder, &mut archive),
            Err(PackageError::Stopped(crate::budget::Stop::OverBudget {
                kind: crate::budget::BudgetKind::Items,
                limit: 1
            }))
        ));
    }

    #[test]
    fn a_package_without_slides_is_malformed() {
        let bytes = presentation(Vec::new());
        assert!(matches!(
            convert_package(&bytes),
            Err(PackageError::Malformed(_))
        ));
    }

    #[test]
    fn an_unnumbered_slide_part_is_reported_as_an_omission() {
        let bytes = presentation(vec![
            ("ppt/slides/slide1.xml".to_owned(), slide("One", "body")),
            ("ppt/slides/slideX.xml".to_owned(), slide("X", "body")),
        ]);
        let document = convert_package(&bytes).expect("conversion");
        assert!(document.is_partial());
        assert!(matches!(
            document.omissions().first(),
            Some(Omission::UnreadablePart { .. })
        ));
    }
}
