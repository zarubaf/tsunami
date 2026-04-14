use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use wellen::simple::read;
use wellen::{Hierarchy, Signal, SignalRef, SignalValue, Time, Var, VarRef};

/// Opaque handle to an opened waveform file.
#[derive(Clone)]
pub struct WaveformHandle {
    inner: Arc<Mutex<wellen::simple::Waveform>>,
}

impl WaveformHandle {
    pub fn open(path: &str) -> Result<Self, String> {
        let wave =
            read(path).map_err(|e| format!("Failed to open waveform: {e}"))?;
        Ok(Self {
            inner: Arc::new(Mutex::new(wave)),
        })
    }

    pub fn with_wave<F, R>(&self, f: F) -> Result<R, String>
    where
        F: FnOnce(&mut wellen::simple::Waveform) -> Result<R, String>,
    {
        let mut wave = self
            .inner
            .lock()
            .map_err(|e| format!("Lock error: {e}"))?;
        f(&mut wave)
    }
}

// ---------------------------------------------------------------------------
// Value helpers
// ---------------------------------------------------------------------------

/// Format a SignalValue as a hex string.
pub fn signal_value_to_hex(val: &SignalValue) -> String {
    match val {
        SignalValue::Binary(bytes, width) => {
            let hex_digits = (*width as usize + 3) / 4;
            let mut result = String::with_capacity(hex_digits);
            let num_bytes = (hex_digits + 1) / 2;
            for i in (0..num_bytes).rev() {
                if i < bytes.len() {
                    result.push_str(&format!("{:02x}", bytes[i]));
                } else {
                    result.push_str("00");
                }
            }
            let trimmed = result.trim_start_matches('0');
            if trimmed.is_empty() {
                "0".to_string()
            } else {
                trimmed.to_string()
            }
        }
        SignalValue::FourValue(_bytes, width) => match val.to_bit_string() {
            Some(s) => {
                if s.contains('x') || s.contains('z') {
                    s
                } else {
                    let hex_digits = (s.len() + 3) / 4;
                    let padded = format!("{:0>width$}", s, width = hex_digits * 4);
                    let mut hex = String::with_capacity(hex_digits);
                    for chunk in padded.as_bytes().chunks(4) {
                        let nibble = chunk
                            .iter()
                            .fold(0u8, |acc, &b| (acc << 1) | if b == b'1' { 1 } else { 0 });
                        hex.push(char::from_digit(nibble as u32, 16).unwrap());
                    }
                    let trimmed = hex.trim_start_matches('0');
                    if trimmed.is_empty() {
                        "0".to_string()
                    } else {
                        trimmed.to_string()
                    }
                }
            }
            None => format!("{width}-bit 4-value"),
        },
        SignalValue::NineValue(_, width) => match val.to_bit_string() {
            Some(s) => s,
            None => format!("{width}-bit 9-value"),
        },
        SignalValue::Real(r) => format!("{r}"),
        SignalValue::String(s) => s.to_string(),
        _ => "?".to_string(),
    }
}

fn has_x_or_z(val: &SignalValue) -> (bool, bool) {
    match val.to_bit_string() {
        Some(s) => (
            s.contains('x') || s.contains('X'),
            s.contains('z') || s.contains('Z'),
        ),
        None => (false, false),
    }
}

/// Convert a signal value to an unsigned integer (returns None for x/z values).
pub fn signal_value_to_u64(val: &SignalValue) -> Option<u64> {
    match val {
        SignalValue::Binary(bytes, _width) => {
            let mut result: u64 = 0;
            for (i, &byte) in bytes.iter().enumerate() {
                result |= (byte as u64) << (i * 8);
            }
            Some(result)
        }
        SignalValue::FourValue(_, _) | SignalValue::NineValue(_, _) => {
            let (is_x, is_z) = has_x_or_z(val);
            if is_x || is_z {
                return None;
            }
            val.to_bit_string()
                .and_then(|s| u64::from_str_radix(&s, 2).ok())
        }
        SignalValue::Real(r) => Some(*r as u64),
        SignalValue::String(_) | _ => None,
    }
}

/// Resolve a dotted signal path to a VarRef.
pub fn resolve_signal(hier: &Hierarchy, path: &str) -> Result<VarRef, String> {
    let parts: Vec<&str> = path.split('.').collect();
    if parts.len() < 2 {
        return Err(format!(
            "Signal path must have at least scope.name, got: {path}"
        ));
    }
    let scope_parts = &parts[..parts.len() - 1];
    let var_name = parts[parts.len() - 1];

    hier.lookup_var(
        &scope_parts.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        var_name.to_string(),
    )
    .ok_or_else(|| format!("Signal not found: {path}"))
}

/// Get the time table index for a given time (last index <= target).
pub fn time_to_idx(time_table: &[Time], target: Time) -> u32 {
    match time_table.binary_search(&target) {
        Ok(i) => i as u32,
        Err(0) => 0,
        Err(i) => (i - 1) as u32,
    }
}

/// Get value of a loaded signal at a time table index.
pub fn get_signal_value_at_idx(signal: &Signal, tt_idx: u32) -> Option<SignalValue<'_>> {
    signal
        .get_offset(tt_idx)
        .map(|offset| signal.get_value_at(&offset, 0))
}

// ---------------------------------------------------------------------------
// Return types (serde-serializable)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct ValueInfo {
    pub hex: String,
    pub is_x: bool,
    pub is_z: bool,
}

impl ValueInfo {
    pub fn from_signal_value(val: &SignalValue) -> Self {
        let hex = signal_value_to_hex(val);
        let (is_x, is_z) = has_x_or_z(val);
        Self { hex, is_x, is_z }
    }

    pub fn unknown() -> Self {
        Self {
            hex: "x".to_string(),
            is_x: true,
            is_z: false,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SignalInfo {
    pub path: String,
    pub width: u32,
    #[serde(rename = "type")]
    pub var_type: String,
    pub direction: String,
    pub index: Option<String>,
}

fn make_signal_info(hier: &Hierarchy, var: &Var) -> SignalInfo {
    SignalInfo {
        path: var.full_name(hier),
        width: var.length().unwrap_or(1),
        var_type: format!("{:?}", var.var_type()),
        direction: format!("{:?}", var.direction()),
        index: var
            .index()
            .map(|idx| format!("[{}:{}]", idx.msb(), idx.lsb())),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct WaveformInfo {
    pub timescale_factor: u32,
    pub timescale_unit: String,
    pub duration: u64,
    pub num_signals: usize,
    pub num_time_points: usize,
    pub file_format: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WaveformLength {
    pub start_time: u64,
    pub end_time: u64,
    pub time_steps: usize,
    pub timescale: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Transition {
    pub time: u64,
    pub value: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TransitionResult {
    pub signal: String,
    pub t0_ps: u64,
    pub t1_ps: u64,
    pub total_transitions: usize,
    pub truncated: bool,
    pub transitions: Vec<Transition>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValueRange {
    pub start: u64,
    pub end: u64,
    pub value: String,
}

// ---------------------------------------------------------------------------
// Comparison helpers
// ---------------------------------------------------------------------------

pub fn edge_matches(direction: &str, prev: u64, curr: u64) -> Result<bool, String> {
    let is_rising = curr > prev;
    let is_falling = curr < prev;
    match direction {
        "rising" => Ok(is_rising),
        "falling" => Ok(is_falling),
        "any" => Ok(is_rising || is_falling),
        _ => Err("direction must be 'rising', 'falling', or 'any'".to_string()),
    }
}

pub fn parse_compare_value(value: &str) -> Result<u64, String> {
    if let Some(hex) = value.strip_prefix("0x").or_else(|| value.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16).map_err(|_| format!("Invalid hex value: {value}"))
    } else if let Some(bin) = value.strip_prefix("0b").or_else(|| value.strip_prefix("0B")) {
        u64::from_str_radix(bin, 2).map_err(|_| format!("Invalid binary value: {value}"))
    } else {
        value
            .parse::<u64>()
            .map_err(|_| format!("Invalid decimal value: {value}"))
    }
}

pub fn compare_matches(condition: &str, current: u64, expected: u64) -> Result<bool, String> {
    match condition {
        "eq" => Ok(current == expected),
        "neq" => Ok(current != expected),
        "gt" => Ok(current > expected),
        "lt" => Ok(current < expected),
        "gte" => Ok(current >= expected),
        "lte" => Ok(current <= expected),
        _ => Err("condition must be one of 'eq', 'neq', 'gt', 'lt', 'gte', or 'lte'".to_string()),
    }
}

// ---------------------------------------------------------------------------
// Query functions (pure Rust, no PyO3)
// ---------------------------------------------------------------------------

pub fn waveform_info(handle: &WaveformHandle) -> Result<WaveformInfo, String> {
    handle.with_wave(|wave| {
        let hier = wave.hierarchy();
        let time_table = wave.time_table();

        let (factor, unit) = if let Some(ts) = hier.timescale() {
            (ts.factor, format!("{:?}", ts.unit))
        } else {
            (1, "Unknown".to_string())
        };

        Ok(WaveformInfo {
            timescale_factor: factor,
            timescale_unit: unit,
            duration: time_table.last().copied().unwrap_or(0),
            num_signals: hier.num_unique_signals(),
            num_time_points: time_table.len(),
            file_format: format!("{:?}", hier.file_format()),
        })
    })
}

pub fn get_waveform_length(handle: &WaveformHandle) -> Result<WaveformLength, String> {
    handle.with_wave(|wave| {
        let hier = wave.hierarchy();
        let time_table = wave.time_table();

        let timescale = hier
            .timescale()
            .map(|ts| format!("{} {:?}", ts.factor, ts.unit));

        Ok(WaveformLength {
            start_time: time_table.first().copied().unwrap_or(0),
            end_time: time_table.last().copied().unwrap_or(0),
            time_steps: time_table.len(),
            timescale,
        })
    })
}

pub fn list_signals(handle: &WaveformHandle, pattern: &str) -> Result<Vec<SignalInfo>, String> {
    handle.with_wave(|wave| {
        let hier = wave.hierarchy();
        let glob = glob::Pattern::new(pattern)
            .map_err(|e| format!("Invalid glob pattern: {e}"))?;

        let mut results = Vec::new();
        for var_ref in hier.iter_vars() {
            let var: &Var = &var_ref;
            let full_name = var.full_name(hier);
            if glob.matches(&full_name) {
                results.push(make_signal_info(hier, var));
            }
        }
        Ok(results)
    })
}

pub fn get_signal_info(handle: &WaveformHandle, signal: &str) -> Result<SignalInfo, String> {
    handle.with_wave(|wave| {
        let hier = wave.hierarchy();
        let var_ref = resolve_signal(hier, signal)?;
        let var: &Var = &hier[var_ref];
        Ok(make_signal_info(hier, var))
    })
}

pub fn list_scopes(handle: &WaveformHandle, prefix: &str) -> Result<Vec<String>, String> {
    handle.with_wave(|wave| {
        let hier = wave.hierarchy();
        let mut results = Vec::new();
        for scope in hier.iter_scopes() {
            let full_name = scope.full_name(hier);
            if prefix.is_empty() || full_name.starts_with(prefix) {
                results.push(full_name);
            }
        }
        Ok(results)
    })
}

pub fn get_value(handle: &WaveformHandle, signal: &str, time_ps: u64) -> Result<ValueInfo, String> {
    handle.with_wave(|wave| {
        let hier = wave.hierarchy();
        let var_ref = resolve_signal(hier, signal)?;
        let var: &Var = &hier[var_ref];
        let sig_ref = var.signal_ref();

        wave.load_signals(&[sig_ref]);

        let time_table = wave.time_table();
        let tt_idx = time_to_idx(time_table, time_ps);

        let signal_data = wave
            .get_signal(sig_ref)
            .ok_or_else(|| "Failed to load signal data".to_string())?;

        let info = if let Some(val) = get_signal_value_at_idx(signal_data, tt_idx) {
            ValueInfo::from_signal_value(&val)
        } else {
            ValueInfo::unknown()
        };

        wave.unload_signals(&[sig_ref]);
        Ok(info)
    })
}

pub fn get_snapshot(
    handle: &WaveformHandle,
    signals: &[String],
    time_ps: u64,
) -> Result<HashMap<String, ValueInfo>, String> {
    handle.with_wave(|wave| {
        let hier = wave.hierarchy();
        let mut var_refs: Vec<(String, VarRef, SignalRef)> = Vec::new();

        for sig_path in signals {
            let var_ref = resolve_signal(hier, sig_path)?;
            let var: &Var = &hier[var_ref];
            var_refs.push((sig_path.clone(), var_ref, var.signal_ref()));
        }

        let sig_refs: Vec<SignalRef> = var_refs.iter().map(|(_, _, sr)| *sr).collect();
        wave.load_signals(&sig_refs);

        let time_table = wave.time_table();
        let tt_idx = time_to_idx(time_table, time_ps);

        let mut result = HashMap::new();
        for (path, _, sig_ref) in &var_refs {
            if let Some(signal_data) = wave.get_signal(*sig_ref) {
                let info = if let Some(val) = get_signal_value_at_idx(signal_data, tt_idx) {
                    ValueInfo::from_signal_value(&val)
                } else {
                    ValueInfo::unknown()
                };
                result.insert(path.clone(), info);
            }
        }

        wave.unload_signals(&sig_refs);
        Ok(result)
    })
}

pub fn get_transitions(
    handle: &WaveformHandle,
    signal: &str,
    t0_ps: u64,
    t1_ps: u64,
    max_edges: usize,
) -> Result<TransitionResult, String> {
    handle.with_wave(|wave| {
        let hier = wave.hierarchy();
        let var_ref = resolve_signal(hier, signal)?;
        let var: &Var = &hier[var_ref];
        let sig_ref = var.signal_ref();

        wave.load_signals(&[sig_ref]);

        let time_table = wave.time_table();
        let signal_data = wave
            .get_signal(sig_ref)
            .ok_or_else(|| "Failed to load signal data".to_string())?;

        let mut transitions = Vec::new();
        let mut total_in_range = 0usize;
        let mut truncated = false;

        for &tt_idx in signal_data.time_indices() {
            let t = time_table[tt_idx as usize];
            if t < t0_ps {
                continue;
            }
            if t > t1_ps {
                break;
            }
            total_in_range += 1;
            if transitions.len() >= max_edges {
                truncated = true;
                continue;
            }
            if let Some(offset) = signal_data.get_offset(tt_idx) {
                let val = signal_data.get_value_at(&offset, 0);
                transitions.push(Transition {
                    time: t,
                    value: signal_value_to_hex(&val),
                });
            }
        }

        wave.unload_signals(&[sig_ref]);

        Ok(TransitionResult {
            signal: signal.to_string(),
            t0_ps,
            t1_ps,
            total_transitions: total_in_range,
            truncated,
            transitions,
        })
    })
}

pub fn find_edges(
    handle: &WaveformHandle,
    signal: &str,
    direction: &str,
    start_ps: u64,
    end_ps: Option<u64>,
    limit: usize,
) -> Result<Vec<u64>, String> {
    handle.with_wave(|wave| {
        if limit < 1 {
            return Err("limit must be at least 1".to_string());
        }
        let hier = wave.hierarchy();
        let var_ref = resolve_signal(hier, signal)?;
        let var: &Var = &hier[var_ref];
        let sig_ref = var.signal_ref();

        wave.load_signals(&[sig_ref]);

        let time_table = wave.time_table();
        let signal_data = wave
            .get_signal(sig_ref)
            .ok_or_else(|| "Failed to load signal data".to_string())?;

        let mut prev_val: Option<u64> = None;
        let mut matches = Vec::new();

        for &tt_idx in signal_data.time_indices() {
            let t = time_table[tt_idx as usize];
            if let Some(offset) = signal_data.get_offset(tt_idx) {
                let val = signal_data.get_value_at(&offset, 0);
                let numeric = signal_value_to_u64(&val);
                if let Some(end) = end_ps {
                    if t > end {
                        break;
                    }
                }

                if t >= start_ps {
                    if let (Some(prev), Some(curr)) = (prev_val, numeric) {
                        if edge_matches(direction, prev, curr)? {
                            matches.push(t);
                            if matches.len() >= limit {
                                break;
                            }
                        }
                    }
                }
                prev_val = numeric;
            }
        }

        wave.unload_signals(&[sig_ref]);
        Ok(matches)
    })
}

pub fn find_value(
    handle: &WaveformHandle,
    signal: &str,
    condition: &str,
    value: &str,
    start_ps: u64,
    end_ps: Option<u64>,
    limit: usize,
) -> Result<Vec<ValueRange>, String> {
    handle.with_wave(|wave| {
        if limit < 1 {
            return Err("limit must be at least 1".to_string());
        }
        let expected = parse_compare_value(value)?;
        let hier = wave.hierarchy();
        let var_ref = resolve_signal(hier, signal)?;
        let var: &Var = &hier[var_ref];
        let sig_ref = var.signal_ref();

        wave.load_signals(&[sig_ref]);

        let time_table = wave.time_table();
        let signal_data = wave
            .get_signal(sig_ref)
            .ok_or_else(|| "Failed to load signal data".to_string())?;

        let mut matches = Vec::new();
        let mut active_start: Option<u64> = None;
        let mut active_value = String::new();
        let mut last_match_time: Option<u64> = None;

        for &tt_idx in signal_data.time_indices() {
            let t = time_table[tt_idx as usize];
            if let Some(end) = end_ps {
                if t > end {
                    break;
                }
            }
            if let Some(offset) = signal_data.get_offset(tt_idx) {
                let val = signal_data.get_value_at(&offset, 0);
                let numeric = signal_value_to_u64(&val);
                let hex = signal_value_to_hex(&val);

                if t < start_ps {
                    if let Some(current) = numeric {
                        if compare_matches(condition, current, expected)? {
                            active_start = Some(start_ps);
                            active_value = hex;
                        } else {
                            active_start = None;
                            active_value.clear();
                        }
                    } else {
                        active_start = None;
                        active_value.clear();
                    }
                    continue;
                }

                let is_match = match numeric {
                    Some(current) => compare_matches(condition, current, expected)?,
                    None => false,
                };

                if is_match {
                    if active_start.is_none() {
                        active_start = Some(t);
                        active_value = hex;
                    }
                    last_match_time = Some(t);
                } else if let Some(range_start) = active_start.take() {
                    matches.push(ValueRange {
                        start: range_start,
                        end: t,
                        value: active_value.clone(),
                    });
                    if matches.len() >= limit {
                        wave.unload_signals(&[sig_ref]);
                        return Ok(matches);
                    }
                    active_value.clear();
                    last_match_time = None;
                }
            }
        }

        if let Some(range_start) = active_start {
            let range_end = end_ps.or(last_match_time).unwrap_or(range_start);
            matches.push(ValueRange {
                start: range_start,
                end: range_end,
                value: active_value,
            });
        }

        wave.unload_signals(&[sig_ref]);
        Ok(matches)
    })
}

/// Timescale in picoseconds, derived from waveform info.
pub fn timescale_ps(info: &WaveformInfo) -> u64 {
    let unit_ps: f64 = match info.timescale_unit.as_str() {
        "FemtoSeconds" => 0.001,
        "PicoSeconds" => 1.0,
        "NanoSeconds" => 1_000.0,
        "MicroSeconds" => 1_000_000.0,
        "MilliSeconds" => 1_000_000_000.0,
        "Seconds" => 1_000_000_000_000.0,
        _ => 1.0,
    };
    (info.timescale_factor as f64 * unit_ps) as u64
}
