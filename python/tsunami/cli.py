"""CLI entry point for tsunami."""

from __future__ import annotations

import argparse
import json
import sys

import tsunami._engine as engine
from tsunami.time_parse import parse_time


def _get_handle(args):
    return engine.open(args.file)


def _get_timescale_ps(handle):
    info = engine.waveform_info(handle)
    factor = info.get("timescale_factor", 1)
    unit = info.get("timescale_unit", "ps")
    unit_ps = {
        "FemtoSeconds": 0.001,
        "PicoSeconds": 1,
        "NanoSeconds": 1_000,
        "MicroSeconds": 1_000_000,
        "MilliSeconds": 1_000_000_000,
        "Seconds": 1_000_000_000_000,
    }.get(unit, 1)
    return int(factor * unit_ps)


def cmd_info(args):
    handle = _get_handle(args)
    info = engine.waveform_info(handle)
    for k, v in info.items():
        print(f"  {k}: {v}")


def cmd_signals(args):
    handle = _get_handle(args)
    results = engine.list_signals(handle, args.pattern)
    for sig in results:
        width = sig["width"]
        path = sig["path"]
        print(f"  [{width:>4}] {path}")
    print(f"\n  {len(results)} signal(s) matched")


def cmd_scopes(args):
    handle = _get_handle(args)
    results = engine.list_scopes(handle, args.prefix)
    for s in results:
        print(f"  {s}")
    print(f"\n  {len(results)} scope(s)")


def cmd_value(args):
    handle = _get_handle(args)
    ts_ps = _get_timescale_ps(handle)
    t = parse_time(args.time, timescale_ps=ts_ps)
    val = engine.get_value(handle, args.signal, t)
    x_marker = " [X]" if val.get("is_x") else ""
    z_marker = " [Z]" if val.get("is_z") else ""
    print(f"  {args.signal} @ {t}ps = 0x{val['hex']}{x_marker}{z_marker}")


def cmd_transitions(args):
    handle = _get_handle(args)
    ts_ps = _get_timescale_ps(handle)
    t0 = parse_time(args.t0, timescale_ps=ts_ps)
    t1 = parse_time(args.t1, timescale_ps=ts_ps)
    result = engine.get_transitions(handle, args.signal, t0, t1, args.max_edges)
    print(f"  {args.signal}: {result['total_transitions']} transitions in [{t0}, {t1}]")
    if result["truncated"]:
        print(f"  (showing first {len(result['transitions'])}, truncated)")
    for tr in result["transitions"]:
        print(f"    t={tr['time']:>12} : 0x{tr['value']}")


def cmd_snapshot(args):
    handle = _get_handle(args)
    ts_ps = _get_timescale_ps(handle)
    t = parse_time(args.time, timescale_ps=ts_ps)
    result = engine.get_snapshot(handle, args.signals, t)
    print(f"  Snapshot @ {t}ps:")
    for sig, val in result.items():
        x_marker = " [X]" if val.get("is_x") else ""
        z_marker = " [Z]" if val.get("is_z") else ""
        print(f"    {sig} = 0x{val['hex']}{x_marker}{z_marker}")


def cmd_anomalies(args):
    handle = _get_handle(args)
    ts_ps = _get_timescale_ps(handle)
    t0 = parse_time(args.t0, timescale_ps=ts_ps)
    t1 = parse_time(args.t1, timescale_ps=ts_ps)
    expected = int(args.expected_period) if args.expected_period else None
    anomalies = engine.find_anomalies(handle, args.signal, t0, t1, expected)
    if not anomalies:
        print("  No anomalies detected.")
    else:
        print(f"  {len(anomalies)} anomaly/anomalies:")
        for a in anomalies:
            print(f"    t={a['time_ps']:>12} [{a['kind']}] {a['detail']}")


def cmd_summarize(args):
    handle = _get_handle(args)
    ts_ps = _get_timescale_ps(handle)
    t0 = parse_time(args.t0, timescale_ps=ts_ps)
    t1 = parse_time(args.t1, timescale_ps=ts_ps)
    result = engine.summarize(handle, args.signal, t0, t1)
    print(f"  Summary for {args.signal} [{t0}, {t1}]:")
    print(f"    total_transitions: {result['total_transitions']}")
    if result.get("dominant_period_ps") is not None:
        print(f"    dominant_period_ps: {result['dominant_period_ps']}")
    if result.get("duty_cycle") is not None:
        print(f"    duty_cycle: {result['duty_cycle']:.3f}")
    if result.get("anomalies"):
        print(f"    anomalies: {len(result['anomalies'])}")
        for a in result["anomalies"]:
            print(f"      t={a['time_ps']:>12} [{a['kind']}] {a['detail']}")


def cmd_serve(args):
    from tsunami.server import start_server
    if args.file:
        print(f"Starting tsunami MCP server for: {args.file}", file=sys.stderr)
    else:
        print("Starting tsunami MCP server (no file pre-loaded)", file=sys.stderr)
    start_server(args.file)


def main():
    parser = argparse.ArgumentParser(
        prog="tsunami",
        description="AI-assisted hardware waveform debugging tool",
    )
    subparsers = parser.add_subparsers(dest="command", required=True)

    # info
    p = subparsers.add_parser("info", help="Show waveform metadata")
    p.add_argument("file", help="Waveform file (FST/VCD)")
    p.set_defaults(func=cmd_info)

    # signals
    p = subparsers.add_parser("signals", help="Search for signals")
    p.add_argument("file", help="Waveform file")
    p.add_argument("pattern", nargs="?", default="*", help="Glob pattern (default: *)")
    p.set_defaults(func=cmd_signals)

    # scopes
    p = subparsers.add_parser("scopes", help="Browse scope hierarchy")
    p.add_argument("file", help="Waveform file")
    p.add_argument("prefix", nargs="?", default="", help="Scope prefix")
    p.set_defaults(func=cmd_scopes)

    # value
    p = subparsers.add_parser("value", help="Get signal value at a time")
    p.add_argument("file", help="Waveform file")
    p.add_argument("signal", help="Signal path")
    p.add_argument("time", help="Time (e.g., 1284ns, 100ps, 642cyc)")
    p.set_defaults(func=cmd_value)

    # transitions
    p = subparsers.add_parser("transitions", help="Get signal transitions in range")
    p.add_argument("file", help="Waveform file")
    p.add_argument("signal", help="Signal path")
    p.add_argument("t0", help="Start time")
    p.add_argument("t1", help="End time")
    p.add_argument("--max-edges", type=int, default=1000, help="Max edges (default: 1000)")
    p.set_defaults(func=cmd_transitions)

    # snapshot
    p = subparsers.add_parser("snapshot", help="Get multiple signal values at a time")
    p.add_argument("file", help="Waveform file")
    p.add_argument("time", help="Time point")
    p.add_argument("signals", nargs="+", help="Signal paths")
    p.set_defaults(func=cmd_snapshot)

    # anomalies
    p = subparsers.add_parser("anomalies", help="Detect signal anomalies")
    p.add_argument("file", help="Waveform file")
    p.add_argument("signal", help="Signal path")
    p.add_argument("t0", help="Start time")
    p.add_argument("t1", help="End time")
    p.add_argument("--expected-period", help="Expected period in ps")
    p.set_defaults(func=cmd_anomalies)

    # summarize
    p = subparsers.add_parser("summarize", help="Summarize a signal in a window")
    p.add_argument("file", help="Waveform file")
    p.add_argument("signal", help="Signal path")
    p.add_argument("t0", help="Start time")
    p.add_argument("t1", help="End time")
    p.set_defaults(func=cmd_summarize)

    # serve
    p = subparsers.add_parser("serve", help="Start MCP server")
    p.add_argument("file", nargs="?", default=None, help="Waveform file (FST/VCD), optional")
    p.set_defaults(func=cmd_serve)

    args = parser.parse_args()
    args.func(args)


if __name__ == "__main__":
    main()
