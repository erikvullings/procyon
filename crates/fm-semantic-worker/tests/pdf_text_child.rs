//! Exercises an opt-in converter child using only generated PDF source bytes.

#![cfg(feature = "experimental-large-pdf")]

use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

use fm_semantic_conversion::{Provenance, VisualProvenance};
use fm_semantic_worker::pdf_text_child::{
    convert_spool, convert_spool_visual_page, convert_spool_with_visual_stage,
    stage_spool_visual_pages,
};
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

fn synthetic_visual_pdf(visual_pages: &[u32]) -> Vec<u8> {
    let mut pdf = Document::load_mem(&synthetic_pdf()).expect("load visual fixture");
    let jpeg = vec![
        0xff, 0xd8, 0xff, 0xc0, 0, 17, 8, 0, 1, 0, 2, 3, 1, 0x11, 0, 2, 0x11, 0, 3, 0x11, 0, 0xff,
        0xd9,
    ];
    let image = pdf.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Image", "Width" => 2,
            "Height" => 1, "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => 8, "Filter" => "DCTDecode",
        },
        jpeg.clone(),
    ));
    for number in visual_pages {
        let page = pdf.get_pages()[number];
        let parent = pdf
            .get_object(page)
            .expect("page")
            .as_dict()
            .expect("page dictionary")
            .get(b"Parent")
            .expect("parent")
            .as_reference()
            .expect("parent reference");
        let resources = pdf
            .get_object(parent)
            .expect("parent")
            .as_dict()
            .expect("parent dictionary")
            .get(b"Resources")
            .expect("resources")
            .as_reference()
            .expect("resources reference");
        let page_resources = pdf.add_object(pdf.get_object(resources).expect("resources").clone());
        pdf.get_object_mut(page)
            .expect("page")
            .as_dict_mut()
            .expect("page dictionary")
            .set("Resources", page_resources);
        pdf.add_xobject(page, b"Figure", image).expect("add figure");
        let content_id = pdf.get_page_contents(page)[0];
        let content = pdf
            .get_object_mut(content_id)
            .expect("page content")
            .as_stream_mut()
            .expect("content stream");
        let mut drawing = content.get_plain_content().expect("page content bytes");
        drawing.extend_from_slice(b"q /Figure Do Q\n");
        content.set_plain_content(drawing);
    }
    let mut result = Vec::new();
    pdf.save_to(&mut result).expect("save visual fixture");
    result
}

#[cfg(not(feature = "experimental-pdf-render"))]
#[test]
fn visual_stage_rejects_unrendered_inline_image_on_text_page() {
    let mut pdf = Document::load_mem(&synthetic_pdf()).expect("synthetic PDF");
    let page = pdf.get_pages()[&2];
    let content_id = pdf.get_page_contents(page)[0];
    let stream = pdf
        .get_object_mut(content_id)
        .expect("page content")
        .as_stream_mut()
        .expect("content stream");
    let mut content = stream.get_plain_content().expect("content bytes");
    content.extend_from_slice(b"q BI /W 2 /H 1 /CS /RGB /BPC 8 ID abcdef EI Q\n");
    stream.set_plain_content(content);
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes).expect("save inline-image PDF");
    let digest = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let mut spool = tempfile::tempfile().expect("anonymous PDF spool");
    spool.write_all(&bytes).expect("source bytes");
    let error = stage_spool_visual_pages(
        Path::new(env!("CARGO_BIN_EXE_fm-semantic-worker")),
        &spool,
        &digest,
        &CancellationToken::new(),
    )
    .expect_err("inline image needs an approved renderer");
    assert!(
        error
            .to_string()
            .contains("PDF page 2: unsupported inline image")
    );
}

#[cfg(feature = "experimental-pdf-render")]
fn installed_renderer_worker() -> (tempfile::TempDir, std::path::PathBuf) {
    let library = std::env::var_os("PROCYON_TEST_PDFIUM_LIBRARY")
        .expect("set PROCYON_TEST_PDFIUM_LIBRARY to a verified PDFium library");
    let directory = tempfile::tempdir().expect("isolated renderer install");
    let worker = directory.path().join(if cfg!(windows) {
        "fm-semantic-worker.exe"
    } else {
        "fm-semantic-worker"
    });
    std::fs::copy(env!("CARGO_BIN_EXE_fm-semantic-worker"), &worker).expect("copy worker");
    std::fs::copy(
        library,
        pdfium_render::prelude::Pdfium::pdfium_platform_library_name_at_path(directory.path()),
    )
    .expect("install verified local renderer beside worker");
    (directory, worker)
}

#[cfg(feature = "experimental-pdf-render")]
#[test]
fn renderer_rejects_absent_or_invalid_adjacent_library() {
    let mut pdf = Document::load_mem(&synthetic_pdf()).expect("synthetic PDF");
    let page = pdf.get_pages()[&2];
    let content_id = pdf.get_page_contents(page)[0];
    let stream = pdf
        .get_object_mut(content_id)
        .expect("page content")
        .as_stream_mut()
        .expect("content stream");
    let mut content = stream.get_plain_content().expect("content bytes");
    content.extend_from_slice(b"q BI /W 2 /H 1 /CS /RGB /BPC 8 ID abcdef EI Q\n");
    stream.set_plain_content(content);
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes).expect("save inline-image PDF");
    let digest = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let mut spool = tempfile::tempfile().expect("anonymous source spool");
    spool.write_all(&bytes).expect("source bytes");

    let directory = tempfile::tempdir().expect("isolated worker");
    let worker = directory.path().join(if cfg!(windows) {
        "fm-semantic-worker.exe"
    } else {
        "fm-semantic-worker"
    });
    std::fs::copy(env!("CARGO_BIN_EXE_fm-semantic-worker"), &worker).expect("copy worker");
    let cancellation = CancellationToken::new();
    let error = stage_spool_visual_pages(&worker, &spool, &digest, &cancellation)
        .expect_err("missing adjacent PDFium must fail closed on the visual page");
    assert!(
        error
            .to_string()
            .contains("PDF page 2: renderer unavailable")
    );

    let library =
        pdfium_render::prelude::Pdfium::pdfium_platform_library_name_at_path(directory.path());
    std::fs::write(&library, b"invalid PDFium library").expect("write invalid library");
    let error = stage_spool_visual_pages(&worker, &spool, &digest, &cancellation)
        .expect_err("invalid adjacent PDFium must fail closed on the visual page");
    assert!(
        error
            .to_string()
            .contains("PDF page 2: renderer unavailable")
    );
}

#[cfg(feature = "experimental-pdf-render")]
#[test]
#[ignore = "requires PROCYON_TEST_PDFIUM_LIBRARY pointing at a verified local PDFium library"]
fn supervised_renderer_cites_mixed_and_image_only_inline_pages() {
    let (_directory, worker) = installed_renderer_worker();
    let mut pdf = Document::load_mem(&synthetic_pdf()).expect("synthetic PDF");
    for page_number in [2, 3] {
        let page = pdf.get_pages()[&page_number];
        let content_id = pdf.get_page_contents(page)[0];
        let stream = pdf
            .get_object_mut(content_id)
            .expect("page content")
            .as_stream_mut()
            .expect("content stream");
        let mut content = if page_number == 2 {
            stream.get_plain_content().expect("text")
        } else {
            Vec::new()
        };
        content.extend_from_slice(
            b"q 150 0 0 150 72 500 cm BI /W 2 /H 1 /CS /RGB /BPC 8 ID \xff\x00\x00\x00\x00\xff EI Q\n",
        );
        stream.set_plain_content(content);
    }
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes).expect("save mixed PDF");
    let digest = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let mut spool = tempfile::tempfile().expect("anonymous source spool");
    spool.write_all(&bytes).expect("source");
    let mut staged = stage_spool_visual_pages(&worker, &spool, &digest, &CancellationToken::new())
        .expect("render missing images in supervised child");
    assert_eq!(staged.page_count(), 3);
    assert!(
        staged
            .next_page()
            .expect("text-only page")
            .unwrap()
            .is_empty()
    );
    for page in [2, 3] {
        let images = staged.next_page().expect("raster").expect("page");
        assert_eq!(images.len(), 1);
        assert_eq!(
            images[0].provenance,
            VisualProvenance::PdfPageRender { page_number: page }
        );
        assert_eq!(images[0].media_type.as_str(), "image/jpeg");
        assert!(!images[0].data.is_empty());
        if page == 3 {
            let raster = image::load_from_memory(&images[0].data)
                .expect("rendered JPEG")
                .into_rgb8();
            assert!(
                raster
                    .pixels()
                    .any(|pixel| pixel[0] > 180 && pixel[1] < 100 && pixel[2] < 100),
                "image-only page should contain its red inline raster"
            );
        }
    }
    assert!(staged.next_page().expect("stage end").is_none());
    assert_eq!(
        convert_spool_with_visual_stage(
            &worker,
            &spool,
            &digest,
            &staged,
            &CancellationToken::new()
        )
        .expect("complete text with rendered pages")
        .units()
        .len(),
        2
    );
}

#[cfg(feature = "experimental-pdf-render")]
#[test]
#[ignore = "requires a generated 354-page RLE scan and verified local PDFium library"]
fn supervised_renderer_stages_every_large_non_reusable_scan_page() {
    use std::io::Read;

    let path = std::env::var_os("PROCYON_SYNTHETIC_RLE_SCAN_PDF")
        .map(std::path::PathBuf::from)
        .expect("set PROCYON_SYNTHETIC_RLE_SCAN_PDF to the generated fixture");
    assert_eq!(path.file_name().unwrap(), "synthetic-354-page-rle-scan.pdf");
    let target = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target")
        .canonicalize()
        .expect("workspace target");
    assert!(
        path.canonicalize()
            .expect("synthetic fixture")
            .starts_with(target),
        "the scan fixture must stay within this worktree's target directory"
    );
    let mut spool = std::fs::File::open(&path).expect("synthetic scan");
    let size = spool.metadata().expect("source size").len();
    assert!(size > 174_000_000 && size < 200 * 1024 * 1024);
    let mut hasher = Sha256::new();
    let mut chunk = [0_u8; 64 * 1024];
    loop {
        let count = spool.read(&mut chunk).expect("read source");
        if count == 0 {
            break;
        }
        hasher.update(&chunk[..count]);
    }
    let digest = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let (_directory, worker) = installed_renderer_worker();
    let started = std::time::Instant::now();
    let mut staged = stage_spool_visual_pages(&worker, &spool, &digest, &CancellationToken::new())
        .expect("supervised 354-page rendering");
    assert_eq!(staged.page_count(), 354);
    let staged_bytes = staged.staged_bytes().unwrap();
    assert!(staged_bytes < 256 * 1024 * 1024);
    for page in 1..=354 {
        let images = staged.next_page().expect("bounded page").expect("page");
        assert_eq!(images.len(), 1);
        assert_eq!(
            images[0].provenance,
            VisualProvenance::PdfPageRender { page_number: page }
        );
    }
    assert!(staged.next_page().expect("end").is_none());
    assert!(
        convert_spool_with_visual_stage(
            &worker,
            &spool,
            &digest,
            &staged,
            &CancellationToken::new()
        )
        .expect("complete image-only scan")
        .units()
        .is_empty()
    );
    eprintln!(
        "Rendered 354 synthetic pages in {:?}; {staged_bytes} staged bytes",
        started.elapsed()
    );
}

#[cfg(feature = "experimental-pdf-render")]
#[test]
#[ignore = "requires a generated 354-page RLE scan and verified local PDFium library"]
fn supervised_renderer_cancels_an_in_progress_large_scan() {
    let (_directory, worker) = installed_renderer_worker();
    let source = std::env::var_os("PROCYON_SYNTHETIC_RLE_SCAN_PDF")
        .map(std::path::PathBuf::from)
        .expect("generated synthetic RLE scan");
    assert_eq!(
        source.file_name().unwrap(),
        "synthetic-354-page-rle-scan.pdf"
    );
    assert!(
        source.canonicalize().expect("synthetic scan").starts_with(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target")
                .canonicalize()
                .expect("workspace target")
        )
    );
    let spool = std::fs::File::open(source).expect("open synthetic source");
    let cancellation = CancellationToken::new();
    let signal = cancellation.clone();
    let start = std::time::Instant::now();
    let attempt = std::thread::spawn(move || {
        stage_spool_visual_pages(
            &worker,
            &spool,
            "f2e8e12f962f43b5e384dbb8a2648c8ce88213a9895bd11bb5e6fc7b350d3964",
            &signal,
        )
    });
    std::thread::sleep(std::time::Duration::from_millis(500));
    cancellation.cancel();
    assert!(attempt.join().expect("stage thread").is_err());
    assert!(
        start.elapsed() < std::time::Duration::from_secs(10),
        "cancelled rendering should not wait for all 354 pages"
    );
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
fn anonymous_pdf_spool_child_returns_one_bounded_cited_image_page() {
    let bytes = synthetic_visual_pdf(&[2]);
    let mut spool = tempfile::tempfile().unwrap();
    spool.write_all(&bytes).unwrap();
    let digest = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let worker = Path::new(env!("CARGO_BIN_EXE_fm-semantic-worker"));
    let image = convert_spool_visual_page(worker, &spool, &digest, 2, &CancellationToken::new())
        .expect("page image");
    assert_eq!(
        image.provenance,
        VisualProvenance::PdfImage {
            page_number: 2,
            image_index: 0,
        }
    );
    assert_eq!(image.media_type.as_str(), "image/jpeg");
    assert_eq!(image.data.len(), 23);
    assert!(
        convert_spool_visual_page(
            worker,
            &spool,
            &"0".repeat(64),
            2,
            &CancellationToken::new()
        )
        .is_err()
    );
    assert!(
        convert_spool_visual_page(worker, &spool, &digest, 1, &CancellationToken::new()).is_err()
    );
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    assert!(convert_spool_visual_page(worker, &spool, &digest, 2, &cancelled).is_err());
}

#[test]
fn visual_pages_stage_only_complete_page_cited_images() {
    let worker = Path::new(env!("CARGO_BIN_EXE_fm-semantic-worker"));
    let cancellation = CancellationToken::new();
    let bytes = synthetic_visual_pdf(&[1, 2, 3]);
    let mut spool = tempfile::tempfile().expect("anonymous source spool");
    spool.write_all(&bytes).expect("source");
    let digest = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert!(stage_spool_visual_pages(worker, &spool, &"0".repeat(64), &cancellation).is_err());
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    assert!(stage_spool_visual_pages(worker, &spool, &digest, &cancelled).is_err());
    let mut staged =
        stage_spool_visual_pages(worker, &spool, &digest, &cancellation).expect("stage pages");
    assert_eq!(staged.page_count(), 3);
    assert_eq!(
        convert_spool_with_visual_stage(worker, &spool, &digest, &staged, &cancellation)
            .expect("same-source text")
            .units()
            .len(),
        3
    );
    assert!(
        convert_spool_with_visual_stage(worker, &spool, &"0".repeat(64), &staged, &cancellation)
            .is_err()
    );
    for page in 1..=3 {
        let visual = staged
            .next_page()
            .expect("image")
            .expect("page")
            .into_iter()
            .next()
            .expect("page evidence");
        assert_eq!(
            visual.provenance,
            VisualProvenance::PdfImage {
                page_number: page,
                image_index: 0,
            }
        );
        assert_eq!(visual.data.len(), 23);
    }
    assert!(staged.next_page().expect("end of stage").is_none());

    let bytes = synthetic_visual_pdf(&[2]);
    let mut incomplete = tempfile::tempfile().expect("anonymous incomplete source spool");
    incomplete.write_all(&bytes).expect("source");
    let digest = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let mut mixed =
        stage_spool_visual_pages(worker, &incomplete, &digest, &cancellation).expect("mixed pages");
    assert!(
        mixed
            .next_page()
            .expect("text-only page")
            .expect("page 1")
            .is_empty()
    );
    assert!(
        mixed
            .next_page()
            .expect("image page")
            .expect("page 2")
            .len()
            == 1
    );
    assert!(
        mixed
            .next_page()
            .expect("text-only page")
            .expect("page 3")
            .is_empty()
    );
    assert!(mixed.next_page().expect("end of stage").is_none());

    #[cfg(not(feature = "experimental-pdf-render"))]
    {
        let mut image_only_gap = Document::load_mem(&bytes).expect("synthetic PDF");
        let page = image_only_gap.get_pages()[&1];
        image_only_gap
            .get_object_mut(page)
            .expect("first page")
            .as_dict_mut()
            .expect("page dictionary")
            .remove(b"Contents");
        let mut bytes = Vec::new();
        image_only_gap
            .save_to(&mut bytes)
            .expect("save missing page");
        let mut gap = tempfile::tempfile().expect("anonymous incomplete source spool");
        gap.write_all(&bytes).expect("source");
        let digest = Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let error = stage_spool_visual_pages(worker, &gap, &digest, &cancellation)
            .expect_err("image-only page without extractable image");
        assert!(
            error
                .to_string()
                .contains("PDF page 1: no usable visual evidence"),
            "unexpected page failure: {error}"
        );
    }
}

#[test]
fn visual_stage_keeps_both_distinct_images_on_one_pdf_page() {
    let mut pdf = Document::load_mem(&synthetic_visual_pdf(&[2])).expect("synthetic PDF");
    let page = pdf.get_pages()[&2];
    let second = pdf.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Image", "Width" => 2,
            "Height" => 1, "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => 8, "Filter" => "DCTDecode",
        },
        vec![
            0xff, 0xd8, 0xff, 0xc0, 0, 17, 8, 0, 1, 0, 2, 3, 1, 0x11, 0, 2, 0x11, 0, 3, 0x11, 0,
            0xfe, 0xd9,
        ],
    ));
    pdf.add_xobject(page, b"Second", second)
        .expect("second image");
    let content_id = pdf.get_page_contents(page)[0];
    let content = pdf
        .get_object_mut(content_id)
        .expect("page content")
        .as_stream_mut()
        .expect("content stream");
    let mut drawing = content.get_plain_content().expect("content bytes");
    drawing.extend_from_slice(b"q /Second Do Q\n");
    content.set_plain_content(drawing);
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes).expect("save PDF");
    let digest = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let mut spool = tempfile::tempfile().expect("anonymous PDF spool");
    spool.write_all(&bytes).expect("spool PDF");
    let mut stage = stage_spool_visual_pages(
        Path::new(env!("CARGO_BIN_EXE_fm-semantic-worker")),
        &spool,
        &digest,
        &CancellationToken::new(),
    )
    .expect("complete multi-image stage");
    assert!(stage.next_page().expect("page 1").expect("page").is_empty());
    let figures = stage.next_page().expect("page 2").expect("page");
    assert_eq!(figures.len(), 2);
    assert_ne!(figures[0].id, figures[1].id);
    assert_eq!(
        figures
            .iter()
            .map(|image| image.provenance.clone())
            .collect::<Vec<_>>(),
        [
            VisualProvenance::PdfImage {
                page_number: 2,
                image_index: 0
            },
            VisualProvenance::PdfImage {
                page_number: 2,
                image_index: 1
            }
        ]
    );
    assert!(stage.next_page().expect("page 3").expect("page").is_empty());
    assert!(stage.next_page().expect("stage end").is_none());
}

#[test]
#[ignore = "requires the generated 174.2 MB synthetic scanned fixture"]
fn anonymous_spool_extracts_the_final_synthetic_scan_page() {
    let path = std::env::var_os("PROCYON_SYNTHETIC_SCAN_PDF")
        .map(std::path::PathBuf::from)
        .expect("set PROCYON_SYNTHETIC_SCAN_PDF to the generated scanned fixture");
    assert_eq!(path.file_name().unwrap(), "synthetic-354-page-scan.pdf");
    let mut source = std::fs::File::open(&path).unwrap();
    assert_eq!(source.metadata().unwrap().len(), 174_224_803);
    let mut spool = tempfile::tempfile().unwrap();
    std::io::copy(&mut source, &mut spool).unwrap();
    let image = convert_spool_visual_page(
        Path::new(env!("CARGO_BIN_EXE_fm-semantic-worker")),
        &spool,
        "d85243b5afaeb10f44e9f73f87424c656c53147d706e4a4643e2268c3bcb2e3e",
        354,
        &CancellationToken::new(),
    )
    .unwrap();
    assert_eq!(image.media_type.as_str(), "image/png");
    assert_eq!(
        image.provenance,
        VisualProvenance::PdfImage {
            page_number: 354,
            image_index: 0,
        }
    );
    assert_eq!((image.width, image.height), (512, 320));
    let mut staged = stage_spool_visual_pages(
        Path::new(env!("CARGO_BIN_EXE_fm-semantic-worker")),
        &spool,
        "d85243b5afaeb10f44e9f73f87424c656c53147d706e4a4643e2268c3bcb2e3e",
        &CancellationToken::new(),
    )
    .expect("all synthetic scan pages staged");
    assert_eq!(staged.page_count(), 354);
    eprintln!(
        "anonymous visual stage bytes: {}",
        staged.staged_bytes().unwrap()
    );
    assert!(
        convert_spool_with_visual_stage(
            Path::new(env!("CARGO_BIN_EXE_fm-semantic-worker")),
            &spool,
            "d85243b5afaeb10f44e9f73f87424c656c53147d706e4a4643e2268c3bcb2e3e",
            &staged,
            &CancellationToken::new()
        )
        .expect("complete image-only PDF")
        .units()
        .is_empty()
    );
    for page in 1..=354 {
        let visual = staged
            .next_page()
            .expect("bounded page")
            .expect("page")
            .into_iter()
            .next()
            .expect("image");
        assert_eq!(
            visual.provenance,
            VisualProvenance::PdfImage {
                page_number: page,
                image_index: 0,
            }
        );
        assert_eq!((visual.width, visual.height), (512, 320));
    }
    assert!(staged.next_page().expect("stage end").is_none());
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
