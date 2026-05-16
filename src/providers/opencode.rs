//! opencode provider (sst/opencode; OTLP via DEVtheOPS/opencode-plugin-otel)

use super::Provider;

pub struct OpenCodeProvider;

impl Provider for OpenCodeProvider {
    fn id(&self) -> &'static str {
        "opencode"
    }

    fn name(&self) -> &'static str {
        "opencode"
    }

    fn metric_prefix(&self) -> &'static str {
        "opencode"
    }

    fn builtin_tools(&self) -> &'static [&'static str] {
        // Native OTLP isn't shipped upstream yet; the community plugin mirrors
        // Claude Code's tool naming. Detection happens via service.name=opencode.
        &[]
    }

    fn shorten_model_name(&self, _name: &str) -> Option<String> {
        None
    }

    fn normalize_token_type(&self, _token_type: &str) -> Option<&'static str> {
        None
    }

    fn service_name(&self) -> Option<&'static str> {
        Some("opencode")
    }

    fn setup_instructions(&self) -> Option<&'static str> {
        Some(
            "opencode (sst/opencode) doesn't have native OTLP yet. Use the community\n\
             plugin DEVtheOPS/opencode-plugin-otel:\n\n\
             1. Install the plugin per its README.\n\
             2. Set these env vars in your shell:\n      \
                  OPENCODE_ENABLE_TELEMETRY=1\n      \
                  OPENCODE_OTLP_ENDPOINT=http://localhost:4318\n      \
                  OPENCODE_OTLP_PROTOCOL=http/protobuf\n\
             3. Restart opencode; tool/token events will appear in agenttop.",
        )
    }
}
