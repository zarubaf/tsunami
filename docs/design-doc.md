# Tsunami — Design Document

*AI-Assisted Hardware Waveform Debugging · v0.1 Draft · Axelera AI Confidential*

---

## 1  Motivation

Hardware simulation debugging today is a manual, visual, and time-consuming process. An engineer opens GTKWave, navigates thousands of signals, visually correlates events across protocol boundaries, and forms hypotheses by hand. For large designs — OoO CPU cores, coherence fabrics, FPGA emulation traces — this process does not scale.

WaveformMCP replaces the manual query loop with an AI-driven one. Claude Code gains the ability to:

- Discover and query signals from FST/VCD files at arbitrary time points
- Search for complex multi-signal conditions using a composable predicate language
- Decode protocol transactions (TileLink, AXI, custom) from raw signals
- Trace instructions through pipeline stages and identify microarchitectural bugs
- Summarise large waveform windows without flooding the context window

Target use cases: **assertion triage** (trace backwards from a failing assertion to root cause), **protocol violation detection** (arbiter misfires, coherence errors), and **pipeline debugging** (phantom wakeups, wrong instruction issue in OoO cores).

---

## 2  Architecture

### 2.1  Layered Design

```
Claude Code
    ↕  MCP protocol (JSON, via mcp-python SDK)
Python MCP Server
  ├── wellen MCP tools       raw signal queries, generic
  └── µScope MCP tools       transaction semantics, Loom-specific
    ↕  PyO3 direct function calls (no serialisation)
Rust waveform_engine.so
  └── wellen crate           FST/VCD, lazy memory-mapped loading
```

| Layer        | Technology              | Responsibility                                                                                              |
| ------------ | ----------------------- | ----------------------------------------------------------------------------------------------------------- |
| MCP Server   | Python (mcp-python SDK) | MCP tool definitions, protocol decoders, µScope semantic layer, predicate DSL builder                       |
| Query Engine | Rust + PyO3             | Signal access, transition scanning, predicate evaluation, summarisation. Python never on the critical path. |
| Waveform I/O | wellen crate (Rust)     | FST/VCD/GHW parsing, memory-mapped lazy signal loading, multithreaded VCD parse, safe Rust throughout       |

### 2.2  Two MCP Servers

Two separate MCP servers are exposed, independently useful and composable in the same Claude Code session:

- **wellen MCP** — generic, design-agnostic. Works on any FST/VCD file. Raw signal queries, edge search, predicate evaluation, summarisation. No protocol or pipeline knowledge.
- **µScope MCP** — semantic, Loom-specific. Consumes the wellen MCP foundation and lifts it to transaction level. TileLink/AXI decoders, pipeline tracers, coherence violation detectors. Requires a per-design config mapping signal paths.

Claude Code can use both simultaneously: start with µScope for the high-level transaction picture, drop to wellen when sub-transaction signal behaviour needs inspection.

### 2.3  PyO3 Integration

The Rust query engine is compiled as a native Python extension (`.so` / `.pyd`) using PyO3 and maturin. There is no subprocess, no serialisation boundary, and no IPC protocol between the Python MCP layer and the Rust engine.

Data never leaves Rust memory until Python actually needs it. For summarised queries — where Rust scans millions of transitions and returns a handful of anomaly timestamps — the cross-language overhead is negligible.

```python
# Python MCP layer — direct function call into Rust
import waveform_engine  # compiled .so via PyO3

transitions = waveform_engine.get_transitions(
    "tb.dut.core.tl_a_valid", t0_ps=1_200_000, t1_ps=1_400_000
)
```

---

## 3  Predicate DSL

### 3.1  Design Goals

Reconstructing protocol transactions requires composable, multi-signal predicates. A simple list of signal/value pairs is insufficient for expressing handshakes, ordering constraints, and protocol violations. The predicate DSL must:

- Be expressible in natural Python syntax via operator overloading and method chaining
- Build a pure data structure (AST) — no execution in Python
- Hand the entire tree to Rust in a single PyO3 call
- Allow Rust to evaluate the predicate at every transition point in one scan pass

### 3.2  Operators

| Operator / Method             | Meaning                                 |
| ----------------------------- | --------------------------------------- |
| `a & b`                       | Logical AND                             |
| `a                            | b`                                      | Logical OR |
| `~a`                          | Logical NOT                             |
| `a ^ b`                       | Logical XOR                             |
| `sig == val`                  | Signal equals constant                  |
| `sig > val` / `sig < val`     | Numeric comparison (unsigned)           |
| `sig.rise()`                  | Rising edge                             |
| `sig.fall()`                  | Falling edge                            |
| `a >> b`                      | Sequence: a followed by b (unbounded)   |
| `a >> (b, window_ps)`         | Sequence: a followed by b within window |
| `a.preceded_by(b, within_ps)` | b occurred before a within window       |
| `sig[hi:lo]`                  | Bitfield extraction before comparison   |

### 3.3  Example: TileLink Transaction

```python
with engine.scope("tb.dut") as s:
    # A-channel handshake with specific opcode (AcquireBlock = 4)
    acquire = (
        s.tl_a_valid &
        s.tl_a_ready &
        (s.tl_a_opcode == 4) &
        (s.tl_a_source == 3)
    )

    # Phantom wakeup: instruction issued but operand not ready
    phantom = (
        s.iq.issue_valid.rise() &
        ~s.iq.src1_ready
    )

    # Protocol violation: D-channel Grant with no preceding Acquire
    spurious_grant = (
        s.tl_d_valid.rise()
        .preceded_by(acquire, within_ps=50_000)
        .__invert__()
    )

    # Full round-trip sequence
    roundtrip = acquire.rise() >> (s.tl_d_valid & s.tl_d_ready, 20 * CYCLE_PS)

# All scanning happens in Rust — single PyO3 call
violations = engine.find_all(spurious_grant, t0_ps=0, t1_ps=sim_end)
```

### 3.4  Signal Scope and Name Resolution

The `engine.scope()` context manager sets a hierarchy prefix. Signal attribute access resolves against the wellen Hierarchy, raising an error at tree-construction time if the path does not exist — not at scan time. This catches typos immediately.

```python
with engine.signals(
    v  = "tb.dut.tl_a_valid",
    r  = "tb.dut.tl_a_ready",
    op = "tb.dut.tl_a_opcode",
) as s:
    handshake = s.v & s.r & (s.op == 4)
```

### 3.5  Rust AST Representation

The Python DSL builds a tree of plain dataclasses. PyO3 maps these to a Rust enum for zero-overhead evaluation:

```rust
#[derive(FromPyObject)]
enum Expr {
    Signal(String),
    Const(u64),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
    Xor(Box<Expr>, Box<Expr>),
    Eq(Box<Expr>, Box<Expr>),
    Gt(Box<Expr>, Box<Expr>),
    Lt(Box<Expr>, Box<Expr>),
    Rise(Box<Expr>),
    Fall(Box<Expr>),
    Sequence { a: Box<Expr>, b: Box<Expr>, within_ps: Option<u64> },
    PrecededBy { a: Box<Expr>, b: Box<Expr>, within_ps: Option<u64> },
    Mask(Box<Expr>, u64),
    Shift(Box<Expr>, u8),
}
```

Evaluation is a single pass over the union of transition points of all signals referenced in the expression. Only the signals actually used by the predicate are loaded from the FST file.

---

## 4  Query API

All operations are Python functions that delegate to Rust via PyO3. Timestamps are accepted in human-readable form at the boundary and normalised to picoseconds internally before crossing into Rust.

### 4.1  Time Normalisation

```python
parse_time("1284ns")   →  1_284_000   # picoseconds
parse_time("1.284us")  →  1_284_000
parse_time(1_284_000)  →  1_284_000   # raw ps passthrough
parse_time("642cyc")   →  642 * timescale_ps
```

### 4.2  Discovery

```python
list_signals(pattern: str) -> list[SignalInfo]
# SignalInfo: path, width, total_transitions, first_change_ps, last_change_ps

signal_info(signal: str) -> SignalInfo

list_scopes(prefix: str) -> list[str]
# hierarchical browse: list_scopes("tb.dut.core") → ["tb.dut.core.iq", ...]

waveform_info() -> WaveformInfo
# timescale, total duration, signal count, simulator version, date
```

### 4.3  Point Queries

```python
get_value(signal: str, time_ps: int) -> Value
# Value: { hex: str, is_x: bool, is_z: bool }

get_snapshot(signals: list[str], time_ps: int) -> dict[str, Value]
# single wellen pass over all signals — preferred over multiple get_value calls
```

### 4.4  Range Queries

```python
get_transitions(
    signal: str,
    t0_ps: int,
    t1_ps: int,
    max_edges: int = 1000,       # hard cap; returns Summary if exceeded
) -> TransitionResult

get_window(
    signals: list[str],
    t0_ps: int,
    t1_ps: int,
    max_edges_per_signal: int = 200,
) -> dict[str, TransitionResult]
# multi-signal range query in a single pass — the main workhorse
```

### 4.5  Search

```python
find_next_edge(
    signal: str,
    direction: Literal["rising", "falling", "any"],
    after_ps: int,
) -> int | None

find_first(predicate: Expr, after_ps: int) -> int | None
# entire scan in Rust; single PyO3 call

find_all(predicate: Expr, t0_ps: int, t1_ps: int) -> list[int]
# all timestamps where predicate is true in window

scan(predicate: Expr, t0_ps: int, t1_ps: int) -> list[tuple[int, Value]]
# evaluate predicate at every transition point; used to build transaction decoders
```

### 4.6  Summarisation

Summarisation happens in Rust before data crosses to Python. This is the mechanism that prevents large waveform windows from flooding Claude's context window.

```python
summarize(signal: str, t0_ps: int, t1_ps: int) -> Summary
# Summary: total_transitions, dominant_period_ps, duty_cycle,
#          value_histogram (non-clock signals), anomalies list

summarize_window(signals: list[str], t0_ps: int, t1_ps: int) -> dict[str, Summary]

find_anomalies(
    signal: str,
    t0_ps: int,
    t1_ps: int,
    expected_period_ps: int | None = None,   # inferred if None
) -> list[Anomaly]
# Anomaly: { time_ps, kind: "glitch" | "gap" | "stuck", detail }
```

Example summarised output for a 10µs window with 48k transitions:

```json
{
  "total_transitions": 48291,
  "dominant_period_ps": 2000,
  "anomalies": [
    { "t": 1284300, "kind": "gap", "gap_ps": 6000, "expected_ps": 2000 },
    { "t": 2019100, "kind": "gap", "gap_ps": 4000, "expected_ps": 2000 }
  ]
}
```

---

## 5  µScope Semantic Layer

### 5.1  Per-Design Configuration

Signal path mappings are centralised in `config.py`. This is the only file that changes when signal hierarchy changes in the RTL. All decoders reference it, so a path rename is a one-line fix.

```python
# config.py — per-design signal name mappings
TILELINK = {
    "a_valid":   "tb.dut.core.tl_out_a_valid",
    "a_ready":   "tb.dut.core.tl_out_a_ready",
    "a_opcode":  "tb.dut.core.tl_out_a_bits_opcode",
    "a_address": "tb.dut.core.tl_out_a_bits_address",
    "a_source":  "tb.dut.core.tl_out_a_bits_source",
}

ISSUE_QUEUE = {
    "entry_valid":    "tb.dut.core.iq.entry_valid",
    "entry_src1_rdy": "tb.dut.core.iq.entry_src1_ready",
    "issue_sel":      "tb.dut.core.iq.issue_select",
    "issue_pc":       "tb.dut.core.iq.issue_pc",
}
```

### 5.2  Protocol Decoders

Each decoder is a Python class that uses the predicate DSL and query API to reconstruct domain-level objects from raw signals. Heavy scanning is delegated to Rust; Python handles orchestration only.

```python
class TileLinkDecoder:
    def get_transactions(self, t0_ps, t1_ps) -> list[TLTransaction]:
        # 1. find all A-channel handshakes via find_all()
        # 2. snapshot opcode/address/source at each handshake time
        # 3. find matching D-channel response by source ID
        # 4. return decoded {opcode, address, source, latency_ps, ...}

    def find_violations(self, t0_ps, t1_ps) -> list[TLViolation]:
        # Grant without pending Acquire, source ID reuse, etc.
```

Full transaction reconstruction example:

```python
def get_transactions(self, t0_ps, t1_ps):
    cfg = TILELINK
    with engine.signals(**cfg) as s:
        a_handshake = s.a_valid & s.a_ready
    req_times = engine.find_all(a_handshake, t0_ps, t1_ps)

    transactions = []
    for t_req in req_times:
        snap = engine.get_snapshot(
            [cfg["a_opcode"], cfg["a_address"], cfg["a_source"]], t_req
        )
        d_match = engine.signals(
            dv = cfg["d_valid"], dr = cfg["d_ready"], ds = cfg["d_source"]
        )
        with d_match as s:
            resp_pred = s.dv & s.dr & (s.ds == int(snap[cfg["a_source"]], 16))
        t_resp = engine.find_first(resp_pred, after_ps=t_req)

        transactions.append(TLTransaction(
            opcode   = snap[cfg["a_opcode"]],
            address  = snap[cfg["a_address"]],
            t_req    = t_req,
            t_resp   = t_resp,
            latency_ps = t_resp - t_req if t_resp else None,
        ))
    return transactions
```

### 5.3  Pipeline Tracer

The pipeline tracer follows a single instruction through all stages using a sequence of `find_first()` calls, one per stage boundary. Results are returned as a structured timeline that Claude can reason over directly.

```python
class IssueQueueDecoder:
    def trace_instruction(self, pc: int, dispatch_time_ps: int) -> IQTrace:
        # Returns per-stage timeline:
        # dispatch → rename → IQ allocation → src ready → issue / squash
        # with timestamps and operand values at each stage

    def find_phantom_wakeups(self, t0_ps, t1_ps) -> list[PhantomWakeup]:
        # Instruction issued while a source pReg was never marked ready,
        # or was marked ready then squashed before issue.
```

Example Claude Code session output from `trace_instruction`:

```
Instruction PC=0x80341c dispatched at t=1.200µs
  Renamed:   pReg42 ← pReg17 + pReg31  (t=1.214µs)
  IQ entry:  slot 7 allocated            (t=1.214µs)
  pReg17:    ready                       (t=1.310µs)
  pReg31:    wakeup broadcast            (t=1.400µs)  ← producer squashed at 1.398µs
  Issued:    t=1.440µs  ← src2 not actually ready
  → PHANTOM WAKEUP: pReg31 producer (PC=0x80338c) squashed 2ns before issue
```

---

## 6  MCP Tool Surface

MCP tools are the interface Claude Code sees. Each tool maps to one or a small composition of query API calls. Tool descriptions guide Claude toward the right query for each debugging task.

### 6.1  wellen MCP Tools

| Tool                | Inputs              | Description                                                          |
| ------------------- | ------------------- | -------------------------------------------------------------------- |
| `search_signals`    | `pattern: str`      | Signal discovery. Always the first call in a session.                |
| `get_signal_window` | `signals[], t0, t1` | Multi-signal transitions. Auto-summarises if > 200 edges per signal. |
| `find_first`        | `predicate, after`  | First timestamp matching a predicate expression.                     |
| `find_all`          | `predicate, t0, t1` | All matching timestamps in window.                                   |
| `get_snapshot`      | `signals[], time`   | Values of all signals at a single point. Cheap multi-signal lookup.  |
| `find_anomalies`    | `signal, t0, t1`    | Glitches, unexpected gaps, stuck signals.                            |
| `waveform_info`     | —                   | Timescale, duration, signal count.                                   |

### 6.2  µScope MCP Tools

| Tool                     | Inputs                   | Description                                                                                    |
| ------------------------ | ------------------------ | ---------------------------------------------------------------------------------------------- |
| `decode_tilelink`        | `t0, t1`                 | Decoded TileLink transactions: address, opcode, source, latency. Includes protocol violations. |
| `decode_axi`             | `prefix, t0, t1`         | AXI4 burst transactions decoded from raw signals.                                              |
| `trace_instruction`      | `pc_hex, hint_time`      | Full pipeline timeline for one instruction: dispatch → rename → IQ → issue/squash.             |
| `find_assertion_context` | `signal, time, window`   | Snapshot all related signals around an assertion fire. Traces asserting signal backwards.      |
| `find_phantom_wakeups`   | `t0, t1`                 | Instructions that issued despite an operand source being unready or squashed.                  |
| `find_first_divergence`  | `signal_a, signal_b, t0` | First time two signals that should match diverge. DUT vs reference comparison.                 |

### 6.3  CLI Mode

The same tool functions are exposed as a CLI for interactive use without Claude Code:

```bash
# MCP mode (Claude Code)
waveform-mcp serve sim.fst

# CLI mode (interactive)
waveform-mcp query sim.fst signals "*tl*valid*"
waveform-mcp query sim.fst transitions tb.dut.tl_a_valid 1200ns 1400ns
waveform-mcp query sim.fst snapshot 1284ns tb.dut.tl_a_valid tb.dut.tl_a_ready
waveform-mcp query sim.fst anomalies tb.dut.clk 0 10us
```

---

## 7  Repository Structure

```
waveform-mcp/
  waveform_engine/           ← Rust crate (PyO3)
    src/
      lib.rs                 ← PyO3 module entry, exported functions
      query.rs               ← get_transitions, get_snapshot, find_first ...
      predicate.rs           ← Expr enum, evaluation engine
      summarise.rs           ← anomaly detection, period inference
    Cargo.toml               ← wellen + pyo3 + serde dependencies
  mcp/
    wellen_server.py         ← wellen MCP tool definitions
    uscope_server.py         ← µScope MCP tool definitions
    predicate.py             ← Python DSL (Expr dataclasses + operators)
    time_parse.py            ← ns/us/cyc → ps normalisation
    config.py                ← per-design signal name mappings
    decoders/
      tilelink.py
      axi4.py
      issue_queue.py
      pipeline.py
  cli.py                     ← interactive CLI (same tools, argparse frontend)
  pyproject.toml             ← maturin build config
  CLAUDE.md                  ← design context for Claude Code sessions
```

---

## 8  Implementation Plan

| Phase                | Deliverable                                                                              | Notes                                                                      |
| -------------------- | ---------------------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| 1 — Rust foundation  | `waveform_engine.so` with `get_value`, `get_transitions`, `get_snapshot`, `list_signals` | wellen as backend. Validate on real FST. CLI smoke test.                   |
| 2 — Predicate engine | Expr AST in Python + evaluation in Rust. `find_first`, `find_all`, `scan`.               | AND/OR/NOT/EQ/RISE/FALL first. Sequence (`>>`) in phase 3.                 |
| 3 — Summarisation    | `summarize`, `find_anomalies` in Rust. Auto-summarise in `get_window`.                   | Period inference, glitch detection. Validate context window stays bounded. |
| 4 — wellen MCP       | Full MCP server wired to Rust engine. Claude Code config.                                | Verify end-to-end: Claude Code → MCP → PyO3 → wellen → FST.                |
| 5 — TileLink decoder | `decode_tilelink`, `find_violations`. Config-driven signal paths.                        | Test on existing TileLink sim traces.                                      |
| 6 — Pipeline decoder | `trace_instruction`, `find_phantom_wakeups`.                                             | Design-specific. Requires IQ signal mapping in config.                     |
| 7 — µScope MCP       | Second MCP server with semantic tools.                                                   | Thin layer; heavy work already in wellen MCP + Rust.                       |

---

## 9  Key Design Decisions

### PyO3 over subprocess

A separate Rust binary speaking a JSON or line protocol over stdio was considered. PyO3 was chosen because it eliminates serialisation overhead entirely, allows Rust to own the scan loop without data copying, and simplifies deployment to a single pip-installable package built with maturin.

### wellen over fstapi FFI

The `wellen` crate was chosen over a direct C FFI to GTKWave's fstapi because it is written in safe Rust, is actively maintained (it is the backend of the Surfer waveform viewer), and provides a lazy subset-access model that matches the query engine's access pattern. FST, VCD, and GHW are all supported through the same interface.

### WAL rejected

WAL (Waveform Analysis Language) was evaluated and rejected. The language is a Lisp-style DSL that is awkward to generate and maintain. Its Python backend (pylibfst) is too slow for large simulation traces. The predicate DSL described in Section 3 provides equivalent expressiveness with native Python syntax and Rust evaluation performance.

### Two MCPs, not one

Separating the generic wellen MCP from the µScope semantic MCP keeps each independently useful. The wellen MCP works on any FST/VCD file with no design knowledge. The µScope MCP adds Loom-specific decoders on top. Claude Code can use both in the same session.

### µScope not derived from wellen

µScope is a purely transaction-oriented format — it does not store signal-granularity data. Attempting to derive wellen from µScope would require reconstructing synthetic signals from transaction fields, which is lossy for sub-transaction behaviour. Instead µScope is a separate semantic view consumed by the µScope MCP, while the wellen MCP handles FST/VCD from the same simulation run when sub-transaction visibility is needed.

---

## 10  Future Extensions

- CHI and custom protocol decoders following the same decoder pattern
- SVA-style temporal logic in the predicate engine (`always`, `eventually`, `until`)
- Formal verification trace support (counterexample traces from model checkers)
- VSCode extension integrating waveform queries alongside RTL source navigation
- Automatic CLAUDE.md generation from waveform hierarchy for cold-start sessions
