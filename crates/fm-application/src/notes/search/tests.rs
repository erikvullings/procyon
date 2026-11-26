use std::fs;
use std::sync::Mutex;

use async_trait::async_trait;
use fm_domain::{Location, ProviderId};
use tokio_util::sync::CancellationToken;

use super::*;
use crate::notes::store::tests::{location_of, registry};

#[test]
fn query_terms_are_lowercase_unique_words_without_filler() {
    assert_eq!(
        query_terms("How do I replace the nginx Cert? nginx!"),
        ["replace", "nginx", "cert"]
    );
    assert_eq!(query_terms("x - !"), Vec::<String>::new());
    assert_eq!(query_terms("CSR openssl-req"), ["csr", "openssl", "req"]);
}

#[test]
fn a_title_match_beats_a_tag_which_beats_a_heading_which_beats_body_text() {
    let terms = query_terms("nginx");
    let title = score_note("t.md", "---\ntitle: nginx setup\n---\nbody", &terms).unwrap();
    let tag = score_note("g.md", "---\ntitle: Web\ntags: [nginx]\n---\nbody", &terms).unwrap();
    let heading = score_note("h.md", "# Web\n## Reload nginx\nbody", &terms).unwrap();
    let body = score_note("b.md", "# Web\nrun nginx -t first", &terms).unwrap();
    assert!(title.score > tag.score, "{title:?} {tag:?}");
    assert!(tag.score > heading.score, "{tag:?} {heading:?}");
    assert!(heading.score > body.score, "{heading:?} {body:?}");
    assert_eq!(score_note("n.md", "# Web\nnothing here", &terms), None);
}

#[test]
fn notes_matching_more_terms_rank_higher() {
    let terms = query_terms("nginx certificate");
    let both = score_note("a.md", "# Notes\nnginx and the certificate", &terms).unwrap();
    let one = score_note("b.md", "# Notes\nnginx nginx nginx nginx", &terms).unwrap();
    assert!(both.score > one.score, "{both:?} {one:?}");
}

#[test]
fn the_snippet_is_the_first_matching_body_line_bounded() {
    let terms = query_terms("certbot");
    let long = format!("# T\n\nintro\n  sudo certbot renew {}\n", "x".repeat(400));
    let scored = score_note("a.md", &long, &terms).unwrap();
    assert!(scored.snippet.starts_with("sudo certbot renew"));
    assert!(scored.snippet.chars().count() <= 201);
    assert_eq!(scored.title, "T");
    // A note matching only in its title shows its first body line.
    let titled = score_note("certbot.md", "# certbot\n\nRenewal steps.\n", &terms).unwrap();
    assert_eq!(titled.snippet, "Renewal steps.");
}

fn notes_folder() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    // Written in an order unlike both path order and rank order.
    fs::write(
        dir.path().join("m-csr.md"),
        "---\ntitle: Create a CSR\ntags: [openssl, tls]\n---\nThe certificate request.\n",
    )
    .unwrap();
    fs::create_dir(dir.path().join("sub")).unwrap();
    fs::write(
        dir.path().join("sub/b.md"),
        "# Web server\n## nginx reload\nok\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("a-misc.md"),
        "# Misc\nsomething about nginx\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("z-nginx.md"),
        "---\ntitle: Replace nginx TLS certificate\ntags: [nginx, tls]\n---\n# Replace\n\
         sudo cp fullchain.pem /etc/nginx/certificate.pem\n",
    )
    .unwrap();
    fs::write(dir.path().join("unrelated.md"), "# Groceries\nmilk\n").unwrap();
    dir
}

async fn store_for(dir: &tempfile::TempDir) -> NotesStore {
    NotesStore::open(&registry(), &location_of(dir.path()))
        .await
        .unwrap()
}

fn paths(hits: &[NoteHit]) -> Vec<&str> {
    hits.iter().map(|hit| hit.path.as_str()).collect()
}

#[tokio::test]
async fn the_text_search_ranks_across_folders_and_breaks_ties_by_path() {
    let dir = notes_folder();
    let store = store_for(&dir).await;
    let (strategy, hits) = search_notes(
        &store,
        None,
        "nginx certificate",
        10,
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(strategy, SearchStrategy::Text);
    assert_eq!(
        paths(&hits),
        ["z-nginx.md", "sub/b.md", "a-misc.md", "m-csr.md"]
    );
    assert_eq!(hits[0].title, "Replace nginx TLS certificate");
    assert_eq!(hits[0].tags, ["nginx", "tls"]);
    assert!(hits.windows(2).all(|pair| pair[0].score >= pair[1].score));

    let (_, limited) = search_notes(&store, None, "nginx", 2, &CancellationToken::new()).await;
    assert_eq!(limited.len(), 2);
    let (_, none) = search_notes(&store, None, "?? !!", 5, &CancellationToken::new()).await;
    assert!(none.is_empty());
}

struct FakeIndex {
    answer: Result<Vec<SemanticPassage>, SemanticUnavailable>,
    asked: Mutex<Vec<(String, usize)>>,
}

#[async_trait]
impl SemanticNotes for FakeIndex {
    async fn search(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SemanticPassage>, SemanticUnavailable> {
        self.asked.lock().unwrap().push((query.to_owned(), limit));
        self.answer.clone()
    }
}

fn index(answer: Result<Vec<SemanticPassage>, SemanticUnavailable>) -> FakeIndex {
    FakeIndex {
        answer,
        asked: Mutex::new(Vec::new()),
    }
}

fn passage(location: Location, excerpt: &str) -> SemanticPassage {
    SemanticPassage {
        location,
        excerpt: excerpt.to_owned(),
    }
}

#[tokio::test]
async fn semantic_hits_are_used_merged_per_note_and_kept_inside_the_folder() {
    let dir = notes_folder();
    let store = store_for(&dir).await;
    let root = store.root().clone();
    let outside = Location::new(
        ProviderId::new("local"),
        url::Url::from_file_path(std::env::temp_dir().join("elsewhere.md"))
            .unwrap()
            .to_string(),
    );
    let fake = index(Ok(vec![
        passage(root.join("m-csr.md").unwrap(), "first passage"),
        passage(outside, "outside"),
        passage(root.join("z-nginx.md").unwrap(), "second passage"),
        passage(root.join("m-csr.md").unwrap(), "same note again"),
    ]));

    let (strategy, hits) = search_notes(
        &store,
        Some(&fake),
        "certificate",
        50,
        &CancellationToken::new(),
    )
    .await;

    assert_eq!(strategy, SearchStrategy::Semantic);
    assert_eq!(paths(&hits), ["m-csr.md", "z-nginx.md"]);
    assert_eq!(hits[0].title, "Create a CSR");
    assert_eq!(hits[0].tags, ["openssl", "tls"]);
    assert_eq!(hits[0].snippet, "first passage");
    assert!(hits[0].score > hits[1].score);
    // The index is asked for more passages than notes, and never more than the cap allows.
    let asked = fake.asked.lock().unwrap();
    assert_eq!(asked[0].0, "certificate");
    assert!(asked[0].1 >= MAX_HITS);
}

#[tokio::test]
async fn an_unavailable_or_empty_index_falls_back_to_the_text_search() {
    let dir = notes_folder();
    let store = store_for(&dir).await;
    for fake in [index(Err(SemanticUnavailable)), index(Ok(Vec::new()))] {
        let (strategy, hits) =
            search_notes(&store, Some(&fake), "nginx", 10, &CancellationToken::new()).await;
        assert_eq!(strategy, SearchStrategy::Text);
        assert_eq!(hits[0].path, "z-nginx.md");
        assert_eq!(fake.asked.lock().unwrap().len(), 1);
    }
    // Hits that all fall outside the folder count as empty too.
    let stray = index(Ok(vec![passage(
        Location::new(ProviderId::new("local"), "file:///nowhere/x.md"),
        "x",
    )]));
    let (strategy, _) =
        search_notes(&store, Some(&stray), "nginx", 10, &CancellationToken::new()).await;
    assert_eq!(strategy, SearchStrategy::Text);
}

#[tokio::test]
async fn session_records_and_notes_can_be_searched_apart() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("nginx.md"),
        "# Renew nginx certificate\n\nRun certbot.\n",
    )
    .unwrap();
    fs::create_dir(dir.path().join("sessions")).unwrap();
    fs::write(
        dir.path().join("sessions/2026-11-20-certs-ab12cd34.md"),
        "---\ntitle: certs\nkind: session\n---\n# certs\n\nWe renewed the nginx certificate.\n",
    )
    .unwrap();
    let store = store_for(&dir).await;
    let search = |kind| {
        let store = store.clone();
        async move {
            let (_, hits) =
                search_notes_of_kind(&store, None, "nginx", 10, kind, &CancellationToken::new())
                    .await;
            hits.into_iter().map(|hit| hit.path).collect::<Vec<_>>()
        }
    };
    assert_eq!(
        search(Some(NoteKind::Session)).await,
        ["sessions/2026-11-20-certs-ab12cd34.md"]
    );
    assert_eq!(search(Some(NoteKind::Note)).await, ["nginx.md"]);
    assert_eq!(search(None).await.len(), 2);
    assert!(is_session_record("sessions/x.md"));
    assert!(!is_session_record("sessions.md"));
    assert!(!is_session_record("old-sessions/x.md"));
}
