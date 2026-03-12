# Predicate DSL Guide

The predicate DSL lets you express multi-signal conditions using natural Python
syntax. Expressions build a pure data structure (AST) — no waveform access
happens in Python. The entire tree is handed to Rust in a single PyO3 call for
high-performance evaluation.

## Basic signals

```python
from tsunami.predicate import Signal

clk = Signal("tb.dut.clk")
valid = Signal("tb.dut.tl_a_valid")
ready = Signal("tb.dut.tl_a_ready")
opcode = Signal("tb.dut.tl_a_opcode")
```

## Logical operators

```python
# AND — both must be non-zero
handshake = valid & ready

# OR — either is non-zero
active = valid | ready

# NOT — zero becomes true
idle = ~valid

# XOR — exactly one is non-zero
mismatch = valid ^ ready
```

## Comparisons

Integer operands are automatically wrapped in `Const`:

```python
is_get = opcode == 4         # equals
is_large = opcode > 3        # unsigned greater-than
is_small = opcode < 2        # unsigned less-than
```

## Edge detection

```python
rising = valid.rise()   # 0 → non-zero transition
falling = valid.fall()  # non-zero → 0 transition
```

## Bitfield extraction

```python
data = Signal("tb.dut.data")

low_byte = data[7:0]          # bits 7 down to 0
high_nibble = data[31:28]     # bits 31 down to 28
single_bit = data[15]         # just bit 15

# Use in expressions
check = data[7:0] == 0xff
```

## Sequences

The `>>` operator expresses temporal ordering:

```python
# a followed by b (any time later)
req_then_resp = valid.rise() >> ready.rise()

# a followed by b within 50,000ps
fast_resp = valid.rise() >> (ready.rise(), 50_000)
```

## Preceded-by

Check that a condition occurred in the recent past:

```python
# valid.rise() where handshake happened within the last 10,000ps
guarded = valid.rise().preceded_by(handshake, within_ps=10_000)

# Protocol violation: grant without preceding acquire
spurious_grant = (
    Signal("tb.dut.tl_d_valid").rise()
    .preceded_by(handshake, within_ps=50_000)
    .__invert__()  # negate: true when preceded_by is FALSE
)
```

## Composition

All operators return `Expr` objects, so they compose freely:

```python
acquire = (
    valid & ready
    & (opcode == 4)
    & (Signal("tb.dut.tl_a_source") == 3)
)

# Sequence: acquire followed by grant within 20 cycles
CYCLE_PS = 1000
roundtrip = acquire >> (
    Signal("tb.dut.tl_d_valid") & Signal("tb.dut.tl_d_ready"),
    20 * CYCLE_PS,
)
```

## Scope helper

Avoid repeating hierarchy prefixes:

```python
from tsunami.predicate import scope

with scope("tb.dut.core") as s:
    handshake = s.tl_a_valid & s.tl_a_ready
    # s.tl_a_valid → Signal("tb.dut.core.tl_a_valid")
```

## Signals helper

Map short aliases to full paths (useful with config dicts):

```python
from tsunami.predicate import signals

TILELINK = {
    "v": "tb.dut.core.tl_out_a_valid",
    "r": "tb.dut.core.tl_out_a_ready",
    "op": "tb.dut.core.tl_out_a_bits_opcode",
}

with signals(**TILELINK) as s:
    handshake = s.v & s.r & (s.op == 4)
```

## Evaluating predicates

Pass any `Expr` to the Rust engine:

```python
import tsunami

handle = tsunami.open("sim.fst")

# First match after t=0
t = tsunami.find_first(handle, handshake, after_ps=0)

# All matches in a window
times = tsunami.find_all(handle, handshake, t0_ps=0, t1_ps=10_000_000)

# Scan: all transition points where predicate is true, with values
points = tsunami.scan(handle, handshake, t0_ps=0, t1_ps=10_000_000)
for p in points:
    print(f"  t={p['time']}: value={p['value']}")
```

## How it works

1. Python builds an expression tree (AST) of frozen dataclasses
2. Each node has a `tag` field (`"signal"`, `"and"`, `"rise"`, etc.)
3. PyO3's `FromPyObject` maps the Python tree to a Rust `Expr` enum
4. Rust computes the union of transition points for all referenced signals
5. The predicate is evaluated at each transition point in a single pass
6. Only signals actually used by the predicate are loaded from the FST file
