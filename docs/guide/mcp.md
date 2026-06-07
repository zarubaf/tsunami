# MCP Server Guide

Tsunami includes an [MCP](https://modelcontextprotocol.io/) server that lets
Claude Code query waveform files directly.

## Starting the server

```bash
tsunami serve simulation.fst
```

The server communicates over stdio using the MCP protocol.

If you start the server with a waveform path, that preloaded waveform is
available under the reserved session ID `default`.

## Claude Code configuration

Add to your `.mcp.json` (project-level) or `~/.claude/mcp.json` (global):

```json
{
  "mcpServers": {
    "tsunami": {
      "command": "tsunami",
      "args": ["serve", "path/to/simulation.fst"]
    }
  }
}
```

After restarting Claude Code, the tsunami tools will be available.

## Sessions

Tsunami's MCP tools are session-based.

- Call `open_waveform(path)` to open an additional waveform and get a
  `session_id`.
- Pass `session_id` to all stateful waveform tools.
- If the server was started with `tsunami serve path/to/file.fst`, use
  `session_id="default"` for that preloaded waveform.

## Available tools

### `open_waveform`

Open a waveform file and return a `session_id` plus waveform metadata.

**Parameters:**

- `path` (str): Absolute path to the waveform file.

### `waveform_info`

Returns waveform metadata: timescale, duration, signal count, format.

**Parameters:**

- `session_id` (str): Waveform session ID.

### `search_signals`

Signal discovery with glob patterns. This is typically the first tool Claude
will call.

**Parameters:**

- `session_id` (str): Waveform session ID.
- `pattern` (str, default `"*"`): Glob pattern for signal names.

### `browse_scopes`

Browse the design hierarchy.

**Parameters:**

- `session_id` (str): Waveform session ID.
- `prefix` (str, default `""`): Only scopes starting with this prefix.

### `get_signal_info`

Get metadata for a single signal.

**Parameters:**

- `session_id` (str): Waveform session ID.
- `signal` (str): Full hierarchical signal path.

### `get_snapshot`

Get values of multiple signals at a single time point.

**Parameters:**

- `session_id` (str): Waveform session ID.
- `signals` (list[str]): Signal paths.
- `time` (str | int): Time point (e.g. `"1284ns"`).

### `get_signal_window`

Get transitions for multiple signals in a time range. Automatically summarises
signals with more than `max_edges_per_signal` transitions — this prevents
flooding the context window.

**Parameters:**

- `session_id` (str): Waveform session ID.
- `signals` (list[str]): Signal paths.
- `t0`, `t1` (str | int): Time range.
- `max_edges_per_signal` (int, default 200): Threshold for auto-summarisation.

### `find_first_match`

Find the first timestamp matching a predicate expression.

**Parameters:**

- `session_id` (str): Waveform session ID.
- `predicate_json` (str): JSON-encoded predicate AST.
- `after` (str | int, default 0): Search after this time.

**Example predicate JSON:**

```json
{
  "tag": "and",
  "left": {"tag": "signal", "path": "tb.dut.tl_a_valid"},
  "right": {"tag": "signal", "path": "tb.dut.tl_a_ready"}
}
```

### `find_all_matches`

Find all timestamps matching a predicate in a window.

**Parameters:**

- `session_id` (str): Waveform session ID.
- `predicate_json` (str): JSON-encoded predicate AST.
- `t0`, `t1` (str | int): Time range.

### `find_anomalies`

Detect glitches, gaps, and stuck signals.

**Parameters:**

- `session_id` (str): Waveform session ID.
- `signal` (str): Signal path.
- `t0`, `t1` (str | int): Time range.
- `expected_period_ps` (int | None): Expected period (auto-inferred if omitted).

## Predicate JSON format

The MCP server accepts predicates as JSON objects. Each node has a `tag` field:

| Tag | Fields | Example |
|---|---|---|
| `signal` | `path` | `{"tag": "signal", "path": "tb.dut.clk"}` |
| `const` | `value` | `{"tag": "const", "value": 4}` |
| `and` | `left`, `right` | `{"tag": "and", "left": ..., "right": ...}` |
| `or` | `left`, `right` | |
| `not` | `inner` | `{"tag": "not", "inner": ...}` |
| `xor` | `left`, `right` | |
| `eq` | `left`, `right` | |
| `gt` | `left`, `right` | |
| `lt` | `left`, `right` | |
| `rise` | `inner` | `{"tag": "rise", "inner": {"tag": "signal", "path": "..."}}` |
| `fall` | `inner` | |
| `bit_slice` | `inner`, `high`, `low` | |
| `sequence` | `a`, `b`, `within_ps` | |
| `preceded_by` | `a`, `b`, `within_ps` | |

Shorthand: a plain string is treated as a signal path, and a number as a constant.
