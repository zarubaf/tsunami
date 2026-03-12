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

# Global waveform handle — set when server starts
_handle = None
_timescale_ps = None


def _get_handle():
    if _handle is None:
        raise RuntimeError("No waveform file loaded. Start with: tsunami serve <file.fst>")
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
def waveform_info() -> dict:
    """Get waveform metadata: timescale, duration, signal count, format."""
    return engine.waveform_info(_get_handle())


@mcp.tool()
def search_signals(pattern: str = "*") -> list[dict]:
    """Search for signals matching a glob pattern. Always start here for signal discovery.

    Examples: "*clk*", "tb.dut.*valid*", "*tl_a*"
    """
    return engine.list_signals(_get_handle(), pattern)


@mcp.tool()
def browse_scopes(prefix: str = "") -> list[str]:
    """Browse the signal hierarchy. Returns scope names under the given prefix."""
    return engine.list_scopes(_get_handle(), prefix)


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
def find_all_matches(predicate_json: str, t0: str | int, t1: str | int) -> list[int]:
    """Find all timestamps matching a predicate expression in a window.

    Args:
        predicate_json: JSON-encoded predicate AST
        t0: Start time
        t1: End time
    """
    data = json.loads(predicate_json)
    expr = _expr_from_json(data)
    t0_ps = _parse_t(t0)
    t1_ps = _parse_t(t1)
    return engine.find_all(_get_handle(), expr, t0_ps, t1_ps)


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


def start_server(fst_path: str):
    """Initialize the waveform handle and start the MCP server."""
    global _handle, _timescale_ps

    _handle = engine.open(fst_path)
    info = engine.waveform_info(_handle)
    # Compute timescale in picoseconds
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

    mcp.run(transport="stdio")
