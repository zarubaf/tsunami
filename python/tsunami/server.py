"""MCP Server — exposes tsunami waveform tools via FastMCP."""

from __future__ import annotations

import json
from dataclasses import dataclass

from mcp.server.fastmcp import FastMCP

import tsunami._engine as engine
from tsunami.predicate import (
    And,
    BitSlice,
    Const,
    Eq,
    Fall,
    Gt,
    Lt,
    Not,
    Or,
    PrecededBy,
    Rise,
    Sequence,
    Signal,
    Xor,
)
from tsunami.time_parse import parse_time

mcp = FastMCP("tsunami")

DEFAULT_SESSION_ID = "default"


@dataclass
class WaveformSession:
    handle: object
    timescale_ps: int
    path: str


_sessions: dict[str, WaveformSession] = {}
_next_session_id = 1


def _timescale_ps_from_info(info: dict) -> int:
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
    return int(factor * unit_ps)


def _new_session_id() -> str:
    global _next_session_id
    session_id = f"session-{_next_session_id}"
    _next_session_id += 1
    return session_id


def _create_session(path: str, session_id: str | None = None) -> tuple[str, dict]:
    handle = engine.open(path)
    info = engine.waveform_info(handle)
    assigned_session_id = session_id or _new_session_id()
    _sessions[assigned_session_id] = WaveformSession(
        handle=handle,
        timescale_ps=_timescale_ps_from_info(info),
        path=path,
    )
    return assigned_session_id, info


def _get_session(session_id: str) -> WaveformSession:
    try:
        return _sessions[session_id]
    except KeyError as exc:
        raise RuntimeError(
            f"Unknown waveform session '{session_id}'. Call open_waveform(path) first."
        ) from exc


def _parse_t(session: WaveformSession, value: str | int) -> int:
    return parse_time(value, timescale_ps=session.timescale_ps)


def _expr_from_json(data: dict | str) -> object:
    """Recursively build an Expr from a JSON-serializable dict."""
    if isinstance(data, str):
        return Signal(data)
    if isinstance(data, (int, float)):
        return Const(int(data))

    tag = data.get("tag", data.get("type", ""))

    if tag == "signal":
        return Signal(data["path"])
    if tag == "const":
        return Const(data["value"])
    if tag == "and":
        return And(left=_expr_from_json(data["left"]), right=_expr_from_json(data["right"]))
    if tag == "or":
        return Or(left=_expr_from_json(data["left"]), right=_expr_from_json(data["right"]))
    if tag == "not":
        return Not(inner=_expr_from_json(data["inner"]))
    if tag == "xor":
        return Xor(left=_expr_from_json(data["left"]), right=_expr_from_json(data["right"]))
    if tag == "eq":
        return Eq(left=_expr_from_json(data["left"]), right=_expr_from_json(data["right"]))
    if tag == "gt":
        return Gt(left=_expr_from_json(data["left"]), right=_expr_from_json(data["right"]))
    if tag == "lt":
        return Lt(left=_expr_from_json(data["left"]), right=_expr_from_json(data["right"]))
    if tag == "rise":
        return Rise(inner=_expr_from_json(data["inner"]))
    if tag == "fall":
        return Fall(inner=_expr_from_json(data["inner"]))
    if tag == "bit_slice":
        return BitSlice(inner=_expr_from_json(data["inner"]), high=data["high"], low=data["low"])
    if tag == "sequence":
        return Sequence(
            a=_expr_from_json(data["a"]),
            b=_expr_from_json(data["b"]),
            within_ps=data.get("within_ps"),
        )
    if tag == "preceded_by":
        return PrecededBy(
            a=_expr_from_json(data["a"]),
            b=_expr_from_json(data["b"]),
            within_ps=data.get("within_ps"),
        )
    raise ValueError(f"Unknown expression tag: {tag}")


@mcp.tool()
def open_waveform(path: str) -> dict:
    """Open a waveform file (FST or VCD) and return a session ID.

    Args:
        path: Absolute path to the waveform file.
    """
    session_id, info = _create_session(path)
    return {
        "session_id": session_id,
        "path": path,
        **info,
    }


@mcp.tool()
def waveform_info(session_id: str) -> dict:
    """Get waveform metadata for an open waveform session."""
    session = _get_session(session_id)
    return {
        "session_id": session_id,
        "path": session.path,
        **engine.waveform_info(session.handle),
    }


@mcp.tool()
def search_signals(session_id: str, pattern: str = "*") -> dict:
    """Search for signals matching a glob pattern in a waveform session.

    Examples: "*clk*", "tb.dut.*valid*", "*tl_a*"
    """
    session = _get_session(session_id)
    return {
        "session_id": session_id,
        "signals": engine.list_signals(session.handle, pattern),
    }


@mcp.tool()
def browse_scopes(session_id: str, prefix: str = "") -> dict:
    """Browse the signal hierarchy for an open waveform session."""
    session = _get_session(session_id)
    return {
        "session_id": session_id,
        "scopes": engine.list_scopes(session.handle, prefix),
    }


@mcp.tool()
def get_signal_info(session_id: str, signal: str) -> dict:
    """Get metadata for a single signal in a waveform session."""
    session = _get_session(session_id)
    return {
        "session_id": session_id,
        "signal_info": engine.get_signal_info(session.handle, signal),
    }


@mcp.tool()
def get_snapshot(session_id: str, signals: list[str], time: str | int) -> dict:
    """Get values of multiple signals at a single time point.

    Args:
        session_id: Waveform session returned by open_waveform.
        signals: List of signal paths.
        time: Time point (e.g., "1284ns", "1.284us", 1284000)
    """
    session = _get_session(session_id)
    t = _parse_t(session, time)
    return {
        "session_id": session_id,
        "time_ps": t,
        "values": engine.get_snapshot(session.handle, signals, t),
    }


@mcp.tool()
def get_signal_window(
    session_id: str,
    signals: list[str],
    t0: str | int,
    t1: str | int,
    max_edges_per_signal: int = 200,
) -> dict:
    """Get transitions for multiple signals in a time window.

    Auto-summarises if a signal has more than max_edges_per_signal transitions.
    """
    session = _get_session(session_id)
    t0_ps = _parse_t(session, t0)
    t1_ps = _parse_t(session, t1)

    result = {}
    for sig in signals:
        transitions = engine.get_transitions(
            session.handle, sig, t0_ps, t1_ps, max_edges_per_signal
        )
        if transitions["truncated"]:
            summary = engine.summarize(session.handle, sig, t0_ps, t1_ps)
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

    return {
        "session_id": session_id,
        "t0_ps": t0_ps,
        "t1_ps": t1_ps,
        "signals": result,
    }


@mcp.tool()
def find_first_match(session_id: str, predicate_json: str, after: str | int = 0) -> dict:
    """Find first timestamp matching a predicate expression."""
    session = _get_session(session_id)
    data = json.loads(predicate_json)
    expr = _expr_from_json(data)
    after_ps = _parse_t(session, after)
    return {
        "session_id": session_id,
        "after_ps": after_ps,
        "match_time_ps": engine.find_first(session.handle, expr, after_ps),
    }


@mcp.tool()
def find_all_matches(session_id: str, predicate_json: str, t0: str | int, t1: str | int) -> dict:
    """Find all timestamps matching a predicate expression in a window."""
    session = _get_session(session_id)
    data = json.loads(predicate_json)
    expr = _expr_from_json(data)
    t0_ps = _parse_t(session, t0)
    t1_ps = _parse_t(session, t1)
    return {
        "session_id": session_id,
        "t0_ps": t0_ps,
        "t1_ps": t1_ps,
        "matches": engine.find_all(session.handle, expr, t0_ps, t1_ps),
    }


@mcp.tool()
def find_anomalies(
    session_id: str,
    signal: str,
    t0: str | int,
    t1: str | int,
    expected_period_ps: int | None = None,
) -> dict:
    """Detect anomalies in a signal: glitches, unexpected gaps, stuck signals."""
    session = _get_session(session_id)
    t0_ps = _parse_t(session, t0)
    t1_ps = _parse_t(session, t1)
    return {
        "session_id": session_id,
        "signal": signal,
        "t0_ps": t0_ps,
        "t1_ps": t1_ps,
        "anomalies": engine.find_anomalies(
            session.handle, signal, t0_ps, t1_ps, expected_period_ps
        ),
    }


def start_server(fst_path: str | None = None):
    """Start the MCP server, optionally pre-loading a waveform file as `default`."""
    if fst_path:
        _create_session(fst_path, session_id=DEFAULT_SESSION_ID)

    mcp.run(transport="stdio")
