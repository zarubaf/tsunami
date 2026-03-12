# Engine API Reference

The engine module (`tsunami._engine`) is a compiled Rust extension. All functions
are re-exported from `tsunami` directly.

::: tsunami._engine
    options:
      members:
        - WaveformHandle
        - open
        - waveform_info
        - list_signals
        - list_scopes
        - get_value
        - get_snapshot
        - get_transitions
        - find_next_edge
        - find_first
        - find_all
        - scan
        - summarize
        - summarize_window
        - find_anomalies
