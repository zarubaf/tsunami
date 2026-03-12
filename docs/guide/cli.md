# CLI Guide

The `tsunami` CLI provides direct access to all query functions from the terminal.

## Commands

### `tsunami info`

Show waveform metadata.

```bash
tsunami info sim.fst
```

```
  timescale_factor: 100
  timescale_unit: PicoSeconds
  duration: 645
  num_signals: 38647
  file_format: Fst
```

### `tsunami signals`

Search for signals using glob patterns.

```bash
# All signals
tsunami signals sim.fst

# Clock signals
tsunami signals sim.fst "*clk*"

# TileLink A-channel
tsunami signals sim.fst "*tl_a*"
```

```
  [   1] tb.dut.tl_a_valid
  [   1] tb.dut.tl_a_ready
  [   4] tb.dut.tl_a_opcode
  [  32] tb.dut.tl_a_address

  4 signal(s) matched
```

### `tsunami scopes`

Browse the design hierarchy.

```bash
tsunami scopes sim.fst "tb.dut"
```

### `tsunami value`

Get a signal's value at a specific time.

```bash
tsunami value sim.fst tb.dut.clk 100ps
tsunami value sim.fst tb.dut.tl_a_opcode 1284ns
```

```
  tb.dut.clk @ 100ps = 0x1
```

### `tsunami transitions`

Get all value changes in a time range.

```bash
tsunami transitions sim.fst tb.dut.clk 0 1000ps
tsunami transitions sim.fst tb.dut.tl_a_valid 1us 2us --max-edges 50
```

### `tsunami snapshot`

Get multiple signal values at a single time point.

```bash
tsunami snapshot sim.fst 1284ns tb.dut.tl_a_valid tb.dut.tl_a_ready tb.dut.tl_a_opcode
```

```
  Snapshot @ 1284000ps:
    tb.dut.tl_a_valid = 0x1
    tb.dut.tl_a_ready = 0x1
    tb.dut.tl_a_opcode = 0x4
```

### `tsunami summarize`

Compute signal statistics over a time window.

```bash
tsunami summarize sim.fst tb.dut.clk 0 10us
```

```
  Summary for tb.dut.clk [0, 10000000]:
    total_transitions: 20000
    dominant_period_ps: 1000
    duty_cycle: 0.500
```

### `tsunami anomalies`

Detect glitches, gaps, and stuck signals.

```bash
tsunami anomalies sim.fst tb.dut.clk 0 10us
tsunami anomalies sim.fst tb.dut.data_valid 0 10us --expected-period 2000
```

### `tsunami serve`

Start the MCP server for Claude Code integration.

```bash
tsunami serve sim.fst
```

See the [MCP Server guide](mcp.md) for configuration details.
