//! Runs every bundled preview plugin (task 0142) against representative and hostile inputs,
//! through the real sandboxed runtime and the real manifests under `plugins/`.

use std::path::{Path, PathBuf};

use fm_plugin_api::{PluginManifest, PreviewOutput, PreviewOutputKind};
use fm_plugin_runtime::{
    PREVIEW_MAX_INPUT_BYTES, PluginCancellation, PluginRuntime, PreviewInput, discover_plugins,
};

fn plugins_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins")
}

fn plugin(id: &str) -> (PluginManifest, PathBuf) {
    let discovered = discover_plugins(&plugins_root())
        .into_iter()
        .find(|plugin| plugin.id() == id)
        .unwrap_or_else(|| panic!("bundled plugin {id} missing"));
    assert!(discovered.is_valid(), "{id}: {:?}", discovered.diagnostic);
    (discovered.manifest.expect("manifest"), discovered.directory)
}

fn render(id: &str, name: &str, content: &[u8], truncated: bool) -> PreviewOutput {
    let (manifest, directory) = plugin(id);
    let extension = name.rsplit_once('.').map_or("", |(_, ext)| ext);
    PluginRuntime::default()
        .render_preview(
            &manifest,
            &directory,
            &PreviewInput {
                name,
                extension,
                content,
                truncated,
            },
            &PluginCancellation::default(),
        )
        .unwrap_or_else(|error| panic!("{id} failed on {name}: {error}"))
}

fn bstr(value: &str) -> Vec<u8> {
    format!("{}:{value}", value.len()).into_bytes()
}

fn torrent(files: &[(&str, u64)]) -> Vec<u8> {
    let mut out = b"d8:announce".to_vec();
    out.extend(bstr("http://tracker.example/announce"));
    out.extend(b"7:comment");
    out.extend(bstr("a <b>bold</b> comment"));
    out.extend(b"13:creation datei1700000000e4:infod5:filesl");
    for (path, length) in files {
        out.extend(format!("d6:lengthi{length}e4:pathl").into_bytes());
        out.extend(bstr(path));
        out.extend(b"ee");
    }
    out.extend(b"e4:name");
    out.extend(bstr("My <Collection>"));
    out.extend(b"12:piece lengthi262144e6:pieces20:");
    out.extend([0xAB_u8; 20]);
    out.extend(b"ee");
    out
}

fn html(output: &PreviewOutput) -> &str {
    assert_eq!(output.kind, PreviewOutputKind::Html, "{}", output.body);
    &output.body
}

#[test]
fn every_bundled_preview_plugin_declares_a_valid_lua_contribution() {
    for id in [
        "procyon.preview-torrent",
        "procyon.preview-eml",
        "procyon.preview-vcard-ical",
    ] {
        let (manifest, _) = plugin(id);
        assert!(manifest.contributions.preview.is_some(), "{id}");
        assert!(manifest.permissions.selected_entry_content_read, "{id}");
    }
}

#[test]
fn torrent_preview_lists_metadata_and_escapes_untrusted_text() {
    let output = render(
        "procyon.preview-torrent",
        "a.torrent",
        &torrent(&[("docs/readme.txt", 2048), ("big.iso", 5 * 1024 * 1024)]),
        false,
    );
    let body = html(&output);
    assert!(body.contains("My &lt;Collection&gt;"), "{body}");
    assert!(!body.contains("<Collection>") && !body.contains("<b>bold"));
    assert!(body.contains("docs/readme.txt") && body.contains("big.iso"));
    assert!(body.contains("5.0 MiB"));
    assert!(body.contains("http://tracker.example/announce"));
    assert!(body.contains("2023-11-14"), "creation date: {body}");
}

#[test]
fn torrent_preview_survives_a_truncated_prefix() {
    let mut data = torrent(&[("a.bin", 10)]);
    data.truncate(data.len() - 15); // cut inside `pieces`
    let output = render("procyon.preview-torrent", "a.torrent", &data, true);
    assert!(html(&output).contains("a.bin"));
}

#[test]
fn torrent_preview_rejects_garbage_and_deep_nesting_without_failing_the_plugin() {
    for data in [b"not bencode".to_vec(), vec![b'l'; 100_000], Vec::new()] {
        let output = render("procyon.preview-torrent", "a.torrent", &data, false);
        assert_eq!(output.kind, PreviewOutputKind::Text);
    }
}

#[test]
fn torrent_preview_handles_invalid_utf8_names() {
    let mut data = b"d4:infod4:name3:".to_vec();
    data.extend([0xFF, 0xFE, b'x']);
    data.extend(b"6:lengthi3eee");
    let output = render("procyon.preview-torrent", "a.torrent", &data, false);
    assert!(html(&output).contains('x'));
}

const EML: &str = "From: =?UTF-8?B?w4lyaWM=?= <eric@example.com>\r\nTo: you@example.com\r\nSubject: =?utf-8?Q?Caf=C3=A9_<script>?=\r\nDate: Mon, 1 Jan 2026 10:00:00 +0000\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"XX\"\r\n\r\n--XX\r\nContent-Type: text/html; charset=utf-8\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\n<p onclick=3D\"x()\">Hello=20<b>there</b></p><img src=3D\"https://evil.test/p.gif\"><script>alert(1)</script>\r\n--XX\r\nContent-Type: application/pdf; name=\"report.pdf\"\r\nContent-Disposition: attachment; filename=\"report.pdf\"\r\nContent-Transfer-Encoding: base64\r\n\r\nJVBERi0=\r\n--XX--\r\n";

#[test]
fn eml_preview_decodes_headers_shows_text_only_and_lists_attachments() {
    let output = render("procyon.preview-eml", "m.eml", EML.as_bytes(), false);
    let body = html(&output);
    assert!(body.contains("Café &lt;script&gt;"), "{body}");
    assert!(body.contains("Éric"), "{body}");
    assert!(body.contains("report.pdf"), "{body}");
    assert!(body.contains("Hello there"), "{body}");
    for forbidden in ["<script", "<img", "onclick", "evil.test"] {
        assert!(!body.contains(forbidden), "{forbidden} leaked: {body}");
    }
}

#[test]
fn eml_preview_prefers_plain_text_and_rejects_non_mail() {
    let plain = "From: a@b.c\nSubject: Hi\n\nJust text & <more>";
    let body = render("procyon.preview-eml", "m.eml", plain.as_bytes(), false).body;
    assert!(body.contains("Just text &amp; &lt;more&gt;"), "{body}");
    let junk = render("procyon.preview-eml", "m.eml", &[0, 1, 2, 3], false);
    assert_eq!(junk.kind, PreviewOutputKind::Text);
}

#[test]
fn eml_preview_bounds_pathological_multipart_nesting() {
    let mut message =
        String::from("From: a@b.c\nSubject: Deep\nContent-Type: multipart/mixed; boundary=B\n\n");
    for _ in 0..2000 {
        message.push_str("--B\nContent-Type: multipart/mixed; boundary=B\n\n");
    }
    let output = render("procyon.preview-eml", "m.eml", message.as_bytes(), false);
    assert!(html(&output).contains("Deep"));
}

#[test]
fn vcard_and_ical_previews_render_items_and_escape_values() {
    let vcf = "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Ada <Lovelace>\r\nORG:Analytical\\, Inc.;Engines\r\nTEL;TYPE=WORK,VOICE:+31 20 123\r\nEMAIL;TYPE=INTERNET:ada@example.com\r\nNOTE:Line one\\nLine two\r\n continued\r\nBDAY:18151210\r\nEND:VCARD\r\n";
    let output = render("procyon.preview-vcard-ical", "c.vcf", vcf.as_bytes(), false);
    let body = html(&output);
    assert!(body.contains("Ada &lt;Lovelace&gt;"), "{body}");
    assert!(
        body.contains("work, voice") && body.contains("+31 20 123"),
        "{body}"
    );
    assert!(body.contains("Line one<br>Line twocontinued"), "{body}");
    assert!(body.contains("1815-12-10"), "{body}");

    let ics = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nSUMMARY:Stand-up\nDTSTART:20260315T093000Z\nDTEND:20260315T094500Z\nLOCATION:Room 1\\, Floor 2\nORGANIZER:mailto:boss@example.com\nEND:VEVENT\nBEGIN:VTODO\nSUMMARY:File taxes\nDUE:20260415\nEND:VTODO\nEND:VCALENDAR\n";
    let output = render("procyon.preview-vcard-ical", "e.ics", ics.as_bytes(), false);
    let body = html(&output);
    assert!(
        body.contains("Stand-up") && body.contains("2026-03-15 09:30 UTC"),
        "{body}"
    );
    assert!(
        body.contains("Room 1, Floor 2") && body.contains("boss@example.com"),
        "{body}"
    );
    assert!(
        body.contains("File taxes") && body.contains("2026-04-15"),
        "{body}"
    );
}

#[test]
fn vcard_and_ical_previews_cap_items_and_handle_empty_input() {
    let many: String = (0..150)
        .map(|i| format!("BEGIN:VCARD\nFN:Person {i}\nEND:VCARD\n"))
        .collect();
    let body = render(
        "procyon.preview-vcard-ical",
        "c.vcf",
        many.as_bytes(),
        false,
    )
    .body;
    assert!(body.contains("Person 99") && !body.contains("Person 100"));
    assert!(body.contains("and 50 more"), "{body}");
    let empty = render("procyon.preview-vcard-ical", "c.vcf", b"", false);
    assert_eq!(empty.kind, PreviewOutputKind::Text);
}

#[test]
fn previews_stay_within_budget_on_a_maximum_size_hostile_input() {
    let big = vec![b'a'; PREVIEW_MAX_INPUT_BYTES];
    for (id, name) in [
        ("procyon.preview-torrent", "a.torrent"),
        ("procyon.preview-eml", "a.eml"),
        ("procyon.preview-vcard-ical", "a.vcf"),
    ] {
        let (manifest, directory) = plugin(id);
        let extension = name.rsplit_once('.').map_or("", |(_, ext)| ext);
        let result = PluginRuntime::default().render_preview(
            &manifest,
            &directory,
            &PreviewInput {
                name,
                extension,
                content: &big,
                truncated: true,
            },
            &PluginCancellation::default(),
        );
        assert!(result.is_ok(), "{id}: {result:?}");
    }
}

#[test]
fn torrent_preview_tolerates_non_integer_and_hostile_numbers() {
    // A float-looking integer is malformed bencode: reported, not a plugin error.
    let float = b"d13:creation datei1.5e4:infod4:name1:xee";
    assert_eq!(
        render("procyon.preview-torrent", "a.torrent", float, false).kind,
        PreviewOutputKind::Text
    );
    // Huge integers decode to floats and must be ignored, and a string where a number belongs too.
    let huge = b"d13:creation datei99999999999999999999e4:infod6:lengthi99999999999999999999e4:name1:x12:piece length5:12345ee";
    let output = render("procyon.preview-torrent", "a.torrent", huge, false);
    let body = html(&output);
    assert!(
        body.contains("<h2>x</h2>") && !body.contains("Piece length"),
        "{body}"
    );
}

#[test]
fn eml_preview_joins_split_encoded_words_and_caps_escaped_output() {
    // "Rechnung é" with the two UTF-8 bytes of é split across two encoded words.
    let split =
        "From: a@b.c\nSubject: =?UTF-8?Q?Rech?= =?UTF-8?Q?nung_=C3?= =?UTF-8?Q?=A9?=\n\nbody";
    let body = render("procyon.preview-eml", "m.eml", split.as_bytes(), false).body;
    assert!(body.contains("Rechnung é"), "{body}");

    let huge = format!("From: a@b.c\nSubject: Big\n\n{}", "&".repeat(400_000));
    let output = render("procyon.preview-eml", "m.eml", huge.as_bytes(), true);
    assert!(html(&output).len() < 400_000);
}

#[test]
fn ical_preview_ignores_nested_alarm_components() {
    let ics = "BEGIN:VEVENT\nSUMMARY:Real title\nBEGIN:VALARM\nSUMMARY:Alarm title\nDESCRIPTION:Alarm text\nEND:VALARM\nLOCATION:Here\nEND:VEVENT\n";
    let body = render("procyon.preview-vcard-ical", "e.ics", ics.as_bytes(), false).body;
    assert!(
        body.contains("Real title") && body.contains("Here"),
        "{body}"
    );
    assert!(!body.contains("Alarm"), "{body}");
}
