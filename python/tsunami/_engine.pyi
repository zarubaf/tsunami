"""Type stubs for the Rust extension module (`tsunami._engine`).

This module is a compiled Rust extension built with PyO3. It wraps the
[wellen](https://github.com/ekiwi/wellen) crate to provide high-performance
access to FST and VCD waveform files.

All time parameters are in the waveform's native time units (see
[`waveform_info`][tsunami._engine.waveform_info] for the timescale).
Use [`tsunami.parse_time`][tsunami.time_parse.parse_time] to convert
human-readable strings like `"1284ns"` before calling these functions.
"""

from typing import Any

class WaveformHandle:
    """Opaque handle to an opened waveform file.

    Created by [`open()`][tsunami._engine.open]. Pass this handle as the
    first argument to all query functions. The underlying waveform data is
    memory-mapped and loaded lazily — only the signals you query are read
    from disk.
    """
    ...

def open(path: str) -> WaveformHandle:
    """Open an FST or VCD waveform file.

    Args:
        path: Path to the waveform file. Format is auto-detected.

    Returns:
        A handle to use with all other query functions.

    Raises:
        PanicException: If the file does not exist or cannot be parsed.

    Example:
        ```python
        import tsunami
        handle = tsunami.open("simulation.fst")
        ```
    """
    ...

def waveform_info(handle: WaveformHandle) -> dict[str, Any]:
    """Get waveform metadata.

    Args:
        handle: Waveform handle from [`open()`][tsunami._engine.open].

    Returns:
        Dict with keys:

        - `timescale_factor` (int): Timescale multiplier (e.g. `100`).
        - `timescale_unit` (str): Time unit (e.g. `"PicoSeconds"`, `"NanoSeconds"`).
        - `duration` (int): Last time point in the waveform.
        - `num_signals` (int): Total number of unique signals.
        - `num_time_points` (int): Number of time points in the time table.
        - `file_format` (str): `"Fst"` or `"Vcd"`.

    Example:
        ```python
        info = tsunami.waveform_info(handle)
        print(f"Duration: {info['duration']}, Signals: {info['num_signals']}")
        ```
    """
    ...

def list_signals(handle: WaveformHandle, pattern: str = "*") -> list[dict[str, Any]]:
    """List signals matching a glob pattern.

    This is typically the first call in any debugging session — use it to
    discover signal names before querying values.

    Args:
        handle: Waveform handle.
        pattern: Glob pattern to match against full hierarchical signal paths.
            Supports `*` (any characters) and `?` (single character).

    Returns:
        List of dicts, each with keys:

        - `path` (str): Full hierarchical signal path.
        - `width` (int): Bit width of the signal.
        - `type` (str): Variable type (e.g. `"Wire"`, `"Reg"`, `"Integer"`).
        - `direction` (str): Port direction (e.g. `"Input"`, `"Output"`, `"Internal"`).

    Example:
        ```python
        clocks = tsunami.list_signals(handle, "*clk*")
        for sig in clocks:
            print(f"[{sig['width']:>4}] {sig['path']}")
        ```
    """
    ...

def list_scopes(handle: WaveformHandle, prefix: str = "") -> list[str]:
    """List scopes (hierarchy levels) matching a prefix.

    Use this to browse the design hierarchy without listing individual signals.

    Args:
        handle: Waveform handle.
        prefix: Only return scopes whose full path starts with this string.
            Use `""` to list all scopes.

    Returns:
        List of full scope path strings.

    Example:
        ```python
        scopes = tsunami.list_scopes(handle, "tb.dut")
        # ['tb.dut.core', 'tb.dut.core.iq', 'tb.dut.cache', ...]
        ```
    """
    ...

def get_value(handle: WaveformHandle, signal: str, time_ps: int) -> dict[str, Any]:
    """Get a signal's value at a specific time.

    For querying multiple signals at the same time, prefer
    [`get_snapshot()`][tsunami._engine.get_snapshot] which is more efficient.

    Args:
        handle: Waveform handle.
        signal: Full hierarchical signal path (e.g. `"tb.dut.clk"`).
        time_ps: Time point to query.

    Returns:
        Dict with keys:

        - `hex` (str): Value as a hex string (e.g. `"1"`, `"3f"`, `"x"` for unknown).
        - `is_x` (bool): True if the value contains unknown (X) bits.
        - `is_z` (bool): True if the value contains high-impedance (Z) bits.

    Raises:
        ValueError: If the signal path is not found.

    Example:
        ```python
        val = tsunami.get_value(handle, "tb.dut.clk", 1000)
        print(f"0x{val['hex']}")  # "0x1"
        ```
    """
    ...

def get_snapshot(
    handle: WaveformHandle, signals: list[str], time_ps: int
) -> dict[str, dict[str, Any]]:
    """Get values of multiple signals at a single time point.

    More efficient than calling [`get_value()`][tsunami._engine.get_value]
    in a loop — signals are loaded in a single batch.

    Args:
        handle: Waveform handle.
        signals: List of full signal paths.
        time_ps: Time point to query.

    Returns:
        Dict mapping each signal path to its value dict (same format as
        [`get_value()`][tsunami._engine.get_value]).

    Example:
        ```python
        snap = tsunami.get_snapshot(handle, [
            "tb.dut.tl_a_valid",
            "tb.dut.tl_a_ready",
            "tb.dut.tl_a_opcode",
        ], time_ps=1_284_000)
        for sig, val in snap.items():
            print(f"{sig} = 0x{val['hex']}")
        ```
    """
    ...

def get_transitions(
    handle: WaveformHandle,
    signal: str,
    t0_ps: int,
    t1_ps: int,
    max_edges: int = 1000,
) -> dict[str, Any]:
    """Get signal transitions (value changes) in a time range.

    Args:
        handle: Waveform handle.
        signal: Full signal path.
        t0_ps: Start time (inclusive).
        t1_ps: End time (inclusive).
        max_edges: Maximum number of transitions to return. If the signal
            has more transitions in the range, the result is truncated but
            `total_transitions` still reflects the true count.

    Returns:
        Dict with keys:

        - `signal` (str): The queried signal path.
        - `t0_ps` (int): Start time.
        - `t1_ps` (int): End time.
        - `total_transitions` (int): Actual number of transitions in range.
        - `truncated` (bool): True if output was capped at `max_edges`.
        - `transitions` (list[dict]): List of `{"time": int, "value": str}` dicts.

    Example:
        ```python
        result = tsunami.get_transitions(handle, "tb.dut.clk", 0, 10_000)
        for tr in result["transitions"]:
            print(f"  t={tr['time']}: 0x{tr['value']}")
        ```
    """
    ...

def find_next_edge(
    handle: WaveformHandle,
    signal: str,
    direction: str,
    after_ps: int,
) -> int | None:
    """Find the next edge (value change) on a signal after a given time.

    Args:
        handle: Waveform handle.
        signal: Full signal path.
        direction: One of `"rising"`, `"falling"`, or `"any"`.
        after_ps: Search for edges strictly after this time.

    Returns:
        Time of the next matching edge, or `None` if no edge is found.

    Raises:
        ValueError: If `direction` is not one of the accepted values.

    Example:
        ```python
        t = tsunami.find_next_edge(handle, "tb.dut.clk", "rising", after_ps=0)
        ```
    """
    ...

def find_first(handle: WaveformHandle, expr: Any, after_ps: int) -> int | None:
    """Find the first timestamp where a predicate expression is true.

    The entire scan runs in Rust in a single pass over the union of
    transition points of all referenced signals.

    Args:
        handle: Waveform handle.
        expr: A predicate expression built with the
            [`tsunami.predicate`][tsunami.predicate] DSL.
        after_ps: Search after this time.

    Returns:
        Time of the first match, or `None` if no match is found.

    Example:
        ```python
        from tsunami.predicate import Signal
        valid = Signal("tb.dut.tl_a_valid")
        ready = Signal("tb.dut.tl_a_ready")
        t = tsunami.find_first(handle, valid & ready, after_ps=0)
        ```
    """
    ...

def find_all(
    handle: WaveformHandle, expr: Any, t0_ps: int, t1_ps: int
) -> list[int]:
    """Find all timestamps where a predicate expression is true.

    Args:
        handle: Waveform handle.
        expr: A predicate expression.
        t0_ps: Start of search window.
        t1_ps: End of search window.

    Returns:
        List of timestamps where the predicate evaluated to true.

    Example:
        ```python
        from tsunami.predicate import Signal
        handshake = Signal("tb.dut.tl_a_valid") & Signal("tb.dut.tl_a_ready")
        times = tsunami.find_all(handle, handshake, 0, 10_000_000)
        print(f"{len(times)} handshakes found")
        ```
    """
    ...

def scan(
    handle: WaveformHandle, expr: Any, t0_ps: int, t1_ps: int
) -> list[dict[str, Any]]:
    """Evaluate a predicate at every transition point in a window.

    Unlike [`find_all()`][tsunami._engine.find_all] which returns only matching
    timestamps, `scan()` returns the evaluated value at each point where the
    predicate is true. Useful for building transaction decoders.

    Args:
        handle: Waveform handle.
        expr: A predicate expression.
        t0_ps: Start of scan window.
        t1_ps: End of scan window.

    Returns:
        List of `{"time": int, "value": int}` dicts for each matching point.
    """
    ...

def summarize(
    handle: WaveformHandle, signal: str, t0_ps: int, t1_ps: int
) -> dict[str, Any]:
    """Summarize a signal over a time window.

    Computes statistics entirely in Rust before returning to Python. This is
    the mechanism that prevents large waveform windows from flooding the
    context window when used with an LLM.

    Args:
        handle: Waveform handle.
        signal: Full signal path.
        t0_ps: Start time.
        t1_ps: End time.

    Returns:
        Dict with keys:

        - `total_transitions` (int): Number of value changes in the window.
        - `dominant_period_ps` (int | None): Most common interval between transitions.
        - `duty_cycle` (float | None): Fraction of time the signal is high (1-bit signals only).
        - `value_histogram` (dict[str, int]): Counts of each distinct value.
        - `anomalies` (list[dict]): Detected anomalies (see
            [`find_anomalies()`][tsunami._engine.find_anomalies]).

    Example:
        ```python
        summary = tsunami.summarize(handle, "tb.dut.clk", 0, 10_000_000)
        print(f"Period: {summary['dominant_period_ps']}ps")
        print(f"Duty cycle: {summary['duty_cycle']:.1%}")
        ```
    """
    ...

def summarize_window(
    handle: WaveformHandle, signals: list[str], t0_ps: int, t1_ps: int
) -> dict[str, dict[str, Any]]:
    """Summarize multiple signals over a time window.

    Calls [`summarize()`][tsunami._engine.summarize] for each signal.

    Args:
        handle: Waveform handle.
        signals: List of signal paths.
        t0_ps: Start time.
        t1_ps: End time.

    Returns:
        Dict mapping each signal path to its summary dict.
    """
    ...

def find_anomalies(
    handle: WaveformHandle,
    signal: str,
    t0_ps: int,
    t1_ps: int,
    expected_period_ps: int | None = None,
) -> list[dict[str, Any]]:
    """Detect anomalies in a signal's transition pattern.

    Looks for three kinds of anomaly:

    - **glitch**: An interval shorter than 25% of the dominant period.
    - **gap**: An interval longer than 200% of the dominant period.
    - **stuck**: No transitions for a long time at the end of the window.

    Args:
        handle: Waveform handle.
        signal: Full signal path.
        t0_ps: Start time.
        t1_ps: End time.
        expected_period_ps: Expected period between transitions. If `None`,
            the dominant period is auto-inferred from the data.

    Returns:
        List of anomaly dicts, each with keys:

        - `time_ps` (int): Time where the anomaly was detected.
        - `kind` (str): One of `"glitch"`, `"gap"`, or `"stuck"`.
        - `detail` (str): Human-readable description.

    Example:
        ```python
        anomalies = tsunami.find_anomalies(handle, "tb.dut.clk", 0, 10_000_000)
        for a in anomalies:
            print(f"  t={a['time_ps']}: {a['kind']} — {a['detail']}")
        ```
    """
    ...
