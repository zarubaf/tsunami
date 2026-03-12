# Tsunami

**AI-assisted hardware waveform debugging tool.**

Tsunami provides a Rust-powered query engine (using the [wellen](https://github.com/ekiwi/wellen) crate) exposed to Python via PyO3, with three entry points:

- **Python library** — import `tsunami` and query waveforms programmatically
- **MCP server** — connect Claude Code to your simulation traces
- **CLI** — interactive waveform queries from the terminal

## What can it do?

- **Signal discovery** — glob-based search across thousands of signals
- **Point queries** — get any signal's value at any time
- **Range queries** — get transitions with automatic truncation
- **Predicate search** — find multi-signal conditions using a composable Python DSL, evaluated entirely in Rust
- **Summarisation** — compute period, duty cycle, value histograms without flooding your context window
- **Anomaly detection** — automatically find glitches, gaps, and stuck signals

## Architecture

```
Claude Code / Python script / CLI
    │  MCP protocol (JSON, stdio) or direct Python calls
Python layer (predicate DSL, time parser, MCP server)
    │  PyO3 direct function calls (no serialisation)
Rust query engine (_engine.so)
    │  wellen crate (safe Rust, memory-mapped, lazy loading)
FST / VCD waveform files
```

All heavy computation happens in Rust. Python handles orchestration and the user-facing interfaces. Signal data never leaves Rust memory until Python actually needs the result.

## Quick example

```python
import tsunami

handle = tsunami.open("simulation.fst")

# Discover signals
for sig in tsunami.list_signals(handle, "*tl_a*valid*"):
    print(sig["path"])

# Build a predicate and search
from tsunami.predicate import scope
with scope("tb.dut") as s:
    handshake = s.tl_a_valid & s.tl_a_ready & (s.tl_a_opcode == 4)

matches = tsunami.find_all(handle, handshake, t0_ps=0, t1_ps=10_000_000)
print(f"Found {len(matches)} TileLink Acquire transactions")
```

## Next steps

- [Getting Started](getting-started.md) — installation and first query
- [CLI Guide](guide/cli.md) — terminal usage
- [Predicate DSL](guide/predicates.md) — composable signal expressions
- [API Reference](api/engine.md) — full function documentation
