//! Dispatches `SearchConversations`.
//!
//! Unlike the message tools this one needs the conversation store, so it reads
//! Nexusor's own history rather than asking Cursor.
use crate::{model::ToolCall, store::Store, Result};

use crate::cursor::tools::tool_call_result::{self as result, ToolResultSender};

use super::ToolStart;

pub(super) fn start(
    results: &ToolResultSender,
    call: &ToolCall,
    store: Option<Store>,
) -> Result<ToolStart> {
    let call = call.clone();
    let results = results.clone();
    tokio::spawn(async move {
        match result::search_conversations(&call, store.as_ref()).await {
            Ok(completion) => results.send(completion),
            Err(error) => results.send_error(error),
        }
    });
    Ok(ToolStart {
        messages: Vec::new(),
        completion: None,
    })
}
