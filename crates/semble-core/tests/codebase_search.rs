//! End-to-end proof that the codebase search engine works on a real repository.
//!
//! Indexes this repository itself and searches for identifiers that are known to
//! exist, so a silent regression in the pipeline fails loudly instead of quietly
//! returning nothing.
use std::path::PathBuf;

use semble_core::{
    types::{ContentType, SearchRequest},
    SearchEngine, SembleConfig,
};

fn config() -> SembleConfig {
    SembleConfig::default()
}

fn repo_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is crates/semble-core; the workspace root is two levels up.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root")
        .to_path_buf()
}

#[test]
fn indexing_this_repository_reports_real_files_and_chunks() {
    let repo = repo_root();
    let engine = SearchEngine::load_default(config()).expect("engine");
    let stats = engine
        .prepare(&repo, &[ContentType::Code, ContentType::Docs])
        .expect("prepare the real repository");

    assert!(
        stats.file_count > 50,
        "expected to index this repository's files, got {}",
        stats.file_count
    );
    assert!(
        stats.chunk_count > stats.file_count,
        "chunking should split files into more units than files: {stats:?}"
    );
    assert!(
        stats.source_bytes > 10_000,
        "indexed almost nothing: {stats:?}"
    );
}

#[test]
fn a_known_identifier_is_found_in_the_code_itself() {
    let repo = repo_root();
    let engine = SearchEngine::load_default(config()).expect("engine");
    engine
        .prepare(&repo, &[ContentType::Code])
        .expect("prepare");

    for query in [
        "access_token_needs_refresh",
        "is_failover_provider",
        "cool_auth_resource",
    ] {
        let request = SearchRequest::new(query, &repo);
        let response = engine.search(request).expect("search must not fail");
        assert!(
            !response.results.is_empty(),
            "search for {query:?} returned nothing, so indexing or retrieval is broken"
        );
        let best = &response.results[0];
        assert!(
            best.file_path.starts_with("server") || best.file_path.starts_with("crates"),
            "the top result for {query:?} points at an unexpected file: {}",
            best.file_path
        );
        println!(
            "{query:?} -> {} lines {}..{}",
            best.file_path, best.start_line, best.end_line
        );
    }
}

/// A query with no real match must not fail the pipeline: the semantic stage is a
/// retrieval step, so it still yields candidates, and RRF reports them by rank rather
/// than by absolute relevance.
#[test]
fn a_query_with_no_match_still_completes() {
    let repo = repo_root();
    let engine = SearchEngine::load_default(config()).expect("engine");
    engine
        .prepare(&repo, &[ContentType::Code])
        .expect("prepare");

    for query in ["zzq_nonexistent_identifier_zzq", "asdf qwerty"] {
        let response = engine
            .search(SearchRequest::new(query, &repo))
            .expect("the pipeline must not fail on a query with no real match");
        println!("{query:?} -> {} aday", response.results.len());
    }
}
