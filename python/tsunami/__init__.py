"""Tsunami — AI-assisted hardware waveform debugging tool."""

from tsunami._engine import (
    open,
    WaveformHandle,
    waveform_info,
    list_signals,
    list_scopes,
    get_value,
    get_snapshot,
    get_transitions,
    find_next_edge,
    find_first,
    find_all,
    scan,
    summarize,
    summarize_window,
    find_anomalies,
)
from tsunami.time_parse import parse_time
from tsunami.predicate import Signal, Expr

__all__ = [
    "open",
    "WaveformHandle",
    "waveform_info",
    "list_signals",
    "list_scopes",
    "get_value",
    "get_snapshot",
    "get_transitions",
    "find_next_edge",
    "find_first",
    "find_all",
    "scan",
    "summarize",
    "summarize_window",
    "find_anomalies",
    "parse_time",
    "Signal",
    "Expr",
]
