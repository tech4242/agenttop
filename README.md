```
   ██████╗  ██████╗ ███████╗███╗   ██╗████████╗████████╗ ██████╗ ██████╗
  ██╔══██╗██╔════╝ ██╔════╝████╗  ██║╚══██╔══╝╚══██╔══╝██╔═══██╗██╔══██╗
  ███████║██║  ███╗█████╗  ██╔██╗ ██║   ██║      ██║   ██║   ██║██████╔╝
  ██╔══██║██║   ██║██╔══╝  ██║╚██╗██║   ██║      ██║   ██║   ██║██╔═══╝
  ██║  ██║╚██████╔╝███████╗██║ ╚████║   ██║      ██║   ╚██████╔╝██║
  ╚═╝  ╚═╝ ╚═════╝ ╚══════╝╚═╝  ╚═══╝   ╚═╝      ╚═╝    ╚═════╝ ╚═╝
```

<div align="center">

**htop for AI coding agents**


[![CI](https://github.com/tech4242/agenttop/actions/workflows/ci.yml/badge.svg)](https://github.com/tech4242/agenttop/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/tech4242/agenttop/branch/main/graph/badge.svg)](https://codecov.io/gh/tech4242/agenttop)
[![Rust](https://img.shields.io/badge/rust-1.70+-orange.svg)](https://www.rust-lang.org/)
[![Ratatui](https://img.shields.io/badge/TUI-Ratatui-blue.svg)](https://ratatui.rs/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

[Installation](#installation) • [Usage](#usage) • [Features](#features) • [How It Works](#how-it-works)

</div>

---

A terminal-native observability dashboard for AI coding agents. Real-time visibility into tool usage, token consumption, and productivity metrics.

```
┌─ agenttop ──────────────────────────────────────────────────────── $4.23 ─┐
│ Tokens  In: 89K  Out: 42K  Cache: 25K (94% reuse)  Session Total: 156K   │
├──────────────────────────────────────────────────────────────────────────┤
│ API: 47 calls │ 1.2s avg │ 2 errors │ Active: 1h 47m                     │
├──────────────────────────────────────────────────────────────────────────┤
│ TOOL         CALLS  ERR  APR%   AVG      RANGE        LAST   FREQ        │
│ ▶ Read         89    0  100%    12ms    5ms-45ms      5s    ██████████░░ │
│   Bash         47    1   98%   234ms    50ms-2.1s     2s    █████░░░░░░░ │
│   Edit         34    2   94%    45ms   10ms-200ms    10s    ████░░░░░░░░ │
├──────────────────────────────────────────────────────────────────────────┤
│ MCP Tools                                                                │
│   context7:*   15    1   93%   345ms  100ms-800ms    20s    ██████████░░ │
└───── [q]uit [s]ort [p]ause [d]etail [t]ime [r]eset ──────────────────────┘
```

## Origin Story

This is the spiritual successor of an MCP logging and monitoring tool that I was building over at https://github.com/tech4242/mcphawk. After realising that the tool needs to wrap every MCP server call in e.g. Claude configs and the fact that we can only log useful information for local calls due to various OS limitations (esp. on macOS), I gate it a rest. 

Then recently I realised that we have OTLP support in some these tools, so I wanted to build something simpler (like htop) that just focuses on tool and token usage. YMMV by tool and I am hoping to push these providers to squeeze out a little more of OTLP by exposing more metrics.

Having said that, see the Limitations and Supported Agents chapter below - long way to go but let's get started!

The goal: increase transparency in development without leaving your Terminal.

If you want to contribute, please let me know! 

## Supported Agents

| Agent | OTLP Support | Signals | MCP Tools | Key Metrics |
|-------|--------------|---------|-----------|-------------|
| **Claude Code** | ✅ Full | Metrics, Logs | Anonymized (`mcp_tool`) | tokens, cost, tools, LOC |
| **OpenAI Codex CLI** | ✅ Partial | Logs, Traces | Full names | tokens, tools, prompts |
| **Gemini CLI** | ✅ Full | Metrics, Logs | Full names + `tool_type` | 40+ metrics |
| **Qwen Code** | ✅ Full | Metrics, Logs | Unknown | tokens, diff stats |
| **Cline** | ⚠️ Via provider | Logs, Metrics | N/A | events, errors |
| **Cursor** | ❌ Proprietary | Admin API only | N/A | aggregate stats |
| **GitHub Copilot** | ❌ Proprietary | REST API only | N/A | usage rates |
| **Aider** | ❌ None | - | - | - |

## Features

- **Token Tracking** - Input, output, and cache token metrics
- **Tool Table** - Real-time tool call metrics with:
  - Call count and error count
  - Time since last call
  - Average duration and duration range
  - Relative frequency bar
- **API Metrics** - API calls, latency, active time
- **Productivity Metrics** - Lines of code, commits
- **Cache Reuse Rate** - Prompt caching efficiency
- **Session Cost** - Running cost estimate

## Installation

### Homebrew (macOS)

```bash
brew install tech4242/tap/agenttop
```

### Cargo

```bash
cargo install agenttop
```

### Binary Downloads

Download pre-built binaries from [GitHub Releases](https://github.com/tech4242/agenttop/releases).

## Usage

```bash
# Just run it - auto-configures Claude Code if needed
agenttop
```

That's it! agenttop automatically:
1. Enables Claude Code's OpenTelemetry export (if not already enabled)
2. Starts an OTLP receiver on port 4318
3. Shows real-time metrics in a terminal dashboard

## Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `q` | Quit |
| `s` | Cycle sort column |
| `p` | Pause/resume updates |
| `d` / `Enter` | Show tool details |
| `r` | Reset statistics |
| `↑`/`k` | Select previous |
| `↓`/`j` | Select next |
| `Esc` | Close detail view |

## How It Works

agenttop uses Claude Code's native OpenTelemetry support to collect metrics:

```
Claude Code                        agenttop
    │                                  │
    ├── OTEL metrics ─────────────────►│ HTTP OTLP Receiver
    │   (port 4318)                    │     │
    │                                  │     ▼
    └── OTEL events ──────────────────►│ DuckDB (embedded)
        (tool_result, api_request)     │     │
                                       │     ▼
                                       │ Ratatui TUI
```

### Metrics Collected

| Metric | Description |
|--------|-------------|
| `claude_code.token.usage` | Input/output/cache tokens (by `type` attribute) |
| `claude_code.cost.usage` | Session cost in USD |
| `claude_code.active_time.total` | Active coding time in seconds |
| `claude_code.lines_of_code.count` | Lines added/removed |
| `claude_code.commit.count` | Git commits created |

### Events Collected

| Event | Description |
|-------|-------------|
| `tool_result` / `claude_code.tool_result` | Tool invocations with success/duration |
| `api_request` | API calls with model, latency, token counts |
| `api_error` | API errors with error type and message |

## Limitations

### MCP Tool Names (Claude Code)
Claude Code anonymizes MCP tool names in telemetry for privacy (v2.1.2+).
All MCP tools appear as `mcp_tool`. Other agents (Codex, Gemini) expose full names.

### Context Window Usage
Claude Code does NOT expose context window usage or compaction status in telemetry.
The ~200K context window and ~75% compaction threshold are internal only.
agenttop shows cumulative session tokens, not context window remaining.

### Approval Rate
The `decision` attribute for tool approval tracking is not consistently present
in all Claude Code versions. APR% may show as 100% when data is unavailable.

## Configuration

agenttop automatically configures Claude Code's `~/.claude/settings.json` with the required environment variables:

```json
{
  "enableTelemetry": true,
  "env": {
    "CLAUDE_CODE_ENABLE_TELEMETRY": "1",
    "OTEL_METRICS_EXPORTER": "otlp",
    "OTEL_LOGS_EXPORTER": "otlp",
    "OTEL_EXPORTER_OTLP_PROTOCOL": "http/protobuf",
    "OTEL_EXPORTER_OTLP_ENDPOINT": "http://localhost:4318"
  }
}
```

A backup is created at `~/.claude/settings.json.bak` before any modifications.

**Note:** After agenttop configures your settings, restart Claude Code for the telemetry to take effect.

## Data Storage

Metrics are stored in DuckDB at:
- macOS: `~/Library/Application Support/agenttop/metrics.duckdb`
- Linux: `~/.local/share/agenttop/metrics.duckdb`

Data is automatically pruned after 7 days.

## Development

```bash
# Build
cargo build

# Run
cargo run

# Test
cargo test

# Release build
cargo build --release
```

## License

MIT
