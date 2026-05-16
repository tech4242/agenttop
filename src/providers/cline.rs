//! Cline provider (VSCode extension; native OTLP since 2026)

use super::Provider;

/// Tool names emitted by Cline (subset; non-MCP tools land here).
const BUILTIN_TOOLS: &[&str] = &[
    "read_file",
    "write_to_file",
    "replace_in_file",
    "list_files",
    "search_files",
    "execute_command",
    "browser_action",
    "attempt_completion",
    "ask_followup_question",
    "plan_mode_response",
    "new_task",
    "use_mcp_tool",
    "access_mcp_resource",
];

/// Cline provider. Native OTLP via Cline Enterprise dashboard config.
/// agenttop can't auto-configure Cline (its OTLP settings live in a remote
/// dashboard), so setup is documented instead.
pub struct ClineProvider;

impl Provider for ClineProvider {
    fn id(&self) -> &'static str {
        "cline"
    }

    fn name(&self) -> &'static str {
        "Cline"
    }

    fn metric_prefix(&self) -> &'static str {
        "cline"
    }

    fn builtin_tools(&self) -> &'static [&'static str] {
        BUILTIN_TOOLS
    }

    fn shorten_model_name(&self, _name: &str) -> Option<String> {
        // Cline routes to user-selected upstream models; let other providers
        // (Claude / OpenAI) own model name shortening.
        None
    }

    fn normalize_token_type(&self, _token_type: &str) -> Option<&'static str> {
        None
    }

    fn service_name(&self) -> Option<&'static str> {
        Some("cline")
    }

    fn setup_instructions(&self) -> Option<&'static str> {
        Some(
            "Cline emits standard OTLP and is configured through Cline Enterprise's\n\
             remote configuration dashboard, not a local file. To stream telemetry\n\
             to agenttop:\n\n\
             1. In the Cline Enterprise dashboard, set the OTLP endpoint to:\n      \
                  http://localhost:4318  (HTTP/protobuf)\n\
             2. Set the OTel resource attribute OTEL_SERVICE_NAME=cline so agenttop\n   \
                can distinguish Cline from other agents.\n\
             3. Reload Cline; tool calls and `cline.turns.total` will appear in agenttop.",
        )
    }
}
