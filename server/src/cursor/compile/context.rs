//! Compiles rules, skills, MCP metadata, and environment context.
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::Path,
};

use prost::Message;
use serde_json::Value;

use crate::{
    cursor::{
        protocol::proto::agent::v1 as pb, services::context_sync::RequestContextSynchronizer,
        tools::runtime::McpRoute,
    },
    model::{normalize_tool_name, ToolDefinition},
    store::BlobId,
    Error, Result,
};

pub async fn hydrate(
    request: &pb::AgentRunRequest,
    context_sync: &RequestContextSynchronizer,
) -> Result<pb::RequestContext> {
    let mut context = request_context(request).cloned().unwrap_or_default();
    let Some(parts) = request
        .action
        .as_ref()
        .and_then(|action| action.request_context_parts.as_ref())
    else {
        if is_background_completion(request)
            || (matches!(
                request
                    .action
                    .as_ref()
                    .and_then(|action| action.action.as_ref()),
                Some(pb::conversation_action::Action::SubscriptionNotificationAction(_))
            ) && request_context(request).is_none())
        {
            return context_sync
                .load(request.conversation_id.as_deref().unwrap_or_default())
                .await;
        }
        return Ok(context);
    };

    if let Some(current) = context_sync
        .refresh_if_missing(
            parts,
            request.conversation_id.as_deref().unwrap_or_default(),
        )
        .await?
    {
        context.rules = current.rules;
        context.non_file_rules = current.non_file_rules;
        context.cloud_rule = current.cloud_rule;
        context.agent_skills = current.agent_skills;
        context.skill_options = current.skill_options;
        context.custom_subagents = current.custom_subagents;
        context.tools = current.tools;
        context.mcp_instructions = current.mcp_instructions;
        context.mcp_file_system_options = current.mcp_file_system_options;
        context.mcp_meta_tool_options = current.mcp_meta_tool_options;
        return Ok(context);
    }

    if let Some(part) = decode_part::<pb::RequestContextRulesPart>(
        "rules",
        &parts.rules_blob_id,
        parts.rules_byte_length,
        context_sync,
    )
    .await?
    {
        context.rules = part.rules;
        context.non_file_rules = part.non_file_rules;
        context.cloud_rule = part.cloud_rule;
    }
    if let Some(part) = decode_part::<pb::RequestContextSkillsPart>(
        "skills",
        &parts.skills_blob_id,
        parts.skills_byte_length,
        context_sync,
    )
    .await?
    {
        context.agent_skills = part.agent_skills;
        context.skill_options = part.skill_options;
    }
    if let Some(part) = decode_part::<pb::RequestContextSubagentsPart>(
        "subagents",
        &parts.subagents_blob_id,
        parts.subagents_byte_length,
        context_sync,
    )
    .await?
    {
        context.custom_subagents = part.custom_subagents;
    }
    if let Some(part) = decode_part::<pb::RequestContextMcpsPart>(
        "MCP",
        &parts.mcps_blob_id,
        parts.mcps_byte_length,
        context_sync,
    )
    .await?
    {
        context.tools = part.tools;
        context.mcp_instructions = part.mcp_instructions;
        context.mcp_file_system_options = part.mcp_file_system_options;
        context.mcp_meta_tool_options = part.mcp_meta_tool_options;
    }
    Ok(context)
}

fn is_background_completion(request: &pb::AgentRunRequest) -> bool {
    matches!(
        request
            .action
            .as_ref()
            .and_then(|action| action.action.as_ref()),
        Some(pb::conversation_action::Action::BackgroundTaskCompletionAction(_))
    )
}

async fn decode_part<T: Message + Default>(
    name: &str,
    raw_id: &[u8],
    expected_length: u32,
    context_sync: &RequestContextSynchronizer,
) -> Result<Option<T>> {
    if raw_id.is_empty() {
        if expected_length != 0 {
            return Err(Error::Protocol(format!(
                "{name} context has a byte length but no BlobID"
            )));
        }
        return Ok(None);
    }
    let id = BlobId::from_bytes(raw_id)?;
    let data = context_sync.get(&id).await?.ok_or_else(|| {
        Error::Protocol(format!(
            "{name} context Blob is missing: {}",
            id.to_base64()
        ))
    })?;
    if data.len() != expected_length as usize {
        return Err(Error::Protocol(format!(
            "{name} context Blob length mismatch: expected {expected_length}, got {}",
            data.len()
        )));
    }
    T::decode(data.as_slice())
        .map(Some)
        .map_err(|error| Error::Protocol(format!("invalid {name} context Blob: {error}")))
}

/// 把本地 md 规则目录(rules 服务的存储)合并进请求上下文,
/// 使 Nexusor 运行在 IDE 未携带这些规则时也能消费它们。
/// 与 IDE 已发规则按内容去重;读取失败只告警,不影响运行。
pub fn merge_local_rules(context: &mut pb::RequestContext, rules_dir: &Path) {
    let records = match crate::cursor::services::knowledge::RuleStore::open(rules_dir.into())
        .and_then(|store| store.list())
    {
        Ok(records) => records,
        Err(error) => {
            tracing::warn!(%error, "cannot read local rules; continuing without them");
            return;
        }
    };
    let existing = context
        .rules
        .iter()
        .chain(context.non_file_rules.iter())
        .map(|rule| rule.content.trim().to_owned())
        .chain(context.cloud_rule.iter().map(|rule| rule.trim().to_owned()))
        .collect::<HashSet<_>>();
    for record in records {
        if record.knowledge.trim().is_empty() || existing.contains(record.knowledge.trim()) {
            continue;
        }
        context.non_file_rules.push(pb::CursorRule {
            content: record.knowledge,
            ..Default::default()
        });
    }
}

pub fn request_context(request: &pb::AgentRunRequest) -> Option<&pb::RequestContext> {
    let action = request.action.as_ref()?;
    action
        .request_context_parts
        .as_ref()
        .and_then(|parts| parts.dynamic_context.as_ref())
        .or_else(|| match action.action.as_ref()? {
            pb::conversation_action::Action::UserMessageAction(action) => {
                action.request_context.as_ref()
            }
            pb::conversation_action::Action::ExecutePlanAction(action) => {
                action.request_context.as_ref()
            }
            pb::conversation_action::Action::SubscriptionNotificationAction(action) => {
                action.request_context.as_ref()
            }
            _ => None,
        })
}

pub fn compile_context(context: &pb::RequestContext, today: &str) -> String {
    let mut sections = Vec::new();
    if let Some(hooks) = context
        .hooks_additional_context
        .as_deref()
        .filter(|s| !s.is_empty())
    {
        sections.push(format!("<hook_context>\n{hooks}\n</hook_context>"));
    }
    let mut transcripts = None;
    if let Some(env) = &context.env {
        let workspace = env
            .workspace_paths
            .first()
            .map(String::as_str)
            .unwrap_or("");
        let repo = context.git_repos.iter().find(|repo| repo.path == workspace);
        sections.push(format!(
            "<user_info>\nOS Version: {}\n\nShell: {}\n\nWorkspace Path: {}\n\nIs directory a git repo: {}\n\nTerminals folder: {}\n\nToday's date: {}\n\nNote: Prefer using absolute paths over relative paths as tool call args when possible.\n</user_info>",
            env.os_version,
            env.shell,
            workspace,
            repo.map(|repo| format!("Yes, at {}", repo.path)).unwrap_or_else(|| "No".into()),
            env.terminals_folder,
            today,
        ));
        if !env.agent_transcripts_folder.is_empty() {
            transcripts = Some(format!(
                "<agent_transcripts>\nAgent transcripts (past chats) live in {}. They have names like <uuid>.jsonl, cite parent chat transcripts to the user as [<title for chat <=6 words>\n](<uuid excluding .jsonl>). Don't discuss the folder structure.\n</agent_transcripts>",
                env.agent_transcripts_folder
            ));
        }
    }
    sections.extend(context.git_repos.iter().map(|repo| {
        format!(
            "<git_status>\nThis is the git status at the start of the conversation. Note that this status is a snapshot in time, and will not update during the conversation.\n\n\nGit repo: {}\n\n```\n{}\n```\n</git_status>",
            repo.path, repo.status
        )
    }));
    sections.extend(transcripts);
    let skill_contents = context
        .agent_skills
        .iter()
        .map(|skill| skill.content.as_str())
        .filter(|content| !content.is_empty())
        .collect::<HashSet<_>>();
    let mut rules = context
        .rules
        .iter()
        .chain(context.non_file_rules.iter())
        .filter(|rule| {
            !rule.content.trim().is_empty()
                && !is_skill_rule(rule)
                && !skill_contents.contains(rule.content.as_str())
        })
        .map(|rule| format!("<user_rule>\n{}\n</user_rule>", rule.content))
        .collect::<Vec<_>>();
    rules.extend(
        context
            .cloud_rule
            .iter()
            .map(|rule| format!("<user_rule>\n{rule}\n</user_rule>")),
    );
    if !rules.is_empty() {
        sections.push(format!("<rules>\n{}\n</rules>", rules.join("\n")));
    }
    let skills = context
        .agent_skills
        .iter()
        .filter(|skill| !skill.disable_model_invocation)
        .map(|skill| {
            format!(
                "<agent_skill fullPath=\"{}\">{}</agent_skill>",
                xml(&skill.full_path),
                xml(&skill.description),
            )
        })
        .collect::<Vec<_>>();
    if !skills.is_empty() {
        sections.push(format!(
            "<agent_skills>\n<available_skills>\n{}\n</available_skills>\n</agent_skills>",
            skills.join("\n")
        ));
    }
    let subagents = context
        .custom_subagents
        .iter()
        .map(|agent| {
            format!(
                "<subagent name=\"{}\">{}</subagent>",
                xml(&agent.name),
                agent.description
            )
        })
        .collect::<Vec<_>>();
    if !subagents.is_empty() {
        sections.push(format!(
            "<subagents>\n{}\n</subagents>",
            subagents.join("\n")
        ));
    }
    {
        let servers = context
            .mcp_meta_tool_options
            .as_ref()
            .into_iter()
            .flat_map(|options| &options.mcp_descriptors)
            .filter_map(compile_mcp_descriptor)
            .collect::<Vec<_>>();
        if !servers.is_empty() {
            sections.push(format!(
                "<mcp_meta_tools>\nThe following MCP tools are available. Call a listed tool directly with CallMcpTool without calling GetMcpTools first. If a call returns an error, use it to correct the arguments or authentication and retry when appropriate.\n<mcp_meta_tool_servers>\n{}\n</mcp_meta_tool_servers>\n</mcp_meta_tools>",
                servers.join("\n")
            ));
        }
    }
    sections.join("\n\n")
}

fn compile_mcp_descriptor(server: &pb::McpDescriptor) -> Option<String> {
    if server.server_identifier.trim().is_empty() {
        return None;
    }
    let tools = server
        .tools
        .iter()
        .filter(|tool| !tool.tool_name.trim().is_empty())
        .map(|tool| {
            let mut lines = vec![format!("<mcp_tool name=\"{}\">", xml(&tool.tool_name))];
            if let Some(path) = tool
                .definition_path
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                lines.push(format!("<definition_path>{}</definition_path>", xml(path)));
            }
            if let Some(description) = tool
                .description
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                lines.push(format!("<description>{}</description>", xml(description)));
            }
            if let Some(schema) = mcp_input_schema(tool) {
                lines.push(format!("<input_schema>{}</input_schema>", xml(&schema)));
            }
            lines.push("</mcp_tool>".into());
            lines.join("\n")
        })
        .collect::<Vec<_>>();
    if tools.is_empty() {
        return None;
    }
    Some(format!(
        "<mcp_meta_tool_server name=\"{}\" identifier=\"{}\">\n<tools>\n{}\n</tools>\n</mcp_meta_tool_server>",
        xml(if server.server_name.trim().is_empty() {
            &server.server_identifier
        } else {
            &server.server_name
        }),
        xml(&server.server_identifier),
        tools.join("\n"),
    ))
}

fn mcp_input_schema(tool: &pb::McpToolDescriptor) -> Option<String> {
    tool.input_schema_json
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(|value| {
            serde_json::from_str::<Value>(value)
                .map(|value| value.to_string())
                .unwrap_or_else(|_| value.to_string())
        })
        .or_else(|| {
            tool.input_schema
                .as_ref()
                .map(prost_value)
                .map(|value| value.to_string())
        })
}

pub fn meta_mcp_routes(context: &pb::RequestContext) -> HashMap<(String, String), McpRoute> {
    context
        .mcp_meta_tool_options
        .as_ref()
        .into_iter()
        .flat_map(|options| &options.mcp_descriptors)
        .filter(|server| !server.server_identifier.trim().is_empty())
        .flat_map(|server| {
            server.tools.iter().filter_map(move |tool| {
                if tool.tool_name.trim().is_empty() {
                    return None;
                }
                let provider_identifier = if server.server_name.trim().is_empty() {
                    server.server_identifier.clone()
                } else {
                    server.server_name.clone()
                };
                Some((
                    (server.server_identifier.clone(), tool.tool_name.clone()),
                    McpRoute {
                        name: format!("{}-{}", server.server_identifier, tool.tool_name),
                        provider_identifier,
                        tool_name: tool.tool_name.clone(),
                        description: tool.description.clone().unwrap_or_default(),
                    },
                ))
            })
        })
        .collect()
}

fn is_skill_rule(rule: &pb::CursorRule) -> bool {
    Path::new(&rule.full_path)
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("SKILL.md"))
}

pub async fn selected_context(
    user: &pb::UserMessage,
    blobs: &crate::cursor::services::blob_sync::BlobSynchronizer,
) -> Result<Option<String>> {
    let Some(selected) = user.selected_context.as_ref() else {
        return Ok(None);
    };
    let mut sections = selected.extra_context.clone();
    for folder in &selected.folders {
        let mut tree = Vec::new();
        let mut pending = folder.directory_tree.iter().collect::<Vec<_>>();
        while let Some(node) = pending.pop() {
            tree.push(format!(
                "{} ({} files, children processed: {})",
                node.abs_path, node.num_files, node.children_were_processed
            ));
            tree.extend(
                node.children_files
                    .iter()
                    .map(|file| format!("  {}", file.name)),
            );
            pending.extend(node.children_dirs.iter().rev());
        }
        sections.push(format!(
            "<folder path=\"{}\">\n{}\n</folder>",
            xml(&folder.path),
            tree.join("\n")
        ));
    }
    for selection in &selected.git_pr_diff_selections {
        let content = match (&selection.diff_content, &selection.blob_id) {
            (Some(text), _) => text.clone(),
            (None, Some(id)) => selected_blob_text(id, blobs).await?,
            (None, None) => String::new(),
        };
        sections.push(format!(
            "<pr_diff url=\"{}\" path=\"{}\" start_line=\"{}\" end_line=\"{}\">\n{}\n</pr_diff>",
            xml(&selection.pr_url),
            xml(&selection.file_path),
            selection.start_line,
            selection.end_line,
            content
        ));
    }
    for pr in &selected.selected_pull_requests {
        let summary = match (&pr.summary_json, &pr.blob_id) {
            (Some(text), _) => text.clone(),
            (None, Some(id)) => selected_blob_text(id, blobs).await?,
            (None, None) => String::new(),
        };
        sections.push(format!(
            "<pull_request number=\"{}\" url=\"{}\" folder=\"{}\">\n{}\n{}\n{}\n</pull_request>",
            pr.number,
            xml(&pr.url),
            xml(&pr.folder_path),
            pr.title.as_deref().unwrap_or_default(),
            pr.description.as_deref().unwrap_or_default(),
            summary
        ));
    }
    for entry in &selected.extra_context_entries {
        use pb::extra_context_entry::DataOrBlobId;
        match entry.data_or_blob_id.as_ref() {
            Some(DataOrBlobId::Data(text)) => sections.push(text.clone()),
            Some(DataOrBlobId::BlobId(raw_id)) => {
                let id = BlobId::from_bytes(raw_id)?;
                let data = blobs.get(&id).await?.ok_or_else(|| {
                    Error::Protocol(format!(
                        "selected context Blob is missing: {}",
                        id.to_base64()
                    ))
                })?;
                sections.push(
                    String::from_utf8(data).map_err(|_| {
                        Error::Protocol("selected context Blob is not UTF-8".into())
                    })?,
                );
            }
            None => {
                return Err(Error::Protocol(
                    "selected context entry has no data or BlobID".into(),
                ))
            }
        }
    }
    if let Some(diff) = &selected.git_diff {
        sections.push(format!("<git_diff>\n{}\n</git_diff>", diff.content));
    }
    if let Some(diff) = &selected.git_diff_from_branch_to_main {
        sections.push(format!("<branch_diff>\n{}\n</branch_diff>", diff.content));
    }
    sections.extend(selected.git_commits.iter().map(|commit| {
        format!(
            "<git_commit sha=\"{}\">\n{}\n{}\n{}\n</git_commit>",
            xml(&commit.sha),
            commit.message,
            commit.description.as_deref().unwrap_or_default(),
            commit.diff
        )
    }));
    sections.extend(selected.selected_browsers.iter().map(|browser| {
        format!(
            "<browser id=\"{}\" url=\"{}\">{}</browser>",
            xml(&browser.browser_id),
            xml(&browser.url),
            browser.page_title.as_deref().unwrap_or_default()
        )
    }));
    sections.extend(selected.ui_elements.iter().map(|element| {
        format!(
            "<ui_element xpath=\"{}\">\n{}\n{}\n{}\n{}\n{}\n</ui_element>",
            xml(&element.xpath),
            element.element,
            element.text_content,
            element.extra,
            element.component.as_deref().unwrap_or_default(),
            element.component_props_json.as_deref().unwrap_or_default()
        )
    }));
    sections.extend(selected.console_logs.iter().map(|log| format!("<console_log level=\"{}\" client=\"{}\" session=\"{}\" timestamp=\"{}\">\n{}\n{}\n</console_log>", xml(&log.level), xml(&log.client_name), xml(&log.session_id), log.timestamp, log.message, log.object_data_json.as_deref().unwrap_or_default())));
    sections.extend(
        selected
            .documentations
            .iter()
            .map(|doc| format!("Documentation: {} (id: {})", doc.name, doc.doc_id)),
    );
    sections.extend(
        selected
            .past_chats
            .iter()
            .map(|chat| format!("Past chat: {} (agent: {})", chat.name, chat.agent_id)),
    );
    if let Some(recent) = &selected.recent_agents_context {
        sections.extend(recent.recent_agents.iter().map(|agent| format!(
            "<recent_agent name=\"{}\" transcript_path=\"{}\">\n{}\nRead the transcript with available file tools if its full conversation is needed.\n</recent_agent>",
            xml(&agent.name), xml(&agent.path), agent.overview.as_deref().unwrap_or_default()
        )));
    }
    for document in &selected.selected_documents {
        sections.push(super::documents::context(document, blobs).await?);
    }
    sections.extend(selected.selected_videos.iter().map(|video| format!(
        "<video_attachment filename=\"{}\" path=\"{}\" mime_type=\"{}\">\nIf video bytes were supplied, sampled frames and their timestamps follow as images, or a conversion error is reported. Raw audio is not included; optional local speech recognition supplies a bounded timestamped transcript or an explicit unavailable status. Only describe speech present in the transcript and respect its coverage limits. Without attached frames, inspect the path with available media/file tools before describing visual contents.\n</video_attachment>",
        xml(&video.filename), xml(&video.path), xml(&video.mime_type)
    )));
    sections.extend(
        selected
            .selected_subagents
            .iter()
            .map(|agent| format!("Selected subagent: {}", agent.name)),
    );
    sections.extend(
        selected
            .files
            .iter()
            .map(|file| format!("<file path=\"{}\">\n{}\n</file>", file.path, file.content)),
    );
    sections.extend(
        selected
            .code_selections
            .iter()
            .map(|value| format!("<code path=\"{}\">\n{}\n</code>", value.path, value.content)),
    );
    sections.extend(selected.terminals.iter().map(|value| {
        format!(
            "<terminal title=\"{}\">\n{}\n</terminal>",
            value.title.as_deref().unwrap_or_default(),
            value.content
        )
    }));
    sections.extend(selected.terminal_selections.iter().map(|value| {
        format!(
            "<terminal_selection title=\"{}\">\n{}\n</terminal_selection>",
            value.title.as_deref().unwrap_or_default(),
            value.content
        )
    }));
    sections.extend(selected.cursor_rules.iter().filter_map(|value| {
        value.rule.as_ref().map(|rule| {
            format!(
                "<rule path=\"{}\">\n{}\n</rule>",
                rule.full_path, rule.content
            )
        })
    }));
    sections.extend(selected.cursor_commands.iter().map(|value| {
        format!(
            "<command name=\"{}\">\n{}\n</command>",
            value.name, value.content
        )
    }));
    sections.extend(selected.selected_skills.iter().map(|value| {
        format!(
            "<skill path=\"{}\">\n{}\n{}\n</skill>",
            value.full_path, value.description, value.content
        )
    }));
    for link in &selected.external_links {
        let content = match (&link.pdf_content, &link.blob_id) {
            (Some(text), _) => text.clone(),
            (None, Some(id)) => selected_blob_text(id, blobs).await?,
            (None, None) => String::new(),
        };
        sections.push(format!("External link: {}\n{}", link.url, content));
    }
    Ok(Some(sections.join("\n\n")))
}

async fn selected_blob_text(
    raw_id: &[u8],
    blobs: &crate::cursor::services::blob_sync::BlobSynchronizer,
) -> Result<String> {
    let id = BlobId::from_bytes(raw_id)?;
    let data = blobs.get(&id).await?.ok_or_else(|| {
        Error::Protocol(format!(
            "selected context Blob is missing: {}",
            id.to_base64()
        ))
    })?;
    String::from_utf8(data)
        .map_err(|_| Error::Protocol("selected context Blob is not UTF-8".into()))
}

pub fn dynamic_mcp(
    request: &pb::AgentRunRequest,
    context: &pb::RequestContext,
) -> Result<BTreeMap<String, (pb::McpToolDefinition, ToolDefinition)>> {
    let direct = request
        .mcp_tools
        .iter()
        .flat_map(|tools| tools.mcp_tools.iter());
    let contextual = context.tools.iter();
    let mut output = BTreeMap::new();
    for wire in direct.chain(contextual) {
        if wire.name.is_empty() {
            return Err(Error::Protocol(
                "MCP tool definition is missing name".into(),
            ));
        }
        let parameters = match wire.input_schema_json.as_deref() {
            Some(json) if !json.trim().is_empty() => serde_json::from_str(json)?,
            _ => prost_value(wire.input_schema.as_ref().ok_or_else(|| {
                Error::Protocol(format!("MCP tool {} is missing input schema", wire.name))
            })?),
        };
        let parameters = normalize_mcp_parameters(&wire.name, parameters)?;
        let name = normalize_tool_name(&wire.name);
        let definition = ToolDefinition {
            name: name.clone(),
            description: wire.description.clone(),
            parameters,
        };
        if output
            .insert(name.clone(), (wire.clone(), definition))
            .is_some()
        {
            return Err(Error::Protocol(format!(
                "duplicate MCP tool name after normalization: {name}"
            )));
        }
    }
    Ok(output)
}

fn normalize_mcp_parameters(tool_name: &str, mut parameters: Value) -> Result<Value> {
    let schema = parameters
        .as_object_mut()
        .ok_or_else(|| invalid_mcp_parameters(tool_name))?;
    match schema.get("type") {
        Some(Value::String(schema_type)) if schema_type == "object" => return Ok(parameters),
        Some(_) => return Err(invalid_mcp_parameters(tool_name)),
        None => {}
    }
    let object_only_union = ["anyOf", "oneOf"].into_iter().any(|keyword| {
        schema
            .get(keyword)
            .and_then(Value::as_array)
            .is_some_and(|branches| {
                !branches.is_empty()
                    && branches.iter().all(|branch| {
                        branch
                            .as_object()
                            .and_then(|branch| branch.get("type"))
                            .and_then(Value::as_str)
                            == Some("object")
                    })
            })
    });
    if !object_only_union {
        return Err(invalid_mcp_parameters(tool_name));
    }
    // OpenAI-compatible function schemas (and the corresponding schema
    // validators used by other providers) require the root schema to declare
    // an object type. Cursor's app-control MCP sometimes sends an object-only
    // `anyOf`/`oneOf` schema without that root annotation. Preserve the union
    // while adding the annotation to the model-facing copy.
    schema.insert("type".into(), Value::String("object".into()));
    Ok(parameters)
}

fn invalid_mcp_parameters(tool_name: &str) -> Error {
    Error::Protocol(format!(
        "MCP tool {tool_name} input schema must describe an object"
    ))
}

fn prost_value(value: &prost_types::Value) -> Value {
    use prost_types::value::Kind;
    match value.kind.as_ref() {
        None | Some(Kind::NullValue(_)) => Value::Null,
        // Protobuf represents every number as f64. Recover exactly representable
        // integers so strict provider schema validators do not receive minItems: 1.0.
        Some(Kind::NumberValue(value))
            if value.fract() == 0.0 && value.abs() <= 9_007_199_254_740_991.0 =>
        {
            Value::Number((*value as i64).into())
        }
        Some(Kind::NumberValue(value)) => serde_json::Number::from_f64(*value)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        Some(Kind::StringValue(value)) => Value::String(value.clone()),
        Some(Kind::BoolValue(value)) => Value::Bool(*value),
        Some(Kind::StructValue(value)) => Value::Object(
            value
                .fields
                .iter()
                .map(|(key, value)| (key.clone(), prost_value(value)))
                .collect(),
        ),
        Some(Kind::ListValue(value)) => {
            Value::Array(value.values.iter().map(prost_value).collect())
        }
    }
}

pub(super) fn xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(content: &str) -> pb::CursorRule {
        pb::CursorRule {
            content: content.into(),
            ..Default::default()
        }
    }

    #[test]
    fn merge_local_rules_appends_and_dedupes_by_content() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("a.md"), "shared rule").unwrap();
        std::fs::write(directory.path().join("b.md"), "local only rule").unwrap();
        std::fs::write(directory.path().join("c.md"), "   \n").unwrap();

        let mut context = pb::RequestContext {
            non_file_rules: vec![rule("  shared rule  ")],
            ..Default::default()
        };
        merge_local_rules(&mut context, directory.path());

        let contents = context
            .non_file_rules
            .iter()
            .map(|rule| rule.content.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            contents,
            ["  shared rule  ", "local only rule"],
            "IDE-sent duplicate is kept once and blank local rules are skipped"
        );
    }

    #[test]
    fn merge_local_rules_survives_a_missing_directory() {
        let directory = tempfile::tempdir().unwrap();
        let mut context = pb::RequestContext::default();
        merge_local_rules(&mut context, &directory.path().join("nested/rules"));
        assert!(context.non_file_rules.is_empty());
    }

    #[test]
    fn protobuf_tool_schema_preserves_integer_constraints_and_fractional_values() {
        let int_val = prost_types::Value {
            kind: Some(prost_types::value::Kind::NumberValue(1.0)),
        };
        let float_val = prost_types::Value {
            kind: Some(prost_types::value::Kind::NumberValue(1.5)),
        };
        assert_eq!(prost_value(&int_val), Value::Number(1.into()));
        assert_eq!(
            prost_value(&float_val),
            Value::Number(serde_json::Number::from_f64(1.5).unwrap())
        );
    }
}
