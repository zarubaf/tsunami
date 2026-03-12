"""Time string parser — converts human-readable time strings to picoseconds.

Tsunami accepts time values in multiple formats across the CLI, MCP server,
and Python API. This module provides the [`parse_time`][tsunami.time_parse.parse_time]
function that normalises all of them to integer picoseconds.

Supported formats:

| Input | Result |
|---|---|
| `100` (int/float) | `100` (raw passthrough) |
| `"100ps"` | `100` |
| `"1284ns"` | `1_284_000` |
| `"1.284us"` or `"1.284µs"` | `1_284_000` |
| `"1ms"` | `1_000_000_000` |
| `"1s"` | `1_000_000_000_000` |
| `"642cyc"` | `642 * timescale_ps` (requires `timescale_ps`) |
"""

from __future__ import annotations

import re

_TIME_RE = re.compile(
    r"^\s*([0-9]*\.?[0-9]+)\s*(ps|ns|us|µs|ms|s|cyc)?\s*$", re.IGNORECASE
)

_MULTIPLIERS: dict[str, float] = {
    "ps": 1.0,
    "ns": 1_000.0,
    "us": 1_000_000.0,
    "µs": 1_000_000.0,
    "ms": 1_000_000_000.0,
    "s": 1_000_000_000_000.0,
}


def parse_time(value: int | float | str, timescale_ps: int | None = None) -> int:
    """Parse a time value into picoseconds.

    Args:
        value: Time as an integer (raw picoseconds), float, or string with
            a unit suffix (`"1284ns"`, `"1.284us"`, `"642cyc"`, etc.).
        timescale_ps: Clock period in picoseconds, required when using the
            `"cyc"` unit. Obtain from
            [`waveform_info()`][tsunami._engine.waveform_info].

    Returns:
        Time in picoseconds as an integer.

    Raises:
        ValueError: If the string cannot be parsed or `"cyc"` is used without
            `timescale_ps`.

    Example:
        ```python
        from tsunami.time_parse import parse_time

        parse_time(1234)        # 1234
        parse_time("1284ns")    # 1_284_000
        parse_time("1.284us")   # 1_284_000
        parse_time("642cyc", timescale_ps=1000)  # 642_000
        ```
    """
    if isinstance(value, (int, float)):
        return int(value)

    m = _TIME_RE.match(value)
    if m is None:
        raise ValueError(f"Cannot parse time: {value!r}")

    number = float(m.group(1))
    unit = m.group(2)

    if unit is None:
        return int(number)

    unit_lower = unit.lower() if unit != "µs" else "µs"

    if unit_lower == "cyc":
        if timescale_ps is None:
            raise ValueError("Cannot convert cycles without timescale_ps")
        return int(number * timescale_ps)

    multiplier = _MULTIPLIERS.get(unit_lower)
    if multiplier is None:
        raise ValueError(f"Unknown time unit: {unit!r}")

    return int(number * multiplier)
