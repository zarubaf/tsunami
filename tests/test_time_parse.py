"""Unit tests for the time parser."""

import pytest
from tsunami.time_parse import parse_time


class TestParseTime:
    def test_raw_int(self):
        assert parse_time(1234) == 1234

    def test_raw_float(self):
        assert parse_time(1234.5) == 1234

    def test_picoseconds(self):
        assert parse_time("100ps") == 100

    def test_nanoseconds(self):
        assert parse_time("1284ns") == 1_284_000

    def test_microseconds(self):
        assert parse_time("1.284us") == 1_284_000

    def test_microseconds_unicode(self):
        assert parse_time("1.284µs") == 1_284_000

    def test_milliseconds(self):
        assert parse_time("1ms") == 1_000_000_000

    def test_seconds(self):
        assert parse_time("1s") == 1_000_000_000_000

    def test_cycles(self):
        assert parse_time("642cyc", timescale_ps=1000) == 642_000

    def test_cycles_without_timescale(self):
        with pytest.raises(ValueError, match="timescale_ps"):
            parse_time("642cyc")

    def test_bare_number_string(self):
        assert parse_time("1234") == 1234

    def test_invalid(self):
        with pytest.raises(ValueError, match="Cannot parse"):
            parse_time("abc")

    def test_whitespace(self):
        assert parse_time("  100 ns  ") == 100_000

    def test_fractional_ns(self):
        assert parse_time("1.5ns") == 1500
