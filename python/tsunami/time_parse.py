"""Time string parser — converts human-readable time to picoseconds."""

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

    Accepts:
        - int/float: treated as raw picoseconds
        - "1284ns", "1.284us", "100ps", etc.
        - "642cyc": requires timescale_ps
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
