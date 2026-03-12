# Tsunami — AI-Assisted Hardware Waveform Debugging

## Build & Test

```bash
uv run maturin develop     # Build Rust extension
uv run pytest tests/ -v    # Run all tests
```

## Architecture

- **Rust engine** (`src/`): PyO3 extension wrapping the `wellen` crate for FST/VCD access
- **Python layer** (`python/tsunami/`): Predicate DSL, time parser, MCP server, CLI

## Key entry points

- `tsunami._engine` — Rust extension (query, predicate eval, summarise)
- `tsunami.predicate` — Python DSL for composable signal predicates
- `tsunami.server` — MCP server (FastMCP, stdio transport)
- `tsunami.cli` — CLI: `tsunami info|signals|value|transitions|snapshot|anomalies|summarize|serve`

## Waveform time units

All internal times are in the waveform's native time units (timescale_factor * timescale_unit).
The `parse_time()` helper converts human-readable strings ("1284ns", "1.284us", "642cyc") to picoseconds.

## MCP Server

Start with: `tsunami serve <file.fst>`

Tools: `waveform_info`, `search_signals`, `browse_scopes`, `get_snapshot`,
`get_signal_window`, `find_first_match`, `find_all_matches`, `find_anomalies`

## Predicate DSL

```python
from tsunami.predicate import Signal, scope, signals

with scope("tb.dut") as s:
    handshake = s.tl_a_valid & s.tl_a_ready & (s.tl_a_opcode == 4)
    rising = s.clk.rise()
    seq = handshake >> (s.tl_d_valid, 50000)  # sequence with window
```
