//! Converts completed Tool work into canonical Tool results.
mod exec;
mod gate;
mod generated_image;
mod interaction;
mod local;
mod mcp;
mod mcp_state;
mod search;

use serde_json::Value;
use tokio::sync::mpsc;

use crate::{
    cursor::protocol::proto::agent::v1 as pb,
    model::{ToolCall, ToolImageReference, ToolResult},
    store::BlobId,
    Error, Result,
};

use super::runtime::now_ms;

pub(crate) use exec::{edit_failure, from_exec};
pub(crate) use generated_image::complete_generated_image;
pub(crate) use interaction::{complete_web_fetch, complete_web_search, from_interaction};
pub(crate) use local::{local, subagents_disabled, todo_items};
pub(crate) use mcp::failure as mcp_failure;
pub(crate) use search::complete as semble;

#[derive(Clone, Debug)]
pub struct ToolCompletion {
    result: ToolResult,
    tool_call: pb::ToolCall,
    read_image: Option<ReadImage>,
    mcp_images: Vec<ReadImage>,
}

#[derive(Clone, Debug)]
pub(crate) struct ReadImage {
    pub(crate) data: Vec<u8>,
    pub(crate) mime_type: String,
    pub(crate) path: String,
}

impl ToolCompletion {
    pub(crate) fn apply_mode_switch(&mut self, from: &str, to: &str, reminder: &str) {
        if let Some(pb::tool_call::Tool::SwitchModeToolCall(tool)) = self.tool_call.tool.as_mut() {
            if let Some(pb::switch_mode_result::Result::Success(success)) = tool
                .result
                .as_mut()
                .and_then(|result| result.result.as_mut())
            {
                success.from_mode_id = from.into();
                success.to_mode_id = to.into();
                self.result.content = format!("Mode switched from {from} to {to}.\n\n{reminder}");
            }
        }
    }

    pub(crate) fn with_hook_contexts(mut self, contexts: Vec<pb::HookAdditionalContext>) -> Self {
        for context in &contexts {
            if !context.content.is_empty() {
                self.result.content.push_str(&format!(
                    "\n\nHook context ({}):\n{}",
                    context.hook_event_name, context.content
                ));
            }
        }
        self.tool_call.hook_additional_contexts = contexts;
        self
    }
    pub fn result(&self) -> &ToolResult {
        &self.result
    }

    pub fn tool_call(&self) -> &pb::ToolCall {
        &self.tool_call
    }

    pub(crate) async fn render_read_media(&mut self) {
        let Some(pb::tool_call::Tool::ReadToolCall(tool)) = &self.tool_call.tool else {
            return;
        };
        let Some(pb::read_tool_result::Result::Success(success)) =
            tool.result.as_ref().and_then(|r| r.result.as_ref())
        else {
            return;
        };
        let Some(pb::read_tool_success::Output::Data(data)) = &success.output else {
            return;
        };
        let Some(kind) = crate::media::kind(data, "", &success.path) else {
            return;
        };
        match crate::media::render(data, kind).await {
            Ok(rendered) => {
                self.result
                    .content
                    .push_str(&format!("\n{}", rendered.summary));
                for (label, data) in rendered.images {
                    let path = format!("{}: {label}", success.path);
                    self.result
                        .content
                        .push_str(&format!("\nImage {}: {path}", self.mcp_images.len() + 1));
                    self.mcp_images.push(ReadImage {
                        data,
                        mime_type: "image/png".into(),
                        path,
                    });
                }
            }
            Err(error) => self.result.content.push_str(&format!(
                "\nMedia image conversion unavailable: {error}. Visual content was not attached."
            )),
        }
    }

    pub(super) fn with_read_image(mut self, image: Option<ReadImage>) -> Self {
        self.read_image = image;
        self
    }

    pub(crate) fn take_read_image(&mut self) -> Option<ReadImage> {
        self.read_image.take()
    }

    pub(super) fn with_mcp_images(mut self, images: Vec<ReadImage>) -> Self {
        self.mcp_images = images;
        self
    }

    pub(crate) fn take_mcp_images(&mut self) -> Vec<ReadImage> {
        std::mem::take(&mut self.mcp_images)
    }

    pub(crate) fn persist_mcp_image(&mut self, blob_id: &BlobId, image: &ReadImage) {
        self.result.images.push(ToolImageReference {
            blob_id: blob_id.to_base64(),
            mime_type: image.mime_type.clone(),
            path: image.path.clone(),
        });
    }

    pub(crate) fn persist_read_image(&mut self, blob_id: &BlobId, image: &ReadImage) -> Result<()> {
        self.result.image = Some(ToolImageReference {
            blob_id: blob_id.to_base64(),
            mime_type: image.mime_type.clone(),
            path: image.path.clone(),
        });
        let Some(pb::tool_call::Tool::ReadToolCall(call)) = self.tool_call.tool.as_mut() else {
            return Err(Error::Protocol(
                "Read image completion has no Read tool state".into(),
            ));
        };
        let Some(pb::read_tool_result::Result::Success(success)) = call
            .result
            .as_mut()
            .and_then(|result| result.result.as_mut())
        else {
            return Err(Error::Protocol(
                "Read image completion has no success state".into(),
            ));
        };
        success.output = Some(pb::read_tool_success::Output::DataBlobId(
            blob_id.as_bytes().to_vec(),
        ));
        Ok(())
    }

    pub(crate) fn new(
        call: &ToolCall,
        started_at_ms: u64,
        mut result: ToolResult,
        mut tool: pb::tool_call::Tool,
    ) -> Self {
        // Apply the model-visible size gate once, at the tool completion
        // boundary. Canonical history and every provider projection then
        // carry the same bounded result without reprocessing it.
        gate::tool_completion(&call.name, &mut tool, &mut result.content);
        Self {
            result,
            tool_call: pb::ToolCall {
                tool_call_id: Some(call.call_id.clone()),
                started_at_ms: Some(started_at_ms),
                completed_at_ms: Some(now_ms()),
                tool: Some(tool),
                hook_additional_contexts: Vec::new(),
            },
            read_image: None,
            mcp_images: Vec::new(),
        }
    }

    pub(super) fn from_rendered(
        call: &ToolCall,
        started_at_ms: u64,
        output: String,
        is_error: bool,
        rendered: pb::ToolCall,
    ) -> Result<Self> {
        let tool = rendered.tool.ok_or_else(|| {
            Error::Protocol(format!("tool {} has no Cursor representation", call.name))
        })?;
        Ok(Self::new(
            call,
            started_at_ms,
            ToolResult {
                call_id: call.call_id.clone(),
                content: output,
                is_error,
                image: None,
                images: Vec::new(),
            },
            tool,
        ))
    }
}

#[derive(Clone)]
pub struct ToolResultSender(mpsc::UnboundedSender<Result<ToolWorkEvent>>);
pub struct ToolResultReceiver(mpsc::UnboundedReceiver<Result<ToolWorkEvent>>);

pub enum ToolWorkEvent {
    Completed(Box<ToolCompletion>),
    Exec {
        call_id: String,
        message: Box<pb::AgentServerMessage>,
    },
}

pub fn tool_result_channel() -> (ToolResultSender, ToolResultReceiver) {
    let (sender, receiver) = mpsc::unbounded_channel();
    (ToolResultSender(sender), ToolResultReceiver(receiver))
}

impl ToolResultSender {
    pub fn send(&self, result: ToolCompletion) {
        let _ = self.0.send(Ok(ToolWorkEvent::Completed(Box::new(result))));
    }

    pub(crate) fn send_exec(&self, call_id: String, message: pb::AgentServerMessage) -> Result<()> {
        self.0
            .send(Ok(ToolWorkEvent::Exec {
                call_id,
                message: Box::new(message),
            }))
            .map_err(|_| Error::Cancelled)
    }

    pub fn send_error(&self, error: Error) {
        let _ = self.0.send(Err(error));
    }
}

impl ToolResultReceiver {
    pub async fn recv(&mut self) -> Option<Result<ToolWorkEvent>> {
        self.0.recv().await
    }
}

pub(super) fn prost_json(value: &prost_types::Value) -> Value {
    use prost_types::value::Kind;
    match value.kind.as_ref() {
        None | Some(Kind::NullValue(_)) => Value::Null,
        Some(Kind::NumberValue(value)) => serde_json::Number::from_f64(*value)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        Some(Kind::StringValue(value)) => Value::String(value.clone()),
        Some(Kind::BoolValue(value)) => Value::Bool(*value),
        Some(Kind::StructValue(value)) => Value::Object(
            value
                .fields
                .iter()
                .map(|(key, value)| (key.clone(), prost_json(value)))
                .collect(),
        ),
        Some(Kind::ListValue(value)) => Value::Array(value.values.iter().map(prost_json).collect()),
    }
}
