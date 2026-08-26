# Tsunami

AI-assisted hardware waveform debugging tool. Tsunami provides a Rust-powered query engine (using the [wellen](https://github.com/ekiwi/wellen) crate) with four entry points: a standalone Rust MCP server, a Python library, a Python MCP server, and a CLI.

## Requirements

- Rust toolchain (rustc, cargo)
- Python 3.11+ and [uv](https://docs.astral.sh/uv/) (for the Python library/CLI)

## Setup

### Standalone Rust MCP server (no Python needed)

```bash
cargo build --release -p tsunami-serve
```

The binary is at `target/release/tsunami-serve`.

### Python library & CLI

```bash
uv sync                        # Install Python dependencies
uv run maturin develop         # Build the Rust extension
```

## Usage

### CLI

```bash
# Waveform metadata
tsunami info <file.fst>

# Signal discovery (glob patterns)
tsunami signals <file.fst> "*clk*"

# Browse hierarchy
tsunami scopes <file.fst> "tb.dut"

# Point query
tsunami value <file.fst> <signal> <time>

# Transitions in a time range
tsunami transitions <file.fst> <signal> <t0> <t1>

# Multi-signal snapshot at a single time
tsunami snapshot <file.fst> <time> <signal1> <signal2> ...

# Signal summary (period, duty cycle, anomalies)
tsunami summarize <file.fst> <signal> <t0> <t1>

# Anomaly detection (glitches, gaps, stuck signals)
tsunami anomalies <file.fst> <signal> <t0> <t1>

# Start MCP server for Claude Code
tsunami serve <file.fst>
```

Time values accept human-readable formats: `100ps`, `1284ns`, `1.284us`, `642cyc`, or raw integers (picoseconds).

### Python Library

```python
import tsunami

handle = tsunami.open("sim.fst")
info = tsunami.waveform_info(handle)

# Discover signals
signals = tsunami.list_signals(handle, "*valid*")

# Point query
val = tsunami.get_value(handle, "tb.dut.tl_a_valid", 1_284_000)

# Transitions in a window
result = tsunami.get_transitions(handle, "tb.dut.clk", 0, 10_000_000)

# Multi-signal snapshot
snap = tsunami.get_snapshot(handle, ["tb.dut.clk", "tb.dut.reset"], 1_000)

# Summarise a signal
summary = tsunami.summarize(handle, "tb.dut.clk", 0, 10_000_000)
```

### Predicate DSL

Compose multi-signal conditions that evaluate entirely in Rust in a single pass:

```python
from tsunami.predicate import Signal, scope

with scope("tb.dut") as s:
    # TileLink handshake with specific opcode
    handshake = s.tl_a_valid & s.tl_a_ready & (s.tl_a_opcode == 4)

    # Rising edge detection
    rising = s.clk.rise()

    # Sequence with time window
    roundtrip = handshake >> (s.tl_d_valid & s.tl_d_ready, 50_000)

    # Negated preceded-by (protocol violation)
    spurious = s.tl_d_valid.rise().preceded_by(handshake, within_ps=50_000).__invert__()

# Scan entirely in Rust
matches = tsunami.find_all(handle, handshake, t0_ps=0, t1_ps=10_000_000)
first = tsunami.find_first(handle, rising, after_ps=0)
```

### MCP Server (Claude Code)

Tsunami ships two MCP servers: a standalone Rust binary (`tsunami-serve`) and a Python-based server (`tsunami serve`). Both expose the same session-based tool interface over stdio.

#### Standalone Rust server (recommended)

No Python runtime needed. Build once and point Claude Code at the binary:

```bash
cargo build --release -p tsunami-serve
```

Start with a pre-loaded waveform:

```bash
./target/release/tsunami-serve sim.fst
```

Or start without a file and use `open_waveform` to load dynamically:

```bash
./target/release/tsunami-serve
```

Claude Code MCP config (`.mcp.json` or `~/.claude/mcp.json`):

```json
{
  "mcpServers": {
    "tsunami": {
      "command": "/path/to/tsunami-serve",
      "args": ["sim.fst"]
    }
  }
}
```

#### Python server

```bash
tsunami serve sim.fst
```

```json
{
  "mcpServers": {
    "tsunami": {
      "command": "tsunami",
      "args": ["serve", "sim.fst"]
    }
  }
}
```

#### Available tools

Both servers expose: `open_waveform`, `waveform_info`, `get_waveform_length`, `search_signals`, `browse_scopes`, `get_signal_info`, `get_snapshot`, `get_signal_window`, `find_edge`, `find_value`, `find_pattern`, `find_first_match`, `find_all_matches`, `find_anomalies`.

The MCP server is session-based. `open_waveform` returns a `session_id`, and all
other waveform tools require that session ID. If you start the server with a
file argument, the preloaded waveform is available as session `default`.

## Architecture

```
Claude Code / LLM
    |  MCP protocol (JSON, stdio)
    |
    ├── tsunami-serve (pure Rust binary, rmcp)
    |       |
    └── Python MCP Server / CLI (FastMCP)
            |  PyO3 calls
            |
      tsunami-core (pure Rust library)
            |  wellen crate
      FST / VCD waveform files
```

| Layer | Technology | Responsibility |
|---|---|---|
| Rust MCP Server | `tsunami-serve`, rmcp | Standalone MCP server, no Python needed |
| Python MCP Server | FastMCP | Tool definitions, Python-side time parsing |
| Core Engine | `tsunami-core` | Signal queries, predicate eval, summarisation, time parsing |
| PyO3 Bindings | `tsunami` cdylib | Thin wrappers exposing `tsunami-core` to Python |
| Waveform I/O | wellen | FST/VCD parsing, memory-mapped lazy loading |

## Testing

```bash
uv run pytest tests/ -v
```

## Project Structure

```
tsunami/
├── Cargo.toml                      # Workspace root
├── pyproject.toml                  # maturin build backend, uv managed
├── crates/
│   ├── tsunami-core/               # Pure Rust library (no PyO3)
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── query.rs            # Signal queries, value helpers
│   │       ├── predicate.rs        # Expr AST, evaluation engine
│   │       ├── summarise.rs        # Anomaly detection, period inference
│   │       └── time_parse.rs       # Human-readable time parser
│   └── tsunami-serve/              # Standalone Rust MCP server binary
│       └── src/
│           ├── main.rs             # CLI entry, stdio transport
│           └── server.rs           # MCP tool handlers (rmcp)
├── src/                            # PyO3 cdylib (thin wrappers)
│   ├── lib.rs                      # Module entry
│   ├── py_query.rs                 # Query wrappers
│   ├── py_predicate.rs             # Predicate wrappers
│   └── py_summarise.rs             # Summary wrappers
├── python/tsunami/
│   ├── __init__.py                 # Public API re-exports
│   ├── _engine.pyi                 # Type stubs for Rust module
│   ├── predicate.py                # Python DSL (Expr dataclasses + operators)
│   ├── time_parse.py               # ns/us/cyc -> ps normalisation
│   ├── server.py                   # Python MCP server (FastMCP)
│   └── cli.py                      # CLI entry point
└── tests/
    ├── test_engine.py              # Rust engine integration tests
    ├── test_predicate.py           # DSL unit tests
    └── test_time_parse.py          # Time parser tests
```
