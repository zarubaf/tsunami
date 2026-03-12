# Getting Started

## Requirements

- **Python 3.12+**
- **Rust toolchain** (rustc, cargo) — for building from source
- **[uv](https://docs.astral.sh/uv/)** — for project management

## Installation

### From PyPI (prebuilt wheels)

```bash
pip install tsunami-wave
```

No Rust toolchain needed — binary wheels include the compiled extension.

### From source

```bash
git clone https://github.com/zarubaf/tsunami.git
cd tsunami
uv sync
uv run maturin develop
```

### As a dependency

```toml
# In your pyproject.toml
dependencies = ["tsunami-wave"]
```

## Verify the installation

```bash
tsunami info your_simulation.fst
```

You should see output like:

```
  timescale_factor: 100
  timescale_unit: PicoSeconds
  duration: 645
  num_signals: 38647
  file_format: Fst
```

## Your first query

### CLI

```bash
# List all clock signals
tsunami signals sim.fst "*clk*"

# Get clock value at time 1000ps
tsunami value sim.fst tb.dut.clk 1000

# See clock transitions from 0 to 10000ps
tsunami transitions sim.fst tb.dut.clk 0 10000
```

### Python

```python
import tsunami

handle = tsunami.open("sim.fst")
info = tsunami.waveform_info(handle)
print(f"Signals: {info['num_signals']}, Duration: {info['duration']}")

# Search for signals
signals = tsunami.list_signals(handle, "*valid*")
for s in signals[:10]:
    print(f"  [{s['width']:>4}] {s['path']}")

# Query a value
val = tsunami.get_value(handle, "tb.dut.clk", 1000)
print(f"clk @ 1000 = 0x{val['hex']}")
```

## Time formats

Tsunami accepts time values in multiple formats everywhere — CLI arguments,
Python function calls, and MCP tool parameters:

| Format | Example | Meaning |
|---|---|---|
| Raw integer | `1000` | 1000 (native time units) |
| Picoseconds | `"1000ps"` | 1,000 ps |
| Nanoseconds | `"1.284us"` | 1,284,000 ps |
| Microseconds | `"1.284us"` | 1,284,000 ps |
| Milliseconds | `"1ms"` | 1,000,000,000 ps |
| Cycles | `"642cyc"` | 642 × timescale_ps |

## Next steps

- [CLI Guide](guide/cli.md) — all available commands
- [Python Guide](guide/python.md) — library usage patterns
- [Predicate DSL](guide/predicates.md) — composable signal queries
- [MCP Server](guide/mcp.md) — Claude Code integration
