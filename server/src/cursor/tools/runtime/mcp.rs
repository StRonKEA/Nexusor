//! MCP kesfi ve yonlendirme.
use super::*;

impl CursorToolRuntime {
    pub(crate) async fn discover_mcp(&self, call: &ToolCall, result: &pb::McpStateExecResult) {
        let Some(pb::mcp_state_exec_result::Result::Success(success)) = &result.result else {
            return;
        };
        let mut discovery = self.discovered_mcp.lock().await;
        let requested = call
            .arguments
            .get("server")
            .and_then(serde_json::Value::as_str);
        if let Some(server) = requested {
            discovery.servers.insert(server.to_owned(), HashMap::new());
        } else {
            discovery.servers.clear();
            discovery.complete = true;
        }
        for server in &success.servers {
            if server.server_identifier.is_empty() {
                continue;
            }
            if requested.is_some_and(|requested| requested != server.server_identifier) {
                continue;
            }
            let routes = discovery
                .servers
                .entry(server.server_identifier.clone())
                .or_default();
            for tool in &server.tools {
                if tool.tool_name.is_empty() {
                    continue;
                }
                routes.insert(
                    tool.tool_name.clone(),
                    McpRoute {
                        name: tool.name.clone(),
                        provider_identifier: tool.provider_identifier.clone(),
                        tool_name: tool.tool_name.clone(),
                        description: tool.description.clone(),
                    },
                );
            }
        }
    }

    pub(crate) async fn mcp_route(
        &self,
        context: &ExecContext,
        server: &str,
        tool: &str,
    ) -> Option<McpRoute> {
        let discovery = self.discovered_mcp.lock().await;
        if let Some(routes) = discovery.servers.get(server) {
            return routes.get(tool).cloned();
        }
        if discovery.complete {
            return None;
        }
        context
            .mcp_routes
            .get(&(server.into(), tool.into()))
            .cloned()
    }

    pub(crate) async fn available_mcp_servers(
        &self,
        context: &ExecContext,
    ) -> Vec<String> {
        let discovery = self.discovered_mcp.lock().await;
        let mut servers: std::collections::BTreeSet<String> = discovery.servers.keys().cloned().collect();
        for (server_id, _) in context.mcp_routes.keys() {
            servers.insert(server_id.clone());
        }
        servers.into_iter().collect()
    }
}
