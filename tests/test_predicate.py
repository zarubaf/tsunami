"""Unit tests for the predicate DSL."""

from tsunami.predicate import (
    Signal, Const, And, Or, Not, Xor, Eq, Gt, Lt,
    Rise, Fall, BitSlice, Sequence, PrecededBy,
    scope, signals,
)


class TestSignal:
    def test_create(self):
        s = Signal("tb.dut.clk")
        assert s.tag == "signal"
        assert s.path == "tb.dut.clk"

    def test_bitslice(self):
        s = Signal("tb.dut.data")
        sliced = s[7:0]
        assert isinstance(sliced, BitSlice)
        assert sliced.high == 7
        assert sliced.low == 0

    def test_single_bit(self):
        s = Signal("tb.dut.data")
        bit = s[3]
        assert isinstance(bit, BitSlice)
        assert bit.high == 3
        assert bit.low == 3


class TestOperators:
    def test_and(self):
        a = Signal("a")
        b = Signal("b")
        result = a & b
        assert isinstance(result, And)
        assert result.left is a
        assert result.right is b

    def test_or(self):
        a = Signal("a")
        b = Signal("b")
        result = a | b
        assert isinstance(result, Or)

    def test_not(self):
        a = Signal("a")
        result = ~a
        assert isinstance(result, Not)
        assert result.inner is a

    def test_xor(self):
        a = Signal("a")
        b = Signal("b")
        result = a ^ b
        assert isinstance(result, Xor)

    def test_eq_int(self):
        a = Signal("a")
        result = a == 4
        assert isinstance(result, Eq)
        assert isinstance(result.right, Const)
        assert result.right.value == 4

    def test_gt(self):
        a = Signal("a")
        result = a > 10
        assert isinstance(result, Gt)

    def test_lt(self):
        a = Signal("a")
        result = a < 5
        assert isinstance(result, Lt)

    def test_rise(self):
        a = Signal("a")
        result = a.rise()
        assert isinstance(result, Rise)

    def test_fall(self):
        a = Signal("a")
        result = a.fall()
        assert isinstance(result, Fall)

    def test_sequence(self):
        a = Signal("a")
        b = Signal("b")
        result = a >> b
        assert isinstance(result, Sequence)
        assert result.within_ps is None

    def test_sequence_with_window(self):
        a = Signal("a")
        b = Signal("b")
        result = a >> (b, 50000)
        assert isinstance(result, Sequence)
        assert result.within_ps == 50000

    def test_preceded_by(self):
        a = Signal("a")
        b = Signal("b")
        result = a.preceded_by(b, within_ps=10000)
        assert isinstance(result, PrecededBy)
        assert result.within_ps == 10000


class TestComposition:
    def test_complex_expression(self):
        valid = Signal("tb.dut.tl_a_valid")
        ready = Signal("tb.dut.tl_a_ready")
        opcode = Signal("tb.dut.tl_a_opcode")

        handshake = valid & ready & (opcode == 4)
        assert isinstance(handshake, And)

    def test_negated_preceded_by(self):
        a = Signal("a")
        b = Signal("b")
        result = ~a.preceded_by(b, within_ps=50000)
        assert isinstance(result, Not)


class TestScope:
    def test_scope_proxy(self):
        with scope("tb.dut") as s:
            sig = s.clk
            assert isinstance(sig, Signal)
            assert sig.path == "tb.dut.clk"

    def test_scope_nested(self):
        with scope("tb.dut.core") as s:
            sig = s.tl_a_valid
            assert sig.path == "tb.dut.core.tl_a_valid"


class TestSignals:
    def test_signals_proxy(self):
        with signals(v="tb.dut.valid", r="tb.dut.ready") as s:
            assert isinstance(s.v, Signal)
            assert s.v.path == "tb.dut.valid"
            assert s.r.path == "tb.dut.ready"

    def test_signals_unknown_alias(self):
        import pytest
        with signals(v="tb.dut.valid") as s:
            with pytest.raises(AttributeError):
                _ = s.unknown
