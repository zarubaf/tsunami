"""Integration tests for the Rust engine against an arbiter waveform."""

import os
import pytest
import tsunami._engine as engine

FST_PATH = os.path.join(os.path.dirname(__file__), "fixtures", "arbiter.fst")

# Key signals in the arbiter waveform
REQUEST = "$rootio.request_i"
GRANT_0 = "$rootio.grants_o.[0]"
PREFIX_SUM_0 = "cc_arb_priority.gen_prefix_sum.prefix_sum.[0]"


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
        assert info["num_signals"] == 347
        assert info["duration"] == 999000
        assert info["file_format"] == "Fst"
        assert info["timescale_unit"] == "PicoSeconds"


class TestListSignals:
    def test_list_all(self, handle):
        signals = engine.list_signals(handle, "*")
        assert len(signals) == 350

    def test_list_glob_request(self, handle):
        signals = engine.list_signals(handle, "*request*")
        assert len(signals) > 0
        for sig in signals:
            assert "request" in sig["path"].lower()

    def test_signal_has_expected_fields(self, handle):
        signals = engine.list_signals(handle, REQUEST)
        assert len(signals) >= 1
        sig = signals[0]
        assert sig["path"] == REQUEST
        assert sig["width"] == 20

    def test_no_match(self, handle):
        signals = engine.list_signals(handle, "nonexistent_signal_xyz")
        assert len(signals) == 0


class TestListScopes:
    def test_list_all(self, handle):
        scopes = engine.list_scopes(handle, "")
        assert len(scopes) > 0

    def test_list_prefix(self, handle):
        scopes = engine.list_scopes(handle, "cc_arb_priority")
        assert len(scopes) > 0
        for s in scopes:
            assert s.startswith("cc_arb_priority")


class TestGetValue:
    def test_get_request_value(self, handle):
        val = engine.get_value(handle, REQUEST, 0)
        assert "hex" in val
        assert "is_x" in val
        assert "is_z" in val
        assert not val["is_x"]
        assert not val["is_z"]

    def test_get_grant_value(self, handle):
        val = engine.get_value(handle, GRANT_0, 500000)
        assert val["hex"] == "10000"

    def test_value_not_found(self, handle):
        with pytest.raises(ValueError, match="Signal not found"):
            engine.get_value(handle, "nonexistent.signal", 0)


class TestGetSnapshot:
    def test_snapshot_multiple_signals(self, handle):
        result = engine.get_snapshot(handle, [REQUEST, GRANT_0], 500000)
        assert REQUEST in result
        assert GRANT_0 in result
        assert result[REQUEST]["hex"] == "253004"
        assert result[GRANT_0]["hex"] == "10000"


class TestGetTransitions:
    def test_request_transitions(self, handle):
        result = engine.get_transitions(handle, REQUEST, 0, 100000, 1000)
        assert result["total_transitions"] == 101
        assert not result["truncated"]
        assert len(result["transitions"]) == 101

    def test_truncation(self, handle):
        result = engine.get_transitions(handle, REQUEST, 0, 100000, 10)
        assert result["truncated"]
        assert len(result["transitions"]) == 10
        assert result["total_transitions"] == 101

    def test_first_transition_value(self, handle):
        result = engine.get_transitions(handle, REQUEST, 0, 5000, 100)
        assert result["transitions"][0]["time"] == 0
        assert result["transitions"][0]["value"] == "b11f07"


class TestFindNextEdge:
    def test_rising_edge(self, handle):
        t = engine.find_next_edge(handle, REQUEST, "rising", 0)
        assert t is not None
        assert t == 2000

    def test_any_edge(self, handle):
        t = engine.find_next_edge(handle, REQUEST, "any", 0)
        assert t is not None
        assert t == 1000

    def test_grant_edge(self, handle):
        t = engine.find_next_edge(handle, GRANT_0, "any", 0)
        assert t is not None
        assert t > 0


class TestSummarize:
    def test_request_summary(self, handle):
        result = engine.summarize(handle, REQUEST, 0, 100000)
        assert result["total_transitions"] == 101
        assert result["dominant_period_ps"] == 1000
        # Multi-valued signal, no duty cycle
        assert result["duty_cycle"] is None

    def test_full_range_summary(self, handle):
        result = engine.summarize(handle, REQUEST, 0, 999000)
        assert result["total_transitions"] == 1000
        assert result["dominant_period_ps"] is not None


class TestFindAnomalies:
    def test_regular_signal_no_anomalies(self, handle):
        # request_i changes every 1000ps — very regular, no anomalies
        anomalies = engine.find_anomalies(handle, REQUEST, 0, 100000)
        assert isinstance(anomalies, list)
        assert len(anomalies) == 0
