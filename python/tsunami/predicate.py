"""Predicate DSL — composable expressions for waveform queries.

Builds a pure data structure (AST) that is handed to the Rust evaluator
in a single PyO3 call. No execution happens in Python.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Optional


@dataclass(frozen=True, eq=False)
class Expr:
    """Base expression node. All nodes have a 'tag' used by Rust's FromPyObject."""

    tag: str

    def __and__(self, other: Expr) -> Expr:
        return And(left=self, right=_coerce(other))

    def __rand__(self, other: Expr) -> Expr:
        return And(left=_coerce(other), right=self)

    def __or__(self, other: Expr) -> Expr:
        return Or(left=self, right=_coerce(other))

    def __ror__(self, other: Expr) -> Expr:
        return Or(left=_coerce(other), right=self)

    def __invert__(self) -> Expr:
        return Not(inner=self)

    def __xor__(self, other: Expr) -> Expr:
        return Xor(left=self, right=_coerce(other))

    def __rxor__(self, other: Expr) -> Expr:
        return Xor(left=_coerce(other), right=self)

    def __eq__(self, other: object) -> Expr:  # type: ignore[override]
        return Eq(left=self, right=_coerce(other))

    def __gt__(self, other: object) -> Expr:  # type: ignore[override]
        return Gt(left=self, right=_coerce(other))

    def __lt__(self, other: object) -> Expr:  # type: ignore[override]
        return Lt(left=self, right=_coerce(other))

    def __rshift__(self, other) -> Expr:
        """Sequence operator: self >> other or self >> (other, window_ps)."""
        if isinstance(other, tuple) and len(other) == 2:
            expr, window = other
            return Sequence(a=self, b=_coerce(expr), within_ps=int(window))
        return Sequence(a=self, b=_coerce(other), within_ps=None)

    def rise(self) -> Expr:
        return Rise(inner=self)

    def fall(self) -> Expr:
        return Fall(inner=self)

    def preceded_by(self, other: Expr, within_ps: int | None = None) -> Expr:
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
    """Reference to a waveform signal by hierarchical path."""

    tag: str = "signal"
    path: str = ""

    def __init__(self, path: str):
        object.__setattr__(self, "tag", "signal")
        object.__setattr__(self, "path", path)

    def __getitem__(self, key) -> Expr:
        """Bitfield extraction: sig[7:0] or sig[3]."""
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
    """Context manager for hierarchy prefix."""

    def __init__(self, prefix: str):
        self._prefix = prefix

    def __enter__(self) -> ScopeProxy:
        return ScopeProxy(self._prefix)

    def __exit__(self, *args):
        pass


class signals:
    """Context manager for aliased signal bindings."""

    def __init__(self, **kwargs: str):
        self._mapping = kwargs

    def __enter__(self) -> SignalsProxy:
        return SignalsProxy(self._mapping)

    def __exit__(self, *args):
        pass
