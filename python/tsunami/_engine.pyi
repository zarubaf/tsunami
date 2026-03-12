"""Type stubs for the Rust extension module."""

from typing import Any

class WaveformHandle:
    """Opaque handle to an opened waveform file."""
    ...

def open(path: str) -> WaveformHandle:
    """Open an FST/VCD waveform file."""
    ...

def waveform_info(handle: WaveformHandle) -> dict[str, Any]:
    """Get waveform metadata: timescale, duration, signal count, format."""
    ...

def list_signals(handle: WaveformHandle, pattern: str = "*") -> list[dict[str, Any]]:
    """List signals matching a glob pattern.

    Returns list of dicts with keys: path, width, type, direction.
    """
    ...

def list_scopes(handle: WaveformHandle, prefix: str = "") -> list[str]:
    """List scopes matching a prefix."""
    ...

def get_value(handle: WaveformHandle, signal: str, time_ps: int) -> dict[str, Any]:
    """Get signal value at a time. Returns dict with keys: hex, is_x, is_z."""
    ...

def get_snapshot(
    handle: WaveformHandle, signals: list[str], time_ps: int
) -> dict[str, dict[str, Any]]:
    """Get values of multiple signals at a single time point."""
    ...

def get_transitions(
    handle: WaveformHandle,
    signal: str,
    t0_ps: int,
    t1_ps: int,
    max_edges: int = 1000,
) -> dict[str, Any]:
    """Get signal transitions in a time range.

    Returns dict with keys: signal, t0_ps, t1_ps, total_transitions,
    truncated, transitions.
    """
    ...

def find_next_edge(
    handle: WaveformHandle,
    signal: str,
    direction: str,
    after_ps: int,
) -> int | None:
    """Find next edge. direction: 'rising', 'falling', or 'any'."""
    ...

def find_first(handle: WaveformHandle, expr: Any, after_ps: int) -> int | None:
    """Find first timestamp matching a predicate expression."""
    ...

def find_all(
    handle: WaveformHandle, expr: Any, t0_ps: int, t1_ps: int
) -> list[int]:
    """Find all timestamps matching a predicate expression in a window."""
    ...

def scan(
    handle: WaveformHandle, expr: Any, t0_ps: int, t1_ps: int
) -> list[dict[str, Any]]:
    """Evaluate predicate at every transition point in a window."""
    ...

def summarize(
    handle: WaveformHandle, signal: str, t0_ps: int, t1_ps: int
) -> dict[str, Any]:
    """Summarize a signal in a time window.

    Returns dict with keys: total_transitions, dominant_period_ps,
    duty_cycle, value_histogram, anomalies.
    """
    ...

def summarize_window(
    handle: WaveformHandle, signals: list[str], t0_ps: int, t1_ps: int
) -> dict[str, dict[str, Any]]:
    """Summarize multiple signals in a time window."""
    ...

def find_anomalies(
    handle: WaveformHandle,
    signal: str,
    t0_ps: int,
    t1_ps: int,
    expected_period_ps: int | None = None,
) -> list[dict[str, Any]]:
    """Detect anomalies: glitches, unexpected gaps, stuck signals."""
    ...
