//! Minimal `PROPFIND` `multistatus` response parsing (RFC 4918 §9.1).
//!
//! Deliberately tolerant of namespace prefixes (`D:`, `d:`, `lp1:`, an
//! unprefixed default namespace, ...) by matching on the local (post-`:`)
//! element name only - real servers (Nextcloud/ownCloud/Apache `mod_dav`)
//! differ here, and this provider must work against any of them rather than
//! one specific vendor's XML style.

use chrono::{DateTime, Utc};
use quick_xml::events::Event;
use quick_xml::name::QName;
use quick_xml::reader::Reader;

/// One `<D:response>` entry from a `PROPFIND` result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DavEntry {
    /// The entry's `href`, still percent-encoded and server-relative.
    pub href: String,
    /// Whether `resourcetype` contained a `collection` element.
    pub is_collection: bool,
    /// `getcontentlength`, when present (never present for collections).
    pub content_length: Option<u64>,
    /// `getlastmodified`, parsed from its RFC 1123 text, when present and
    /// well-formed.
    pub last_modified: Option<DateTime<Utc>>,
}

/// A malformed or unparseable `multistatus` response body.
#[derive(Debug, Clone, thiserror::Error)]
#[error("malformed WebDAV multistatus response: {0}")]
pub(crate) struct XmlError(String);

fn local_name(name: QName<'_>) -> String {
    let raw = name.as_ref();
    let text = String::from_utf8_lossy(raw);
    text.rsplit_once(':')
        .map_or_else(|| text.to_string(), |(_, local)| local.to_owned())
        .to_ascii_lowercase()
}

/// Parses a `multistatus` response body into its `response` entries.
pub(crate) fn parse_multistatus(body: &[u8]) -> Result<Vec<DavEntry>, XmlError> {
    let mut reader = Reader::from_reader(body);
    reader.config_mut().trim_text(true);

    let mut entries = Vec::new();
    let mut current_href: Option<String> = None;
    let mut current_collection = false;
    let mut current_length: Option<u64> = None;
    let mut current_modified: Option<DateTime<Utc>> = None;
    let mut text_target: Option<&'static str> = None;
    let mut buffer = Vec::new();

    loop {
        match reader
            .read_event_into(&mut buffer)
            .map_err(|error| XmlError(error.to_string()))?
        {
            Event::Eof => break,
            Event::Start(tag) | Event::Empty(tag) => match local_name(tag.name()).as_str() {
                "response" => {
                    current_href = None;
                    current_collection = false;
                    current_length = None;
                    current_modified = None;
                }
                "collection" => current_collection = true,
                "href" => text_target = Some("href"),
                "getcontentlength" => text_target = Some("length"),
                "getlastmodified" => text_target = Some("modified"),
                _ => {}
            },
            Event::Text(text) => {
                if let Some(target) = text_target {
                    let decoded = text.decode().map_err(|error| XmlError(error.to_string()))?;
                    let value = quick_xml::escape::unescape(&decoded)
                        .map_err(|error| XmlError(error.to_string()))?
                        .into_owned();
                    match target {
                        "href" => current_href = Some(value),
                        "length" => current_length = value.trim().parse().ok(),
                        "modified" => {
                            current_modified = DateTime::parse_from_rfc2822(value.trim())
                                .ok()
                                .map(|dt| dt.with_timezone(&Utc));
                        }
                        _ => {}
                    }
                }
            }
            Event::End(tag) => {
                let name = local_name(tag.name());
                if matches!(
                    name.as_str(),
                    "href" | "getcontentlength" | "getlastmodified"
                ) {
                    text_target = None;
                }
                if name == "response" {
                    if let Some(href) = current_href.take() {
                        entries.push(DavEntry {
                            href,
                            is_collection: current_collection,
                            content_length: current_length.take(),
                            last_modified: current_modified.take(),
                        });
                    }
                    current_collection = false;
                }
            }
            _ => {}
        }
        buffer.clear();
    }

    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_directory_and_a_file_entry() {
        let body = br#"<?xml version="1.0" encoding="utf-8"?>
<D:multistatus xmlns:D="DAV:">
  <D:response>
    <D:href>/dav/files/erik/</D:href>
    <D:propstat>
      <D:prop>
        <D:resourcetype><D:collection/></D:resourcetype>
        <D:getlastmodified>Mon, 01 Jan 2024 00:00:00 GMT</D:getlastmodified>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
  <D:response>
    <D:href>/dav/files/erik/report.txt</D:href>
    <D:propstat>
      <D:prop>
        <D:resourcetype/>
        <D:getcontentlength>1234</D:getcontentlength>
        <D:getlastmodified>Mon, 01 Jan 2024 00:00:00 GMT</D:getlastmodified>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
</D:multistatus>"#;

        let entries = parse_multistatus(body).expect("must parse");
        assert_eq!(entries.len(), 2);
        assert!(entries[0].is_collection);
        assert_eq!(entries[0].href, "/dav/files/erik/");
        assert!(!entries[1].is_collection);
        assert_eq!(entries[1].content_length, Some(1234));
        assert!(entries[1].last_modified.is_some());
    }

    #[test]
    fn tolerates_an_unprefixed_default_namespace() {
        let body = br#"<multistatus xmlns="DAV:">
  <response>
    <href>/dav/x</href>
    <propstat>
      <prop><resourcetype/><getcontentlength>1</getcontentlength></prop>
      <status>HTTP/1.1 200 OK</status>
    </propstat>
  </response>
</multistatus>"#;
        let entries = parse_multistatus(body).expect("must parse");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].href, "/dav/x");
    }
}
