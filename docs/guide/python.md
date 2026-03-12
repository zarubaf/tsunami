# Python Library Guide

## Opening a waveform

```python
import tsunami

handle = tsunami.open("simulation.fst")
info = tsunami.waveform_info(handle)
```

The handle is an opaque reference to the memory-mapped waveform. Signal data is loaded lazily — only signals you query are read from disk.

## Discovering signals

### By glob pattern

```python
# All signals containing "valid"
signals = tsunami.list_signals(handle, "*valid*")
for sig in signals:
    print(f"[{sig['width']:>4}] {sig['path']}")
```

### By hierarchy

```python
scopes = tsunami.list_scopes(handle, "tb.dut")
for s in scopes:
    print(s)
```

## Point queries

### Single signal

```python
val = tsunami.get_value(handle, "tb.dut.clk", time_ps=1000)
print(f"0x{val['hex']}")       # "0x1"
print(f"is_x={val['is_x']}")   # False
```

### Multiple signals (snapshot)

More efficient than calling `get_value` in a loop:

```python
snap = tsunami.get_snapshot(handle, [
    "tb.dut.tl_a_valid",
    "tb.dut.tl_a_ready",
    "tb.dut.tl_a_opcode",
], time_ps=1_284_000)

for sig, val in snap.items():
    print(f"{sig} = 0x{val['hex']}")
```

## Range queries

### Transitions

```python
result = tsunami.get_transitions(
    handle, "tb.dut.clk",
    t0_ps=0, t1_ps=10_000,
    max_edges=100,  # cap output
)

print(f"Total: {result['total_transitions']}")
print(f"Truncated: {result['truncated']}")
for tr in result["transitions"]:
    print(f"  t={tr['time']}: 0x{tr['value']}")
```

### Edge search

```python
# Find next rising edge after t=0
t = tsunami.find_next_edge(handle, "tb.dut.clk", "rising", after_ps=0)
```

## Predicate search

Use the [predicate DSL](predicates.md) for multi-signal conditions:

```python
from tsunami.predicate import Signal

valid = Signal("tb.dut.tl_a_valid")
ready = Signal("tb.dut.tl_a_ready")
handshake = valid & ready

# First match
t = tsunami.find_first(handle, handshake, after_ps=0)

# All matches in a window
times = tsunami.find_all(handle, handshake, t0_ps=0, t1_ps=10_000_000)
```

## Summarisation

For large windows where individual transitions would be overwhelming:

```python
summary = tsunami.summarize(handle, "tb.dut.clk", t0_ps=0, t1_ps=10_000_000)
print(f"Period:     {summary['dominant_period_ps']}ps")
print(f"Duty cycle: {summary['duty_cycle']:.1%}")
print(f"Anomalies:  {len(summary['anomalies'])}")
```

### Multi-signal summary

```python
summaries = tsunami.summarize_window(
    handle,
    ["tb.dut.clk", "tb.dut.tl_a_valid", "tb.dut.tl_a_ready"],
    t0_ps=0, t1_ps=10_000_000,
)
for sig, s in summaries.items():
    print(f"{sig}: {s['total_transitions']} transitions")
```

## Anomaly detection

```python
anomalies = tsunami.find_anomalies(
    handle, "tb.dut.clk",
    t0_ps=0, t1_ps=10_000_000,
)
for a in anomalies:
    print(f"  t={a['time_ps']}: {a['kind']} — {a['detail']}")
```

Anomaly kinds:

- **glitch** — interval shorter than 25% of the dominant period
- **gap** — interval longer than 200% of the dominant period
- **stuck** — no transitions for a long time at the end of the window

## Time parsing

The [`parse_time`][tsunami.time_parse.parse_time] helper converts human-readable
strings to picoseconds:

```python
from tsunami.time_parse import parse_time

parse_time("1284ns")     # 1_284_000
parse_time("1.284us")    # 1_284_000
parse_time("642cyc", timescale_ps=1000)  # 642_000
parse_time(1234)          # 1234 (passthrough)
```
