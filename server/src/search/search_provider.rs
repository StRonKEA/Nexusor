//! Implements the `SemanticSearch` tool adapter over the Semble search engine.
//! The engine is Cursor's own codebase index; only the wire name is ours.
use std::sync::Arc;

use semble_core::{ContentType, SearchEngine, SearchRequest, SembleConfig};
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::OnceCell;

use crate::{store::Store, Error, Result};

static ENGINE: OnceCell<Arc<SearchEngine>> = OnceCell::const_new();

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ContentSelection {
    #[default]
    Code,
    Docs,
    Config,
    All,
}

#[derive(Debug, Deserialize)]
struct SearchArguments {
    #[serde(default)]
    query: String,
    #[serde(default)]
    repo: String,
    #[serde(default)]
    symbol: Option<String>,
    #[serde(default)]
    file_path: Option<String>,
    #[serde(default)]
    _line: Option<usize>,
    #[serde(default = "default_top_k")]
    top_k: usize,
    #[serde(default = "default_snippet_lines")]
    max_snippet_lines: Option<usize>,
    #[serde(default)]
    content: ContentSelection,
}

pub(crate) async fn execute(
    tool_name: &str,
    arguments: Value,
    store: Option<Store>,
) -> std::result::Result<Value, String> {
    // `semblesearch` is the pre-audit wire name of `SemanticSearch`.
    // `deepsearch`, `gotodefinition`, `searchsymbols` extend the codebase navigation capabilities.
    if !matches!(
        tool_name,
        "semanticsearch" | "semblesearch" | "deepsearch" | "gotodefinition" | "searchsymbols"
    ) {
        return Err(format!("unsupported search tool: {tool_name}"));
    }
    let mut args: SearchArguments =
        serde_json::from_value(arguments).map_err(|error| error.to_string())?;

    // If query is empty but symbol or other fields are present
    if args.query.is_empty() {
        if let Some(ref sym) = args.symbol {
            args.query = sym.clone();
        } else if let Some(ref fp) = args.file_path {
            args.query = fp.clone();
        }
    }
    if args.repo.is_empty() {
        args.repo = ".".into();
    }

    let engine = engine(store).await.map_err(|error| error.to_string())?;

    if tool_name == "gotodefinition" {
        let repo_path = std::path::PathBuf::from(&args.repo);
        let symbol_name = args.query.clone();
        return tokio::task::spawn_blocking(move || {
            engine
                .goto_definition(&repo_path, &symbol_name)
                .map(|defs| serde_json::to_value(defs).unwrap_or(Value::Null))
        })
        .await
        .map_err(|error| format!("Semble definition worker failed: {error}"))?
        .map_err(|error| error.to_string());
    }

    if tool_name == "searchsymbols" {
        let repo_path = std::path::PathBuf::from(&args.repo);
        let filter = if args.query.is_empty() { None } else { Some(args.query.clone()) };
        let limit = args.top_k.max(10);
        return tokio::task::spawn_blocking(move || {
            engine
                .search_symbols(&repo_path, filter.as_deref(), limit)
                .map(|symbols| serde_json::to_value(symbols).unwrap_or(Value::Null))
        })
        .await
        .map_err(|error| format!("Semble symbol worker failed: {error}"))?
        .map_err(|error| error.to_string());
    }

    tokio::task::spawn_blocking(move || {
        engine
            .search(SearchRequest {
                query: args.query,
                repo: args.repo.into(),
                top_k: args.top_k,
                max_snippet_lines: args.max_snippet_lines,
                content: content(args.content),
            })
            .and_then(json_value)
    })
    .await
    .map_err(|error| format!("Semble search worker failed: {error}"))?
    .map_err(|error| error.to_string())
}

async fn engine(store: Option<Store>) -> Result<Arc<SearchEngine>> {
    ENGINE
        .get_or_try_init(|| async move {
            let builder = match store {
                Some(store) => crate::network::blocking_client_builder(&store).await?,
                None => reqwest::blocking::Client::builder().use_native_tls(),
            };
            tokio::task::spawn_blocking(move || {
                let client = builder.build()?;
                SearchEngine::load_default_with_client(SembleConfig::default(), &client)
                    .map(Arc::new)
                    .map_err(|error| Error::Config(format!("load Semble search engine: {error}")))
            })
            .await
            .map_err(|error| Error::Config(format!("load Semble search engine: {error}")))?
        })
        .await
        .cloned()
}

fn json_value(response: semble_core::SearchResponse) -> semble_core::Result<Value> {
    serde_json::to_value(response)
        .map_err(|error| semble_core::Error::Serialization(error.to_string()))
}

fn content(selection: ContentSelection) -> Vec<ContentType> {
    match selection {
        ContentSelection::Code => vec![ContentType::Code],
        ContentSelection::Docs => vec![ContentType::Docs],
        ContentSelection::Config => vec![ContentType::Config],
        ContentSelection::All => vec![ContentType::Code, ContentType::Docs, ContentType::Config],
    }
}

fn default_top_k() -> usize {
    5
}

fn default_snippet_lines() -> Option<usize> {
    Some(10)
}
