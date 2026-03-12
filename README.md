# Tsunami

AI-assisted hardware waveform debugging tool. Tsunami provides a Rust-powered query engine (using the [wellen](https://github.com/ekiwi/wellen) crate) exposed to Python via PyO3, with three entry points: a Python library, an MCP server for Claude Code, and a CLI.

## Requirements

- Python 3.12+
- Rust toolchain (rustc, cargo)
- [uv](https://docs.astral.sh/uv/) for project management

## Setup

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

Start the server:

```bash
tsunami serve sim.fst
```

Add to your Claude Code MCP config:

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

Available tools: `waveform_info`, `search_signals`, `browse_scopes`, `get_snapshot`, `get_signal_window`, `find_first_match`, `find_all_matches`, `find_anomalies`.

## Architecture

```
Claude Code
    |  MCP protocol (JSON, stdio)
Python MCP Server / CLI
    |  PyO3 direct function calls
Rust query engine (_engine.so)
    |  wellen crate
FST / VCD waveform files
```

| Layer        | Technology  | Responsibility                                              |
|--------------|-------------|-------------------------------------------------------------|
| MCP Server   | Python, FastMCP | Tool definitions, time parsing, auto-summarisation      |
| Query Engine | Rust, PyO3  | Signal access, predicate evaluation, summarisation          |
| Waveform I/O | wellen     | FST/VCD parsing, memory-mapped lazy loading                 |

## Testing

```bash
uv run pytest tests/ -v
```

## Project Structure

```
tsunami/
├── pyproject.toml                  # maturin build backend, uv managed
├── Cargo.toml                      # wellen + pyo3 deps
├── src/                            # Rust crate (PyO3 cdylib)
│   ├── lib.rs                      # PyO3 module entry
│   ├── query.rs                    # Core signal queries
│   ├── predicate.rs                # Expr enum, evaluation engine
│   └── summarise.rs                # Anomaly detection, period inference
├── python/tsunami/
│   ├── __init__.py                 # Public API re-exports
│   ├── _engine.pyi                 # Type stubs for Rust module
│   ├── predicate.py                # Python DSL (Expr dataclasses + operators)
│   ├── time_parse.py               # ns/us/cyc -> ps normalisation
│   ├── server.py                   # MCP server (FastMCP)
│   └── cli.py                      # CLI entry point
└── tests/
    ├── test_engine.py              # Rust engine integration tests
    ├── test_predicate.py           # DSL unit tests
    └── test_time_parse.py          # Time parser tests
```
