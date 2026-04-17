use std::collections::HashMap;
use std::sync::{atomic::AtomicUsize, atomic::Ordering, Mutex};

use rmcp::{
    ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use tsunami_core::predicate::Expr;
use tsunami_core::query::{self, WaveformHandle};
use tsunami_core::summarise;
use tsunami_core::time_parse::parse_time;

struct WaveformSession {
    handle: WaveformHandle,
    timescale_ps: u64,
    path: String,
}

pub struct TsunamiServer {
    sessions: Mutex<HashMap<String, WaveformSession>>,
    next_id: AtomicUsize,
    tool_router: ToolRouter<Self>,
}

impl TsunamiServer {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            next_id: AtomicUsize::new(1),
            tool_router: Self::tool_router(),
        }
    }

    pub fn open_session(&self, path: &str, session_id: Option<String>) -> Result<String, String> {
        let handle = WaveformHandle::open(path)?;
        let info = query::waveform_info(&handle)?;
        let timescale_ps = query::timescale_ps(&info);
        let id = session_id.unwrap_or_else(|| {
            let n = self.next_id.fetch_add(1, Ordering::Relaxed);
            format!("session-{n}")
        });
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|e| format!("Lock error: {e}"))?;
        sessions.insert(
            id.clone(),
            WaveformSession {
                handle,
                timescale_ps,
                path: path.to_string(),
            },
        );
        Ok(id)
    }

    fn with_session<F, R>(&self, session_id: &str, f: F) -> Result<R, String>
    where
        F: FnOnce(&WaveformSession) -> Result<R, String>,
    {
        let sessions = self
            .sessions
            .lock()
            .map_err(|e| format!("Lock error: {e}"))?;
        let session = sessions.get(session_id).ok_or_else(|| {
            format!("Unknown waveform session '{session_id}'. Call open_waveform(path) first.")
        })?;
        f(session)
    }

    fn parse_t(&self, session_id: &str, value: &str) -> Result<u64, String> {
        self.with_session(session_id, |s| parse_time(value, Some(s.timescale_ps)))
    }
}

// ---------------------------------------------------------------------------
// Range combination helpers (ported from Python server.py)
// ---------------------------------------------------------------------------

fn merge_ranges(ranges: &mut Vec<(u64, u64)>) -> Vec<(u64, u64)> {
    if ranges.is_empty() {
        return Vec::new();
    }
    ranges.sort();
    let mut merged: Vec<(u64, u64)> = Vec::new();
    for &(start, end) in ranges.iter() {
        if let Some(last) = merged.last_mut() {
            if start <= last.1 {
                last.1 = last.1.max(end);
                continue;
            }
        }
        merged.push((start, end));
    }
    merged
}

fn intersect_ranges(left: &[(u64, u64)], right: &[(u64, u64)]) -> Vec<(u64, u64)> {
    let mut result = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < left.len() && j < right.len() {
        let start = left[i].0.max(right[j].0);
        let end = left[i].1.min(right[j].1);
        if start <= end {
            result.push((start, end));
        }
        if left[i].1 < right[j].1 {
            i += 1;
        } else {
            j += 1;
        }
    }
    result
}

fn combine_condition_ranges(
    range_sets: &[Vec<(u64, u64)>],
    combine: &str,
) -> Result<Vec<(u64, u64)>, String> {
    match combine {
        "or" => {
            let mut all: Vec<(u64, u64)> = range_sets.iter().flatten().copied().collect();
            Ok(merge_ranges(&mut all))
        }
        "and" => {
            if range_sets.is_empty() {
                return Ok(Vec::new());
            }
            let mut merged_sets: Vec<Vec<(u64, u64)>> = range_sets
                .iter()
                .map(|r| {
                    let mut r = r.clone();
                    merge_ranges(&mut r)
                })
                .collect();
            let mut result = merged_sets.remove(0);
            for ranges in &merged_sets {
                result = intersect_ranges(&result, ranges);
                if result.is_empty() {
                    break;
                }
            }
            Ok(result)
        }
        _ => Err("combine must be either 'and' or 'or'".to_string()),
    }
}

fn paginate<T: Clone>(items: &[T], limit: usize, offset: usize) -> Result<(Vec<T>, usize), String> {
    if limit < 1 {
        return Err("limit must be at least 1".to_string());
    }
    let total = items.len();
    let page = items.iter().skip(offset).take(limit).cloned().collect();
    Ok((page, total))
}

// ---------------------------------------------------------------------------
// Parameter structs (serde + schemars for JSON schema generation)
// ---------------------------------------------------------------------------

#[derive(Deserialize, JsonSchema)]
struct OpenWaveformParams {
    #[schemars(description = "Absolute path to the waveform file (FST or VCD)")]
    path: String,
}

#[derive(Deserialize, JsonSchema)]
struct SessionParams {
    #[schemars(description = "Waveform session ID returned by open_waveform")]
    session_id: String,
}

#[derive(Deserialize, JsonSchema)]
struct SearchSignalsParams {
    #[schemars(description = "Waveform session ID")]
    session_id: String,
    #[schemars(description = "Glob pattern (default: \"*\"). Examples: \"*clk*\", \"tb.dut.*valid*\"")]
    pattern: Option<String>,
    #[schemars(description = "Max results to return (default: 50)")]
    limit: Option<usize>,
    #[schemars(description = "Offset for pagination (default: 0)")]
    offset: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
struct BrowseScopesParams {
    #[schemars(description = "Waveform session ID")]
    session_id: String,
    #[schemars(description = "Scope prefix filter")]
    prefix: Option<String>,
    #[schemars(description = "Root scope to browse under")]
    root: Option<String>,
    #[schemars(description = "Maximum depth to descend")]
    max_depth: Option<usize>,
    #[schemars(description = "Max results (default: 50)")]
    limit: Option<usize>,
    #[schemars(description = "Offset for pagination (default: 0)")]
    offset: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
struct GetSignalInfoParams {
    #[schemars(description = "Waveform session ID")]
    session_id: String,
    #[schemars(description = "Dotted signal path (e.g. \"tb.dut.clk\")")]
    signal: String,
}

#[derive(Deserialize, JsonSchema)]
struct FindEdgeParams {
    #[schemars(description = "Waveform session ID")]
    session_id: String,
    #[schemars(description = "Dotted signal path")]
    signal: String,
    #[schemars(description = "Edge type: \"rising\", \"falling\", or \"any\"")]
    edge: String,
    #[schemars(description = "Start time (e.g. \"1284ns\", default: \"0\")")]
    start: Option<String>,
    #[schemars(description = "End time (optional)")]
    end: Option<String>,
    #[schemars(description = "Max results (default: 50)")]
    limit: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
struct FindValueParams {
    #[schemars(description = "Waveform session ID")]
    session_id: String,
    #[schemars(description = "Dotted signal path")]
    signal: String,
    #[schemars(description = "Comparison: eq, neq, gt, lt, gte, lte")]
    condition: String,
    #[schemars(description = "Value to compare (decimal, 0x hex, or 0b binary)")]
    value: String,
    #[schemars(description = "Start time (default: \"0\")")]
    start: Option<String>,
    #[schemars(description = "End time (optional)")]
    end: Option<String>,
    #[schemars(description = "Max results (default: 50)")]
    limit: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
struct FindPatternCondition {
    signal: String,
    condition: String,
    value: String,
}

#[derive(Deserialize, JsonSchema)]
struct FindPatternParams {
    #[schemars(description = "Waveform session ID")]
    session_id: String,
    #[schemars(description = "Array of conditions, each with signal, condition, and value")]
    conditions: Vec<FindPatternCondition>,
    #[schemars(description = "How to combine conditions: \"and\" or \"or\"")]
    combine: String,
    #[schemars(description = "Start time (default: \"0\")")]
    start: Option<String>,
    #[schemars(description = "End time (optional)")]
    end: Option<String>,
    #[schemars(description = "Max results (default: 50)")]
    limit: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
struct GetSnapshotParams {
    #[schemars(description = "Waveform session ID")]
    session_id: String,
    #[schemars(description = "List of signal paths")]
    signals: Vec<String>,
    #[schemars(description = "Time point (e.g. \"1284ns\", \"1.284us\", \"1284000\")")]
    time: String,
}

#[derive(Deserialize, JsonSchema)]
struct GetSignalWindowParams {
    #[schemars(description = "Waveform session ID")]
    session_id: String,
    #[schemars(description = "List of signal paths")]
    signals: Vec<String>,
    #[schemars(description = "Window start time")]
    t0: String,
    #[schemars(description = "Window end time")]
    t1: String,
    #[schemars(description = "Max edges per signal before auto-summarising (default: 200)")]
    max_edges_per_signal: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
struct FindFirstMatchParams {
    #[schemars(description = "Waveform session ID")]
    session_id: String,
    #[schemars(description = "Predicate expression as JSON string")]
    predicate_json: String,
    #[schemars(description = "Search after this time (default: \"0\")")]
    after: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct FindAllMatchesParams {
    #[schemars(description = "Waveform session ID")]
    session_id: String,
    #[schemars(description = "Predicate expression as JSON string")]
    predicate_json: String,
    #[schemars(description = "Window start time")]
    t0: String,
    #[schemars(description = "Window end time")]
    t1: String,
}

#[derive(Deserialize, JsonSchema)]
struct FindAnomaliesParams {
    #[schemars(description = "Waveform session ID")]
    session_id: String,
    #[schemars(description = "Dotted signal path")]
    signal: String,
    #[schemars(description = "Window start time")]
    t0: String,
    #[schemars(description = "Window end time")]
    t1: String,
    #[schemars(description = "Expected period in ps (optional, auto-detected if omitted)")]
    expected_period_ps: Option<u64>,
}

// ---------------------------------------------------------------------------
// MCP Tool implementations
// ---------------------------------------------------------------------------

#[tool_router]
impl TsunamiServer {
    #[tool(description = "Open a waveform file (FST or VCD) and return a session ID.")]
    fn open_waveform(&self, Parameters(p): Parameters<OpenWaveformParams>) -> String {
        let result = (|| -> Result<serde_json::Value, String> {
            let session_id = self.open_session(&p.path, None)?;
            let info = self.with_session(&session_id, |s| query::waveform_info(&s.handle))?;
            Ok(json!({
                "session_id": session_id,
                "path": p.path,
                "timescale_factor": info.timescale_factor,
                "timescale_unit": info.timescale_unit,
                "duration": info.duration,
                "num_signals": info.num_signals,
                "num_time_points": info.num_time_points,
                "file_format": info.file_format,
            }))
        })();
        match result {
            Ok(v) => v.to_string(),
            Err(e) => json!({"error": e}).to_string(),
        }
    }

    #[tool(description = "Get waveform metadata for an open waveform session.")]
    fn waveform_info(&self, Parameters(p): Parameters<SessionParams>) -> String {
        let result = self.with_session(&p.session_id, |s| {
            let info = query::waveform_info(&s.handle)?;
            Ok(json!({
                "session_id": p.session_id,
                "path": s.path,
                "timescale_factor": info.timescale_factor,
                "timescale_unit": info.timescale_unit,
                "duration": info.duration,
                "num_signals": info.num_signals,
                "num_time_points": info.num_time_points,
                "file_format": info.file_format,
            }))
        });
        match result {
            Ok(v) => v.to_string(),
            Err(e) => json!({"error": e}).to_string(),
        }
    }

    #[tool(description = "Get start/end time and time-step count for an open waveform session.")]
    fn get_waveform_length(&self, Parameters(p): Parameters<SessionParams>) -> String {
        let result = self.with_session(&p.session_id, |s| {
            let len = query::get_waveform_length(&s.handle)?;
            Ok(json!({
                "session_id": p.session_id,
                "start_time": len.start_time,
                "end_time": len.end_time,
                "time_steps": len.time_steps,
                "timescale": len.timescale,
            }))
        });
        match result {
            Ok(v) => v.to_string(),
            Err(e) => json!({"error": e}).to_string(),
        }
    }

    #[tool(description = "Search for signals matching a glob pattern in a waveform session. Examples: \"*clk*\", \"tb.dut.*valid*\", \"*tl_a*\"")]
    fn search_signals(&self, Parameters(p): Parameters<SearchSignalsParams>) -> String {
        let pattern = p.pattern.unwrap_or_else(|| "*".to_string());
        let limit = p.limit.unwrap_or(50);
        let offset = p.offset.unwrap_or(0);

        let result = self.with_session(&p.session_id, |s| {
            let signals = query::list_signals(&s.handle, &pattern)?;
            let (page, total) = paginate(&signals, limit, offset)?;
            Ok(json!({
                "session_id": p.session_id,
                "pattern": pattern,
                "limit": limit,
                "offset": offset,
                "total_count": total,
                "signals": page,
            }))
        });
        match result {
            Ok(v) => v.to_string(),
            Err(e) => json!({"error": e}).to_string(),
        }
    }

    #[tool(description = "Browse the signal hierarchy for an open waveform session.")]
    fn browse_scopes(&self, Parameters(p): Parameters<BrowseScopesParams>) -> String {
        let prefix = p.prefix.unwrap_or_default();
        let limit = p.limit.unwrap_or(50);
        let offset = p.offset.unwrap_or(0);

        let result = self.with_session(&p.session_id, |s| {
            if let Some(ref md) = p.max_depth {
                if *md < 1 {
                    return Err("max_depth must be at least 1".to_string());
                }
            }

            let mut scopes = query::list_scopes(&s.handle, &prefix)?;

            let root_depth = if let Some(ref r) = p.root {
                if !scopes.contains(r) {
                    return Err(format!(
                        "Unknown scope '{r}'. Use browse_scopes without root to discover scopes."
                    ));
                }
                let root_prefix = format!("{r}.");
                scopes.retain(|scope| scope.starts_with(&root_prefix));
                r.matches('.').count() + 1
            } else {
                0
            };

            if let Some(md) = p.max_depth {
                scopes.retain(|scope| scope.matches('.').count() < root_depth + md);
            }

            let (page, total) = paginate(&scopes, limit, offset)?;
            Ok(json!({
                "session_id": p.session_id,
                "prefix": prefix,
                "root": p.root,
                "max_depth": p.max_depth,
                "limit": limit,
                "offset": offset,
                "total_count": total,
                "scopes": page,
            }))
        });
        match result {
            Ok(v) => v.to_string(),
            Err(e) => json!({"error": e}).to_string(),
        }
    }

    #[tool(description = "Get metadata for a single signal in a waveform session.")]
    fn get_signal_info(&self, Parameters(p): Parameters<GetSignalInfoParams>) -> String {
        let result = self.with_session(&p.session_id, |s| {
            let info = query::get_signal_info(&s.handle, &p.signal)?;
            Ok(json!({
                "session_id": p.session_id,
                "signal_info": info,
            }))
        });
        match result {
            Ok(v) => v.to_string(),
            Err(e) => json!({"error": e}).to_string(),
        }
    }

    #[tool(description = "Find rising, falling, or any edges on a signal within an optional window.")]
    fn find_edge(&self, Parameters(p): Parameters<FindEdgeParams>) -> String {
        let start_str = p.start.unwrap_or_else(|| "0".to_string());
        let limit = p.limit.unwrap_or(50);

        let result = (|| -> Result<serde_json::Value, String> {
            let start_ps = self.parse_t(&p.session_id, &start_str)?;
            let end_ps = p.end.as_ref().map(|s| self.parse_t(&p.session_id, s)).transpose()?;

            self.with_session(&p.session_id, |s| {
                let matches =
                    query::find_edges(&s.handle, &p.signal, &p.edge, start_ps, end_ps, limit)?;
                Ok(json!({
                    "session_id": p.session_id,
                    "signal": p.signal,
                    "edge": p.edge,
                    "start_ps": start_ps,
                    "end_ps": end_ps,
                    "limit": limit,
                    "matches": matches,
                }))
            })
        })();
        match result {
            Ok(v) => v.to_string(),
            Err(e) => json!({"error": e}).to_string(),
        }
    }

    #[tool(description = "Find contiguous ranges where a signal matches a simple comparison.")]
    fn find_value(&self, Parameters(p): Parameters<FindValueParams>) -> String {
        let start_str = p.start.unwrap_or_else(|| "0".to_string());
        let limit = p.limit.unwrap_or(50);

        let result = (|| -> Result<serde_json::Value, String> {
            let start_ps = self.parse_t(&p.session_id, &start_str)?;
            let end_ps = p.end.as_ref().map(|s| self.parse_t(&p.session_id, s)).transpose()?;

            self.with_session(&p.session_id, |s| {
                let matches = query::find_value(
                    &s.handle,
                    &p.signal,
                    &p.condition,
                    &p.value,
                    start_ps,
                    end_ps,
                    limit,
                )?;
                Ok(json!({
                    "session_id": p.session_id,
                    "signal": p.signal,
                    "condition": p.condition,
                    "value": p.value,
                    "start_ps": start_ps,
                    "end_ps": end_ps,
                    "limit": limit,
                    "matches": matches,
                }))
            })
        })();
        match result {
            Ok(v) => v.to_string(),
            Err(e) => json!({"error": e}).to_string(),
        }
    }

    #[tool(description = "Find ranges where multiple simple signal conditions hold together.")]
    fn find_pattern(&self, Parameters(p): Parameters<FindPatternParams>) -> String {
        let start_str = p.start.unwrap_or_else(|| "0".to_string());
        let limit = p.limit.unwrap_or(50);

        let result = (|| -> Result<serde_json::Value, String> {
            if p.conditions.is_empty() {
                return Err("conditions must not be empty".to_string());
            }

            let start_ps = self.parse_t(&p.session_id, &start_str)?;
            let end_ps = p.end.as_ref().map(|s| self.parse_t(&p.session_id, s)).transpose()?;

            self.with_session(&p.session_id, |s| {
                let waveform_length = query::get_waveform_length(&s.handle)?;
                let max_ranges = waveform_length.time_steps;

                let mut condition_ranges: Vec<Vec<(u64, u64)>> = Vec::new();
                for cond in &p.conditions {
                    let matches = query::find_value(
                        &s.handle,
                        &cond.signal,
                        &cond.condition,
                        &cond.value,
                        start_ps,
                        end_ps,
                        max_ranges,
                    )?;
                    condition_ranges.push(matches.iter().map(|m| (m.start, m.end)).collect());
                }

                let combined = combine_condition_ranges(&condition_ranges, &p.combine)?;
                let (page, total) = paginate(&combined, limit, 0)?;
                let matches: Vec<_> = page
                    .iter()
                    .map(|&(start, end)| json!({"start": start, "end": end}))
                    .collect();

                Ok(json!({
                    "session_id": p.session_id,
                    "combine": p.combine,
                    "start_ps": start_ps,
                    "end_ps": end_ps,
                    "limit": limit,
                    "total_count": total,
                    "matches": matches,
                }))
            })
        })();
        match result {
            Ok(v) => v.to_string(),
            Err(e) => json!({"error": e}).to_string(),
        }
    }

    #[tool(description = "Get values of multiple signals at a single time point.")]
    fn get_snapshot(&self, Parameters(p): Parameters<GetSnapshotParams>) -> String {
        let result = (|| -> Result<serde_json::Value, String> {
            let t = self.parse_t(&p.session_id, &p.time)?;
            self.with_session(&p.session_id, |s| {
                let values = query::get_snapshot(&s.handle, &p.signals, t)?;
                Ok(json!({
                    "session_id": p.session_id,
                    "time_ps": t,
                    "values": values,
                }))
            })
        })();
        match result {
            Ok(v) => v.to_string(),
            Err(e) => json!({"error": e}).to_string(),
        }
    }

    #[tool(description = "Get transitions for multiple signals in a time window. Auto-summarises if a signal has more than max_edges_per_signal transitions.")]
    fn get_signal_window(&self, Parameters(p): Parameters<GetSignalWindowParams>) -> String {
        let max_edges = p.max_edges_per_signal.unwrap_or(200);

        let result = (|| -> Result<serde_json::Value, String> {
            let t0_ps = self.parse_t(&p.session_id, &p.t0)?;
            let t1_ps = self.parse_t(&p.session_id, &p.t1)?;

            self.with_session(&p.session_id, |s| {
                let mut result = serde_json::Map::new();

                for sig in &p.signals {
                    let transitions =
                        query::get_transitions(&s.handle, sig, t0_ps, t1_ps, max_edges)?;
                    if transitions.truncated {
                        let summary = summarise::summarize(&s.handle, sig, t0_ps, t1_ps)?;
                        result.insert(
                            sig.clone(),
                            json!({
                                "mode": "summary",
                                "total_transitions": transitions.total_transitions,
                                "dominant_period_ps": summary.dominant_period_ps,
                                "duty_cycle": summary.duty_cycle,
                                "value_histogram": summary.value_histogram,
                                "anomalies": summary.anomalies,
                            }),
                        );
                    } else {
                        result.insert(
                            sig.clone(),
                            json!({
                                "mode": "transitions",
                                "signal": transitions.signal,
                                "t0_ps": transitions.t0_ps,
                                "t1_ps": transitions.t1_ps,
                                "total_transitions": transitions.total_transitions,
                                "truncated": transitions.truncated,
                                "transitions": transitions.transitions,
                            }),
                        );
                    }
                }

                Ok(json!({
                    "session_id": p.session_id,
                    "t0_ps": t0_ps,
                    "t1_ps": t1_ps,
                    "signals": result,
                }))
            })
        })();
        match result {
            Ok(v) => v.to_string(),
            Err(e) => json!({"error": e}).to_string(),
        }
    }

    #[tool(description = "Find first timestamp matching a predicate expression.")]
    fn find_first_match(&self, Parameters(p): Parameters<FindFirstMatchParams>) -> String {
        let after_str = p.after.unwrap_or_else(|| "0".to_string());

        let result = (|| -> Result<serde_json::Value, String> {
            let after_ps = self.parse_t(&p.session_id, &after_str)?;
            let expr: Expr = serde_json::from_str(&p.predicate_json)
                .map_err(|e| format!("Invalid predicate JSON: {e}"))?;

            self.with_session(&p.session_id, |s| {
                let match_time = tsunami_core::predicate::find_first(&s.handle, &expr, after_ps)?;
                Ok(json!({
                    "session_id": p.session_id,
                    "after_ps": after_ps,
                    "match_time_ps": match_time,
                }))
            })
        })();
        match result {
            Ok(v) => v.to_string(),
            Err(e) => json!({"error": e}).to_string(),
        }
    }

    #[tool(description = "Find all timestamps matching a predicate expression in a window.")]
    fn find_all_matches(&self, Parameters(p): Parameters<FindAllMatchesParams>) -> String {
        let result = (|| -> Result<serde_json::Value, String> {
            let t0_ps = self.parse_t(&p.session_id, &p.t0)?;
            let t1_ps = self.parse_t(&p.session_id, &p.t1)?;
            let expr: Expr = serde_json::from_str(&p.predicate_json)
                .map_err(|e| format!("Invalid predicate JSON: {e}"))?;

            self.with_session(&p.session_id, |s| {
                let matches =
                    tsunami_core::predicate::find_all(&s.handle, &expr, t0_ps, t1_ps)?;
                Ok(json!({
                    "session_id": p.session_id,
                    "t0_ps": t0_ps,
                    "t1_ps": t1_ps,
                    "matches": matches,
                }))
            })
        })();
        match result {
            Ok(v) => v.to_string(),
            Err(e) => json!({"error": e}).to_string(),
        }
    }

    #[tool(description = "Detect anomalies in a signal: glitches, unexpected gaps, stuck signals.")]
    fn find_anomalies(&self, Parameters(p): Parameters<FindAnomaliesParams>) -> String {
        let result = (|| -> Result<serde_json::Value, String> {
            let t0_ps = self.parse_t(&p.session_id, &p.t0)?;
            let t1_ps = self.parse_t(&p.session_id, &p.t1)?;

            self.with_session(&p.session_id, |s| {
                let anomalies = summarise::find_anomalies(
                    &s.handle,
                    &p.signal,
                    t0_ps,
                    t1_ps,
                    p.expected_period_ps,
                )?;
                Ok(json!({
                    "session_id": p.session_id,
                    "signal": p.signal,
                    "t0_ps": t0_ps,
                    "t1_ps": t1_ps,
                    "anomalies": anomalies,
                }))
            })
        })();
        match result {
            Ok(v) => v.to_string(),
            Err(e) => json!({"error": e}).to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// ServerHandler implementation
// ---------------------------------------------------------------------------

#[tool_handler]
impl ServerHandler for TsunamiServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some("Tsunami waveform debugging MCP server. Open a waveform file first with open_waveform, then use the returned session_id for all other tools.".to_string()),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}
