"""MCP Server — exposes tsunami waveform tools via FastMCP."""

from __future__ import annotations

import json
import sys

from mcp.server.fastmcp import FastMCP

import tsunami._engine as engine
from tsunami.time_parse import parse_time
from tsunami.predicate import (
    Signal, Const, And, Or, Not, Xor, Eq, Gt, Lt,
    Rise, Fall, BitSlice, Sequence, PrecededBy,
)

mcp = FastMCP("tsunami")

# Global waveform handle — set dynamically via open_waveform tool or at startup
_handle = None
_timescale_ps = None

# Hard ceiling on how many items any single tool call may return, regardless of
# the caller-requested `limit`. Protects the stdio transport: a single
# unanchored glob (e.g. "*") can match hundreds of thousands of signals in a
# large design, and serializing all of them in one response can exceed the
# MCP client's message-framing limits and force a disconnect.
MAX_RESULT_LIMIT = 500
DEFAULT_RESULT_LIMIT = 200


def _paginate(items: list, limit: int, offset: int) -> tuple[list, int]:
    """Slice `items` to a bounded page and report the total match count.

    Raises ValueError for invalid limit/offset so callers get a clear error
    instead of a silently-wrong (or silently-huge) response.
    """
    if limit < 1:
        raise ValueError("limit must be at least 1")
    if offset < 0:
        raise ValueError("offset must be at least 0")
    limit = min(limit, MAX_RESULT_LIMIT)
    total = len(items)
    return items[offset:offset + limit], total


def _load_waveform(fst_path: str):
    """Open a waveform file and set the global handle."""
    global _handle, _timescale_ps

    _handle = engine.open(fst_path)
    info = engine.waveform_info(_handle)
    factor = info.get("timescale_factor", 1)
    unit = info.get("timescale_unit", "ps")
    unit_ps = {
        "FemtoSeconds": 0.001,
        "PicoSeconds": 1,
        "NanoSeconds": 1_000,
        "MicroSeconds": 1_000_000,
        "MilliSeconds": 1_000_000_000,
        "Seconds": 1_000_000_000_000,
    }.get(unit, 1)
    _timescale_ps = int(factor * unit_ps)
    return info


def _get_handle():
    if _handle is None:
        raise RuntimeError("No waveform loaded. Call open_waveform(path) first.")
    return _handle


def _parse_t(value: str | int) -> int:
    return parse_time(value, timescale_ps=_timescale_ps)


def _expr_from_json(data: dict | str) -> object:
    """Recursively build an Expr from a JSON-serializable dict."""
    if isinstance(data, str):
        # Assume signal path
        return Signal(data)
    if isinstance(data, (int, float)):
        return Const(int(data))

    tag = data.get("tag", data.get("type", ""))

    if tag == "signal":
        return Signal(data["path"])
    elif tag == "const":
        return Const(data["value"])
    elif tag == "and":
        return And(left=_expr_from_json(data["left"]), right=_expr_from_json(data["right"]))
    elif tag == "or":
        return Or(left=_expr_from_json(data["left"]), right=_expr_from_json(data["right"]))
    elif tag == "not":
        return Not(inner=_expr_from_json(data["inner"]))
    elif tag == "xor":
        return Xor(left=_expr_from_json(data["left"]), right=_expr_from_json(data["right"]))
    elif tag == "eq":
        return Eq(left=_expr_from_json(data["left"]), right=_expr_from_json(data["right"]))
    elif tag == "gt":
        return Gt(left=_expr_from_json(data["left"]), right=_expr_from_json(data["right"]))
    elif tag == "lt":
        return Lt(left=_expr_from_json(data["left"]), right=_expr_from_json(data["right"]))
    elif tag == "rise":
        return Rise(inner=_expr_from_json(data["inner"]))
    elif tag == "fall":
        return Fall(inner=_expr_from_json(data["inner"]))
    elif tag == "bit_slice":
        return BitSlice(inner=_expr_from_json(data["inner"]), high=data["high"], low=data["low"])
    elif tag == "sequence":
        return Sequence(
            a=_expr_from_json(data["a"]),
            b=_expr_from_json(data["b"]),
            within_ps=data.get("within_ps"),
        )
    elif tag == "preceded_by":
        return PrecededBy(
            a=_expr_from_json(data["a"]),
            b=_expr_from_json(data["b"]),
            within_ps=data.get("within_ps"),
        )
    else:
        raise ValueError(f"Unknown expression tag: {tag}")


@mcp.tool()
def open_waveform(path: str) -> dict:
    """Open a waveform file (FST or VCD). Must be called before any other tool.

    Returns waveform metadata: timescale, duration, signal count, format.

    Args:
        path: Absolute path to the waveform file.
    """
    return _load_waveform(path)


@mcp.tool()
def waveform_info() -> dict:
    """Get waveform metadata: timescale, duration, signal count, format."""
    return engine.waveform_info(_get_handle())


@mcp.tool()
def search_signals(
    pattern: str = "*", limit: int = DEFAULT_RESULT_LIMIT, offset: int = 0
) -> dict:
    """Search for signals matching a glob pattern. Always start here for signal discovery.

    Examples: "*clk*", "tb.dut.*valid*", "*tl_a*"

    Results are paginated: the response includes `total_count` so you can tell
    whether it was truncated, and `offset` can be used to page through the rest.
    Narrow the pattern instead of raising `limit` when `total_count` is very
    large — the underlying design may have hundreds of thousands of signals.

    Args:
        pattern: Glob pattern (default: "*")
        limit: Max signals to return (default 200, hard-capped at 500)
        offset: Number of matches to skip, for paging through results (default 0)
    """
    matches = engine.list_signals(_get_handle(), pattern)
    page, total = _paginate(matches, limit, offset)
    return {
        "pattern": pattern,
        "limit": limit,
        "offset": offset,
        "total_count": total,
        "signals": page,
    }


@mcp.tool()
def browse_scopes(
    prefix: str = "", limit: int = DEFAULT_RESULT_LIMIT, offset: int = 0
) -> dict:
    """Browse the signal hierarchy. Returns scope names under the given prefix.

    Results are paginated: the response includes `total_count` so you can tell
    whether it was truncated, and `offset` can be used to page through the rest.
    Narrow the prefix instead of raising `limit` when `total_count` is very
    large — the underlying design may have hundreds of thousands of scopes.

    Args:
        prefix: Scope prefix (default: "", i.e. top-level scopes)
        limit: Max scopes to return (default 200, hard-capped at 500)
        offset: Number of matches to skip, for paging through results (default 0)
    """
    matches = engine.list_scopes(_get_handle(), prefix)
    page, total = _paginate(matches, limit, offset)
    return {
        "prefix": prefix,
        "limit": limit,
        "offset": offset,
        "total_count": total,
        "scopes": page,
    }


@mcp.tool()
def get_snapshot(signals: list[str], time: str | int) -> dict:
    """Get values of multiple signals at a single time point. Efficient multi-signal lookup.

    Args:
        signals: List of signal paths (e.g., ["tb.dut.clk", "tb.dut.reset"])
        time: Time point (e.g., "1284ns", "1.284us", 1284000)
    """
    t = _parse_t(time)
    return engine.get_snapshot(_get_handle(), signals, t)


@mcp.tool()
def get_signal_window(
    signals: list[str],
    t0: str | int,
    t1: str | int,
    max_edges_per_signal: int = 200,
) -> dict:
    """Get transitions for multiple signals in a time window.

    Auto-summarises if a signal has more than max_edges_per_signal transitions.

    Args:
        signals: List of signal paths
        t0: Start time
        t1: End time
        max_edges_per_signal: Max edges before auto-summarise (default 200)
    """
    handle = _get_handle()
    t0_ps = _parse_t(t0)
    t1_ps = _parse_t(t1)

    result = {}
    for sig in signals:
        transitions = engine.get_transitions(handle, sig, t0_ps, t1_ps, max_edges_per_signal)
        if transitions["truncated"]:
            # Auto-summarise
            summary = engine.summarize(handle, sig, t0_ps, t1_ps)
            result[sig] = {
                "mode": "summary",
                "total_transitions": transitions["total_transitions"],
                **summary,
            }
        else:
            result[sig] = {
                "mode": "transitions",
                **transitions,
            }
    return result


@mcp.tool()
def find_first_match(predicate_json: str, after: str | int = 0) -> int | None:
    """Find first timestamp matching a predicate expression.

    Args:
        predicate_json: JSON-encoded predicate AST (see predicate DSL docs)
        after: Search after this time (default: 0)

    Example predicate_json:
        {"tag": "and", "left": {"tag": "signal", "path": "tb.dut.valid"},
         "right": {"tag": "signal", "path": "tb.dut.ready"}}
    """
    data = json.loads(predicate_json)
    expr = _expr_from_json(data)
    after_ps = _parse_t(after)
    return engine.find_first(_get_handle(), expr, after_ps)


@mcp.tool()
def find_all_matches(
    predicate_json: str,
    t0: str | int,
    t1: str | int,
    limit: int = DEFAULT_RESULT_LIMIT,
    offset: int = 0,
) -> dict:
    """Find all timestamps matching a predicate expression in a window.

    Results are paginated: the response includes `total_count` so you can tell
    whether it was truncated. Narrow the window instead of raising `limit`
    when `total_count` is very large — a broad predicate over a long window
    can match on a large fraction of all time points.

    Args:
        predicate_json: JSON-encoded predicate AST
        t0: Start time
        t1: End time
        limit: Max timestamps to return (default 200, hard-capped at 500)
        offset: Number of matches to skip, for paging through results (default 0)
    """
    data = json.loads(predicate_json)
    expr = _expr_from_json(data)
    t0_ps = _parse_t(t0)
    t1_ps = _parse_t(t1)
    matches = engine.find_all(_get_handle(), expr, t0_ps, t1_ps)
    page, total = _paginate(matches, limit, offset)
    return {
        "t0_ps": t0_ps,
        "t1_ps": t1_ps,
        "limit": limit,
        "offset": offset,
        "total_count": total,
        "matches": page,
    }


@mcp.tool()
def find_anomalies(
    signal: str,
    t0: str | int,
    t1: str | int,
    expected_period_ps: int | None = None,
) -> list[dict]:
    """Detect anomalies in a signal: glitches, unexpected gaps, stuck signals.

    Args:
        signal: Signal path
        t0: Start time
        t1: End time
        expected_period_ps: Expected period (auto-inferred if not provided)
    """
    t0_ps = _parse_t(t0)
    t1_ps = _parse_t(t1)
    return engine.find_anomalies(_get_handle(), signal, t0_ps, t1_ps, expected_period_ps)


def start_server(fst_path: str | None = None):
    """Start the MCP server, optionally pre-loading a waveform file."""
    if fst_path:
        _load_waveform(fst_path)

    mcp.run(transport="stdio")
