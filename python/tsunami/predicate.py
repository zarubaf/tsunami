"""Predicate DSL — composable expressions for waveform queries.

This module provides a Python DSL for building signal predicates that are
evaluated entirely in Rust. Expressions are pure data structures (an AST) —
no waveform access happens in Python. The entire expression tree is handed
to the Rust evaluator in a single PyO3 call.

## Quick start

```python
from tsunami.predicate import Signal, scope

# Direct signal references
clk = Signal("tb.dut.clk")
valid = Signal("tb.dut.tl_a_valid")

# Using a scope prefix
with scope("tb.dut") as s:
    handshake = s.tl_a_valid & s.tl_a_ready
    acquire = handshake & (s.tl_a_opcode == 4)

# Compose with operators
rising_valid = valid.rise()
sequence = acquire >> (s.tl_d_valid, 50_000)  # with time window
violation = s.tl_d_valid.rise().preceded_by(acquire, within_ps=50_000).__invert__()
```

## Supported operators

| Syntax | Meaning |
|---|---|
| `a & b` | Logical AND |
| `a \\| b` | Logical OR |
| `~a` | Logical NOT |
| `a ^ b` | Logical XOR |
| `sig == val` | Signal equals constant |
| `sig > val` / `sig < val` | Unsigned comparison |
| `sig.rise()` | Rising edge (0 -> non-zero) |
| `sig.fall()` | Falling edge (non-zero -> 0) |
| `a >> b` | Sequence: a followed by b |
| `a >> (b, window_ps)` | Sequence with time window |
| `a.preceded_by(b, within_ps=N)` | b occurred before a within window |
| `sig[7:0]` | Bitfield extraction |
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Optional


@dataclass(frozen=True, eq=False)
class Expr:
    """Base class for all predicate expression nodes.

    All expression nodes carry a `tag` string that the Rust evaluator uses
    to identify the node type via `FromPyObject`. You normally don't construct
    `Expr` directly — use [`Signal`][tsunami.predicate.Signal] and operators instead.
    """

    tag: str

    def __and__(self, other: Expr) -> Expr:
        """Logical AND: `a & b` is true when both operands are non-zero."""
        return And(left=self, right=_coerce(other))

    def __rand__(self, other: Expr) -> Expr:
        return And(left=_coerce(other), right=self)

    def __or__(self, other: Expr) -> Expr:
        """Logical OR: `a | b` is true when either operand is non-zero."""
        return Or(left=self, right=_coerce(other))

    def __ror__(self, other: Expr) -> Expr:
        return Or(left=_coerce(other), right=self)

    def __invert__(self) -> Expr:
        """Logical NOT: `~a` is true when the operand is zero."""
        return Not(inner=self)

    def __xor__(self, other: Expr) -> Expr:
        """Logical XOR: `a ^ b` is true when exactly one operand is non-zero."""
        return Xor(left=self, right=_coerce(other))

    def __rxor__(self, other: Expr) -> Expr:
        return Xor(left=_coerce(other), right=self)

    def __eq__(self, other: object) -> Expr:  # type: ignore[override]
        """Equality: `sig == 4` is true when the signal's value equals 4."""
        return Eq(left=self, right=_coerce(other))

    def __gt__(self, other: object) -> Expr:  # type: ignore[override]
        """Unsigned greater-than comparison."""
        return Gt(left=self, right=_coerce(other))

    def __lt__(self, other: object) -> Expr:  # type: ignore[override]
        """Unsigned less-than comparison."""
        return Lt(left=self, right=_coerce(other))

    def __rshift__(self, other) -> Expr:
        """Sequence operator.

        - `a >> b` — a followed by b (unbounded).
        - `a >> (b, window_ps)` — a followed by b within `window_ps` picoseconds.
        """
        if isinstance(other, tuple) and len(other) == 2:
            expr, window = other
            return Sequence(a=self, b=_coerce(expr), within_ps=int(window))
        return Sequence(a=self, b=_coerce(other), within_ps=None)

    def rise(self) -> Expr:
        """Rising edge: true at the transition from zero to non-zero."""
        return Rise(inner=self)

    def fall(self) -> Expr:
        """Falling edge: true at the transition from non-zero to zero."""
        return Fall(inner=self)

    def preceded_by(self, other: Expr, within_ps: int | None = None) -> Expr:
        """True when `self` is true AND `other` was true within `within_ps` before.

        Args:
            other: The preceding condition.
            within_ps: Maximum lookback window in picoseconds. If `None`, searches
                the entire history.
        """
        return PrecededBy(a=self, b=_coerce(other), within_ps=within_ps)


def _coerce(value: object) -> Expr:
    """Coerce a value to an Expr (int -> Const)."""
    if isinstance(value, Expr):
        return value
    if isinstance(value, int):
        return Const(value=value)
    raise TypeError(f"Cannot coerce {type(value).__name__} to Expr")


@dataclass(frozen=True, eq=False)
class Signal(Expr):
    """Reference to a waveform signal by its full hierarchical path.

    This is the primary leaf node in predicate expressions. The path must
    match a signal in the waveform file exactly (dot-separated hierarchy).

    Args:
        path: Full hierarchical path, e.g. `"tb.dut.tl_a_valid"`.

    Example:
        ```python
        clk = Signal("tb.dut.clk")
        opcode = Signal("tb.dut.tl_a_opcode")

        # Use in expressions
        rising_clk = clk.rise()
        is_get = opcode == 4

        # Bitfield extraction
        low_nibble = opcode[3:0]
        single_bit = opcode[2]
        ```
    """

    tag: str = "signal"
    path: str = ""

    def __init__(self, path: str):
        object.__setattr__(self, "tag", "signal")
        object.__setattr__(self, "path", path)

    def __getitem__(self, key) -> Expr:
        """Extract a bitfield: `sig[7:0]` for a range or `sig[3]` for a single bit."""
        if isinstance(key, slice):
            high = key.start if key.start is not None else 0
            low = key.stop if key.stop is not None else 0
            return BitSlice(inner=self, high=high, low=low)
        return BitSlice(inner=self, high=key, low=key)


@dataclass(frozen=True, eq=False)
class Const(Expr):
    """Constant integer value."""

    tag: str = "const"
    value: int = 0

    def __init__(self, value: int):
        object.__setattr__(self, "tag", "const")
        object.__setattr__(self, "value", value)


@dataclass(frozen=True, eq=False)
class And(Expr):
    tag: str = "and"
    left: Expr = None  # type: ignore
    right: Expr = None  # type: ignore


@dataclass(frozen=True, eq=False)
class Or(Expr):
    tag: str = "or"
    left: Expr = None  # type: ignore
    right: Expr = None  # type: ignore


@dataclass(frozen=True, eq=False)
class Not(Expr):
    tag: str = "not"
    inner: Expr = None  # type: ignore


@dataclass(frozen=True, eq=False)
class Xor(Expr):
    tag: str = "xor"
    left: Expr = None  # type: ignore
    right: Expr = None  # type: ignore


@dataclass(frozen=True, eq=False)
class Eq(Expr):
    tag: str = "eq"
    left: Expr = None  # type: ignore
    right: Expr = None  # type: ignore


@dataclass(frozen=True, eq=False)
class Gt(Expr):
    tag: str = "gt"
    left: Expr = None  # type: ignore
    right: Expr = None  # type: ignore


@dataclass(frozen=True, eq=False)
class Lt(Expr):
    tag: str = "lt"
    left: Expr = None  # type: ignore
    right: Expr = None  # type: ignore


@dataclass(frozen=True, eq=False)
class Rise(Expr):
    tag: str = "rise"
    inner: Expr = None  # type: ignore


@dataclass(frozen=True, eq=False)
class Fall(Expr):
    tag: str = "fall"
    inner: Expr = None  # type: ignore


@dataclass(frozen=True, eq=False)
class BitSlice(Expr):
    tag: str = "bit_slice"
    inner: Expr = None  # type: ignore
    high: int = 0
    low: int = 0


@dataclass(frozen=True, eq=False)
class Sequence(Expr):
    tag: str = "sequence"
    a: Expr = None  # type: ignore
    b: Expr = None  # type: ignore
    within_ps: Optional[int] = None


@dataclass(frozen=True, eq=False)
class PrecededBy(Expr):
    tag: str = "preceded_by"
    a: Expr = None  # type: ignore
    b: Expr = None  # type: ignore
    within_ps: Optional[int] = None


class ScopeProxy:
    """Provides attribute-based signal access with a hierarchy prefix."""

    def __init__(self, prefix: str):
        self._prefix = prefix

    def __getattr__(self, name: str) -> Signal:
        if name.startswith("_"):
            raise AttributeError(name)
        return Signal(f"{self._prefix}.{name}")


class SignalsProxy:
    """Provides aliased signal access."""

    def __init__(self, mapping: dict[str, str]):
        self._mapping = mapping

    def __getattr__(self, name: str) -> Signal:
        if name.startswith("_"):
            raise AttributeError(name)
        if name not in self._mapping:
            raise AttributeError(f"No signal bound to alias '{name}'")
        return Signal(self._mapping[name])


class scope:
    """Context manager that sets a hierarchy prefix for signal access.

    Attribute access on the yielded proxy creates
    [`Signal`][tsunami.predicate.Signal] objects with the prefix prepended.

    Args:
        prefix: Hierarchy prefix (e.g. `"tb.dut.core"`).

    Example:
        ```python
        with scope("tb.dut") as s:
            handshake = s.tl_a_valid & s.tl_a_ready
            # equivalent to:
            # Signal("tb.dut.tl_a_valid") & Signal("tb.dut.tl_a_ready")
        ```
    """

    def __init__(self, prefix: str):
        self._prefix = prefix

    def __enter__(self) -> ScopeProxy:
        return ScopeProxy(self._prefix)

    def __exit__(self, *args):
        pass


class signals:
    """Context manager for aliased signal bindings.

    Maps short alias names to full signal paths. Useful when working with
    signals from a configuration dict.

    Args:
        **kwargs: Mapping of alias name to full signal path.

    Example:
        ```python
        with signals(v="tb.dut.tl_a_valid", r="tb.dut.tl_a_ready") as s:
            handshake = s.v & s.r
        ```
    """

    def __init__(self, **kwargs: str):
        self._mapping = kwargs

    def __enter__(self) -> SignalsProxy:
        return SignalsProxy(self._mapping)

    def __exit__(self, *args):
        pass
