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

A terminal-native observability dashboard for Claude Code. Real-time visibility into tool usage, token consumption, and productivity metrics.

```
┌─ agenttop ─────────────────────────────────────── Session: 2h 15m ─ $4.23 ─┐
│ Tokens [████████████████████░░░░░░░░░░░] 156K/500K                          │
│         In: 89K │ Out: 42K │ Cache: 25K (94% hit)                           │
├─────────────────────────────────────────────────────────────────────────────┤
│ TOOL              CALLS   LAST     AVG      STATUS                          │
│ ▶ Bash              47     2s     234ms    ████████████████░░░░             │
│   Read              89     5s      12ms    ██████████░░░░░░░░░░             │
│   Edit              34    10s      45ms    ███████░░░░░░░░░░░░░             │
│   Write             12    30s      67ms    ███░░░░░░░░░░░░░░░░░             │
│   Grep              28     8s      18ms    █████░░░░░░░░░░░░░░░             │
│   WebSearch          3     1m     890ms    █░░░░░░░░░░░░░░░░░░░             │
│   mcp__github       15    20s     345ms    ████░░░░░░░░░░░░░░░░             │
├─────────────────────────────────────────────────────────────────────────────┤
│ Productivity: 47x │ Lines: +2,847 │ Commits: 3 │ PRs: 1 │ Tools: 234       │
└───── [q]uit [s]ort [p]ause [d]etail [r]eset ────────────────────────────────┘
```

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

## Features

- **Token Usage Bar** - Visual representation of context window usage
- **Tool Table** - Real-time tool call metrics with:
  - Call count
  - Time since last call
  - Average duration
  - Activity sparkline
- **Productivity Metrics** - Lines of code, commits, PRs
- **Cache Hit Rate** - Prompt caching efficiency
- **Session Cost** - Running cost estimate

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
| `claude_code.token.usage` | Input/output/cache tokens |
| `claude_code.cost.usage` | Session cost in USD |
| `claude_code.lines_of_code.count` | Lines added/removed |
| `claude_code.commit.count` | Git commits created |
| `claude_code.pull_request.count` | PRs created |

### Events Collected

| Event | Description |
|-------|-------------|
| `claude_code.tool_result` | Tool invocations with success/duration |
| `claude_code.api_request` | API calls with token counts |

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
