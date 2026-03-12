"""Integration tests for the Rust engine."""

import os
import pytest
import tsunami._engine as engine

FST_PATH = os.path.join(os.path.dirname(__file__), "..", "backend.fst")


@pytest.fixture
def handle():
    return engine.open(FST_PATH)


class TestOpen:
    def test_open_valid_file(self, handle):
        assert handle is not None

    def test_open_invalid_file(self):
        with pytest.raises(BaseException):
            engine.open("nonexistent.fst")


class TestWaveformInfo:
    def test_info_has_expected_keys(self, handle):
        info = engine.waveform_info(handle)
        assert "timescale_factor" in info
        assert "timescale_unit" in info
        assert "duration" in info
        assert "num_signals" in info
        assert "file_format" in info

    def test_info_values(self, handle):
        info = engine.waveform_info(handle)
        assert info["num_signals"] > 0
        assert info["duration"] > 0
        assert info["file_format"] == "Fst"


class TestListSignals:
    def test_list_all(self, handle):
        signals = engine.list_signals(handle, "*")
        assert len(signals) > 0

    def test_list_glob(self, handle):
        signals = engine.list_signals(handle, "*clk*")
        assert len(signals) > 0
        for sig in signals:
            assert "clk" in sig["path"].lower()

    def test_signal_has_expected_fields(self, handle):
        signals = engine.list_signals(handle, "backend_tb.clk")
        assert len(signals) == 1
        sig = signals[0]
        assert sig["path"] == "backend_tb.clk"
        assert sig["width"] == 1

    def test_no_match(self, handle):
        signals = engine.list_signals(handle, "nonexistent_signal_xyz")
        assert len(signals) == 0


class TestListScopes:
    def test_list_all(self, handle):
        scopes = engine.list_scopes(handle, "")
        assert len(scopes) > 0

    def test_list_prefix(self, handle):
        scopes = engine.list_scopes(handle, "backend_tb")
        assert len(scopes) > 0
        for s in scopes:
            assert s.startswith("backend_tb")


class TestGetValue:
    def test_get_clock_value(self, handle):
        val = engine.get_value(handle, "backend_tb.clk", 100)
        assert "hex" in val
        assert "is_x" in val
        assert "is_z" in val
        assert val["hex"] in ("0", "1")

    def test_value_not_found(self, handle):
        with pytest.raises(ValueError, match="Signal not found"):
            engine.get_value(handle, "nonexistent.signal", 0)


class TestGetSnapshot:
    def test_snapshot_multiple_signals(self, handle):
        result = engine.get_snapshot(
            handle,
            ["backend_tb.clk", "backend_tb.dcache_tl_a_valid"],
            100,
        )
        assert "backend_tb.clk" in result
        assert "backend_tb.dcache_tl_a_valid" in result
        assert "hex" in result["backend_tb.clk"]


class TestGetTransitions:
    def test_clock_transitions(self, handle):
        result = engine.get_transitions(handle, "backend_tb.clk", 0, 645, 1000)
        assert result["total_transitions"] == 130
        assert not result["truncated"]
        assert len(result["transitions"]) == 130

    def test_truncation(self, handle):
        result = engine.get_transitions(handle, "backend_tb.clk", 0, 645, 10)
        assert result["truncated"]
        assert len(result["transitions"]) == 10
        assert result["total_transitions"] == 130


class TestFindNextEdge:
    def test_rising_edge(self, handle):
        t = engine.find_next_edge(handle, "backend_tb.clk", "rising", 0)
        assert t is not None
        assert t > 0

    def test_falling_edge(self, handle):
        t = engine.find_next_edge(handle, "backend_tb.clk", "falling", 0)
        assert t is not None
        assert t > 0

    def test_any_edge(self, handle):
        t = engine.find_next_edge(handle, "backend_tb.clk", "any", 0)
        assert t is not None
        assert t > 0


class TestSummarize:
    def test_clock_summary(self, handle):
        result = engine.summarize(handle, "backend_tb.clk", 0, 645)
        assert result["total_transitions"] == 130
        assert result["dominant_period_ps"] is not None
        assert result["duty_cycle"] is not None
        assert 0.0 <= result["duty_cycle"] <= 1.0


class TestFindAnomalies:
    def test_clock_anomalies(self, handle):
        anomalies = engine.find_anomalies(handle, "backend_tb.clk", 0, 645)
        # Regular clock should have no anomalies
        assert isinstance(anomalies, list)
