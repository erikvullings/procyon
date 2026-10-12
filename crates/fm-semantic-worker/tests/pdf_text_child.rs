//! Exercises an opt-in converter child using only generated PDF source bytes.

#![cfg(feature = "experimental-large-pdf")]

use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

use fm_semantic_conversion::Provenance;
use fm_semantic_worker::pdf_text_child::convert_spool;
use lopdf::{Document, Object, Stream, dictionary};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

fn synthetic_pdf() -> Vec<u8> {
    let mut document = Document::with_version("1.5");
    let pages_id = document.new_object_id();
    let font_id = document.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    let resources_id = document.add_object(dictionary! {
        "Font" => dictionary! { "F1" => font_id },
    });
    let mut page_ids = Vec::new();
    for page in 1..=3 {
        let content = format!("BT /F1 12 Tf 72 720 Td (Synthetic page {page}) Tj ET\n");
        let content_id = document.add_object(Stream::new(dictionary! {}, content.into_bytes()));
        page_ids.push(document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        }));
    }
    let kids: Vec<Object> = page_ids.into_iter().map(Object::Reference).collect();
    document.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Count" => kids.len() as i64,
            "Kids" => kids,
            "Resources" => resources_id,
        }),
    );
    let catalog_id = document.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    document.trailer.set("Root", catalog_id);
    let mut bytes = Vec::new();
    document.save_to(&mut bytes).expect("save synthetic PDF");
    bytes
}

#[test]
fn anonymous_pdf_spool_child_round_trips_page_text_and_rejects_wrong_digest() {
    let bytes = synthetic_pdf();
    let mut spool = tempfile::tempfile().unwrap();
    spool.write_all(&bytes).unwrap();
    spool.seek(SeekFrom::End(0)).unwrap();
    let digest = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let worker = Path::new(env!("CARGO_BIN_EXE_fm-semantic-worker"));
    let converted = convert_spool(worker, &spool, &digest, &CancellationToken::new()).unwrap();
    assert!(!converted.is_partial());
    assert_eq!(converted.units().len(), 3);
    assert_eq!(
        converted
            .units()
            .iter()
            .map(|unit| unit.provenance.clone())
            .collect::<Vec<_>>(),
        [
            Provenance::PdfBlock {
                page_number: 1,
                block_index: 0
            },
            Provenance::PdfBlock {
                page_number: 2,
                block_index: 0
            },
            Provenance::PdfBlock {
                page_number: 3,
                block_index: 0
            },
        ]
    );
    assert!(converted.units()[2].text.contains("Synthetic page 3"));
    assert!(convert_spool(worker, &spool, &"0".repeat(64), &CancellationToken::new()).is_err());
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    assert!(convert_spool(worker, &spool, &digest, &cancelled).is_err());
}

#[test]
#[ignore = "requires the generated 188.9 MB synthetic fixture"]
fn anonymous_spool_converts_every_page_of_the_large_synthetic_text_pdf() {
    let path = std::env::var_os("PROCYON_SYNTHETIC_PDF")
        .map(std::path::PathBuf::from)
        .expect("set PROCYON_SYNTHETIC_PDF to the generated text fixture");
    assert_eq!(
        path.file_name().unwrap(),
        "synthetic-354-page-text.pdf",
        "only the generated fixture may be tested"
    );
    let mut source = std::fs::File::open(&path).unwrap();
    assert_eq!(source.metadata().unwrap().len(), 188_870_838);
    let mut spool = tempfile::tempfile().unwrap();
    std::io::copy(&mut source, &mut spool).unwrap();
    let expected_digest = "b1e36f4b3e4664d00e09475372f9e90858aaef0adfab3762aaa0ea98cc4d2285";
    let converted = convert_spool(
        Path::new(env!("CARGO_BIN_EXE_fm-semantic-worker")),
        &spool,
        expected_digest,
        &CancellationToken::new(),
    )
    .unwrap();
    assert!(!converted.is_partial());
    let pages = converted
        .units()
        .iter()
        .filter_map(|unit| match unit.provenance {
            Provenance::PdfBlock { page_number, .. } => Some(page_number),
            _ => None,
        })
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(pages.len(), 354);
    assert_eq!(pages.first(), Some(&1));
    assert_eq!(pages.last(), Some(&354));
    for unit in converted.units() {
        let Provenance::PdfBlock { page_number, .. } = unit.provenance else {
            panic!("missing PDF page citation");
        };
        assert!(
            unit.text
                .contains(&format!("Synthetic page {page_number} of 354:"))
        );
    }
}
