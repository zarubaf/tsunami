use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};
use std::sync::{Arc, Mutex};
use wellen::simple::read;
use wellen::{GetItem, Hierarchy, Signal, SignalRef, SignalValue, Time, Var, VarRef};

/// Opaque handle to an opened waveform file.
#[pyclass]
pub struct WaveformHandle {
    inner: Arc<Mutex<wellen::simple::Waveform>>,
}

impl WaveformHandle {
    pub fn open(path: &str) -> PyResult<Self> {
        let wave = read(path).map_err(|e| PyValueError::new_err(format!("Failed to open waveform: {e}")))?;
        Ok(Self {
            inner: Arc::new(Mutex::new(wave)),
        })
    }

    pub fn with_wave<F, R>(&self, f: F) -> PyResult<R>
    where
        F: FnOnce(&mut wellen::simple::Waveform) -> PyResult<R>,
    {
        let mut wave = self.inner.lock().map_err(|e| PyValueError::new_err(format!("Lock error: {e}")))?;
        f(&mut wave)
    }
}

/// Format a SignalValue as a hex string.
pub fn signal_value_to_hex(val: &SignalValue) -> String {
    match val {
        SignalValue::Binary(bytes, width) => {
            let hex_digits = (*width as usize + 3) / 4;
            let mut result = String::with_capacity(hex_digits);
            // bytes are little-endian packed bits
            let num_bytes = (hex_digits + 1) / 2;
            for i in (0..num_bytes).rev() {
                if i < bytes.len() {
                    result.push_str(&format!("{:02x}", bytes[i]));
                } else {
                    result.push_str("00");
                }
            }
            // Trim leading zeros but keep at least one digit
            let trimmed = result.trim_start_matches('0');
            if trimmed.is_empty() { "0".to_string() } else { trimmed.to_string() }
        }
        SignalValue::FourValue(_bytes, width) => {
            // 4-value: each bit uses 2 bits of storage (value + mask)
            match val.to_bit_string() {
                Some(s) => {
                    // If contains x or z, return bit string
                    if s.contains('x') || s.contains('z') {
                        s
                    } else {
                        // Convert binary string to hex
                        let hex_digits = (s.len() + 3) / 4;
                        let padded = format!("{:0>width$}", s, width = hex_digits * 4);
                        let mut hex = String::with_capacity(hex_digits);
                        for chunk in padded.as_bytes().chunks(4) {
                            let nibble = chunk.iter().fold(0u8, |acc, &b| {
                                (acc << 1) | if b == b'1' { 1 } else { 0 }
                            });
                            hex.push(char::from_digit(nibble as u32, 16).unwrap());
                        }
                        let trimmed = hex.trim_start_matches('0');
                        if trimmed.is_empty() { "0".to_string() } else { trimmed.to_string() }
                    }
                }
                None => format!("{width}-bit 4-value", width = width),
            }
        }
        SignalValue::NineValue(_, width) => {
            match val.to_bit_string() {
                Some(s) => s,
                None => format!("{width}-bit 9-value", width = width),
            }
        }
        SignalValue::Real(r) => format!("{r}"),
        SignalValue::String(s) => s.to_string(),
    }
}

fn has_x_or_z(val: &SignalValue) -> (bool, bool) {
    match val.to_bit_string() {
        Some(s) => (s.contains('x') || s.contains('X'), s.contains('z') || s.contains('Z')),
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
            // Parse from bit string
            val.to_bit_string().and_then(|s| u64::from_str_radix(&s, 2).ok())
        }
        SignalValue::Real(r) => Some(*r as u64),
        _ => None,
    }
}

/// Resolve a dotted signal path to a VarRef.
fn resolve_signal(hier: &Hierarchy, path: &str) -> PyResult<VarRef> {
    let parts: Vec<&str> = path.split('.').collect();
    if parts.len() < 2 {
        return Err(PyValueError::new_err(format!(
            "Signal path must have at least scope.name, got: {path}"
        )));
    }
    let scope_parts = &parts[..parts.len() - 1];
    let var_name = parts[parts.len() - 1];

    hier.lookup_var(
        &scope_parts.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        &var_name.to_string(),
    )
    .ok_or_else(|| PyValueError::new_err(format!("Signal not found: {path}")))
}

/// Get the time table index for a given time (last index <= target).
fn time_to_idx(time_table: &[Time], target: Time) -> u32 {
    match time_table.binary_search(&target) {
        Ok(i) => i as u32,
        Err(0) => 0,
        Err(i) => (i - 1) as u32,
    }
}

/// Get value of a loaded signal at a time table index.
pub fn get_signal_value_at_idx<'a>(signal: &'a Signal, tt_idx: u32) -> Option<SignalValue<'a>> {
    signal.get_offset(tt_idx).map(|offset| signal.get_value_at(&offset, 0))
}

fn make_value_dict<'py>(py: Python<'py>, val: &SignalValue) -> PyResult<Bound<'py, PyDict>> {
    let dict = PyDict::new(py);
    let hex = signal_value_to_hex(val);
    let (is_x, is_z) = has_x_or_z(val);
    dict.set_item("hex", hex)?;
    dict.set_item("is_x", is_x)?;
    dict.set_item("is_z", is_z)?;
    Ok(dict)
}

#[pyfunction]
pub fn waveform_info(py: Python<'_>, handle: &WaveformHandle) -> PyResult<PyObject> {
    handle.with_wave(|wave| {
        let hier = wave.hierarchy();
        let time_table = wave.time_table();
        let dict = PyDict::new(py);

        if let Some(timescale) = hier.timescale() {
            dict.set_item("timescale_factor", timescale.factor)?;
            dict.set_item("timescale_unit", format!("{:?}", timescale.unit))?;
        } else {
            dict.set_item("timescale_factor", 1)?;
            dict.set_item("timescale_unit", "Unknown")?;
        }

        let duration = time_table.last().copied().unwrap_or(0);
        dict.set_item("duration", duration)?;
        dict.set_item("num_signals", hier.num_unique_signals())?;
        dict.set_item("num_time_points", time_table.len())?;
        dict.set_item("file_format", format!("{:?}", hier.file_format()))?;
        Ok(dict.into())
    })
}

#[pyfunction]
#[pyo3(signature = (handle, pattern="*"))]
pub fn list_signals(py: Python<'_>, handle: &WaveformHandle, pattern: &str) -> PyResult<PyObject> {
    handle.with_wave(|wave| {
        let hier = wave.hierarchy();
        let mut results = Vec::new();

        // Build glob pattern
        let glob = glob::Pattern::new(pattern)
            .map_err(|e| PyValueError::new_err(format!("Invalid glob pattern: {e}")))?;

        for var_ref in hier.iter_vars() {
            let var: &Var = &var_ref;
            let full_name = var.full_name(hier);
            if glob.matches(&full_name) {
                let dict = PyDict::new(py);
                dict.set_item("path", &full_name)?;
                dict.set_item("width", var.length().unwrap_or(1))?;
                dict.set_item("type", format!("{:?}", var.var_type()))?;
                dict.set_item("direction", format!("{:?}", var.direction()))?;
                results.push(dict);
            }
        }

        Ok(PyList::new(py, &results)?.into())
    })
}

#[pyfunction]
#[pyo3(signature = (handle, prefix=""))]
pub fn list_scopes(py: Python<'_>, handle: &WaveformHandle, prefix: &str) -> PyResult<PyObject> {
    handle.with_wave(|wave| {
        let hier = wave.hierarchy();
        let mut results = Vec::new();

        for scope in hier.iter_scopes() {
            let full_name = scope.full_name(hier);
            if prefix.is_empty() || full_name.starts_with(prefix) {
                results.push(full_name);
            }
        }

        Ok(PyList::new(py, &results)?.into())
    })
}

#[pyfunction]
pub fn get_value(py: Python<'_>, handle: &WaveformHandle, signal: &str, time_ps: u64) -> PyResult<PyObject> {
    handle.with_wave(|wave| {
        let hier = wave.hierarchy();
        let var_ref = resolve_signal(hier, signal)?;
        let var: &Var = hier.get(var_ref);
        let sig_ref = var.signal_ref();

        wave.load_signals(&[sig_ref]);

        let time_table = wave.time_table();
        let tt_idx = time_to_idx(time_table, time_ps);

        let signal_data = wave.get_signal(sig_ref)
            .ok_or_else(|| PyValueError::new_err("Failed to load signal data"))?;

        let dict = if let Some(val) = get_signal_value_at_idx(signal_data, tt_idx) {
            make_value_dict(py, &val)?
        } else {
            let d = PyDict::new(py);
            d.set_item("hex", "x")?;
            d.set_item("is_x", true)?;
            d.set_item("is_z", false)?;
            d
        };

        wave.unload_signals(&[sig_ref]);
        Ok(dict.into())
    })
}

#[pyfunction]
pub fn get_snapshot(py: Python<'_>, handle: &WaveformHandle, signals: Vec<String>, time_ps: u64) -> PyResult<PyObject> {
    handle.with_wave(|wave| {
        let hier = wave.hierarchy();
        let mut var_refs: Vec<(String, VarRef, SignalRef)> = Vec::new();

        for sig_path in &signals {
            let var_ref = resolve_signal(hier, sig_path)?;
            let var: &Var = hier.get(var_ref);
            var_refs.push((sig_path.clone(), var_ref, var.signal_ref()));
        }

        let sig_refs: Vec<SignalRef> = var_refs.iter().map(|(_, _, sr)| *sr).collect();
        wave.load_signals(&sig_refs);

        let time_table = wave.time_table();
        let tt_idx = time_to_idx(time_table, time_ps);

        let result = PyDict::new(py);
        for (path, _, sig_ref) in &var_refs {
            if let Some(signal_data) = wave.get_signal(*sig_ref) {
                if let Some(val) = get_signal_value_at_idx(signal_data, tt_idx) {
                    result.set_item(path, make_value_dict(py, &val)?)?;
                } else {
                    let d = PyDict::new(py);
                    d.set_item("hex", "x")?;
                    d.set_item("is_x", true)?;
                    d.set_item("is_z", false)?;
                    result.set_item(path, d)?;
                }
            }
        }

        wave.unload_signals(&sig_refs);
        Ok(result.into())
    })
}

#[pyfunction]
#[pyo3(signature = (handle, signal, t0_ps, t1_ps, max_edges=1000))]
pub fn get_transitions(
    py: Python<'_>,
    handle: &WaveformHandle,
    signal: &str,
    t0_ps: u64,
    t1_ps: u64,
    max_edges: usize,
) -> PyResult<PyObject> {
    handle.with_wave(|wave| {
        let hier = wave.hierarchy();
        let var_ref = resolve_signal(hier, signal)?;
        let var: &Var = hier.get(var_ref);
        let sig_ref = var.signal_ref();

        wave.load_signals(&[sig_ref]);

        let time_table = wave.time_table();
        let signal_data = wave.get_signal(sig_ref)
            .ok_or_else(|| PyValueError::new_err("Failed to load signal data"))?;

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
                continue; // keep counting total
            }
            if let Some(offset) = signal_data.get_offset(tt_idx) {
                let val = signal_data.get_value_at(&offset, 0);
                let entry = PyDict::new(py);
                entry.set_item("time", t)?;
                entry.set_item("value", signal_value_to_hex(&val))?;
                transitions.push(entry);
            }
        }

        let result = PyDict::new(py);
        result.set_item("signal", signal)?;
        result.set_item("t0_ps", t0_ps)?;
        result.set_item("t1_ps", t1_ps)?;
        result.set_item("total_transitions", total_in_range)?;
        result.set_item("truncated", truncated)?;
        result.set_item("transitions", PyList::new(py, &transitions)?)?;

        wave.unload_signals(&[sig_ref]);
        Ok(result.into())
    })
}

#[pyfunction]
pub fn find_next_edge(
    py: Python<'_>,
    handle: &WaveformHandle,
    signal: &str,
    direction: &str,
    after_ps: u64,
) -> PyResult<PyObject> {
    handle.with_wave(|wave| {
        let hier = wave.hierarchy();
        let var_ref = resolve_signal(hier, signal)?;
        let var: &Var = hier.get(var_ref);
        let sig_ref = var.signal_ref();

        wave.load_signals(&[sig_ref]);

        let time_table = wave.time_table();
        let signal_data = wave.get_signal(sig_ref)
            .ok_or_else(|| PyValueError::new_err("Failed to load signal data"))?;

        let mut prev_val: Option<u64> = None;
        let mut result_time: Option<u64> = None;

        for &tt_idx in signal_data.time_indices() {
            let t = time_table[tt_idx as usize];
            if let Some(offset) = signal_data.get_offset(tt_idx) {
                let val = signal_data.get_value_at(&offset, 0);
                let numeric = signal_value_to_u64(&val);

                if t > after_ps {
                    if let (Some(prev), Some(curr)) = (prev_val, numeric) {
                        let is_rising = curr > prev;
                        let is_falling = curr < prev;
                        let matched = match direction {
                            "rising" => is_rising,
                            "falling" => is_falling,
                            "any" => is_rising || is_falling,
                            _ => return Err(PyValueError::new_err(
                                "direction must be 'rising', 'falling', or 'any'"
                            )),
                        };
                        if matched {
                            result_time = Some(t);
                            break;
                        }
                    }
                }
                prev_val = numeric;
            }
        }

        wave.unload_signals(&[sig_ref]);

        match result_time {
            Some(t) => Ok(t.into_pyobject(py)?.into_any().unbind()),
            None => Ok(py.None()),
        }
    })
}
