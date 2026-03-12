use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};
use std::collections::{BTreeSet, HashMap};
use wellen::{GetItem, SignalRef, Time, Var};

use crate::query::{get_signal_value_at_idx, signal_value_to_u64, WaveformHandle};

/// Expression AST — mirrors the Python Expr dataclass hierarchy.
/// Uses FromPyObject for automatic conversion from Python.
#[derive(Debug, Clone)]
pub enum Expr {
    Signal(String),
    Const(u64),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
    Xor(Box<Expr>, Box<Expr>),
    Eq(Box<Expr>, Box<Expr>),
    Gt(Box<Expr>, Box<Expr>),
    Lt(Box<Expr>, Box<Expr>),
    Rise(Box<Expr>),
    Fall(Box<Expr>),
    BitSlice(Box<Expr>, u32, u32), // expr, high, low
    Sequence {
        a: Box<Expr>,
        b: Box<Expr>,
        within_ps: Option<u64>,
    },
    PrecededBy {
        a: Box<Expr>,
        b: Box<Expr>,
        within_ps: Option<u64>,
    },
}

impl<'py> FromPyObject<'py> for Expr {
    fn extract_bound(ob: &Bound<'py, PyAny>) -> PyResult<Self> {
        let tag: String = ob.getattr("tag")?.extract()?;
        match tag.as_str() {
            "signal" => {
                let path: String = ob.getattr("path")?.extract()?;
                Ok(Expr::Signal(path))
            }
            "const" => {
                let value: u64 = ob.getattr("value")?.extract()?;
                Ok(Expr::Const(value))
            }
            "and" => {
                let left: Expr = ob.getattr("left")?.extract()?;
                let right: Expr = ob.getattr("right")?.extract()?;
                Ok(Expr::And(Box::new(left), Box::new(right)))
            }
            "or" => {
                let left: Expr = ob.getattr("left")?.extract()?;
                let right: Expr = ob.getattr("right")?.extract()?;
                Ok(Expr::Or(Box::new(left), Box::new(right)))
            }
            "not" => {
                let inner: Expr = ob.getattr("inner")?.extract()?;
                Ok(Expr::Not(Box::new(inner)))
            }
            "xor" => {
                let left: Expr = ob.getattr("left")?.extract()?;
                let right: Expr = ob.getattr("right")?.extract()?;
                Ok(Expr::Xor(Box::new(left), Box::new(right)))
            }
            "eq" => {
                let left: Expr = ob.getattr("left")?.extract()?;
                let right: Expr = ob.getattr("right")?.extract()?;
                Ok(Expr::Eq(Box::new(left), Box::new(right)))
            }
            "gt" => {
                let left: Expr = ob.getattr("left")?.extract()?;
                let right: Expr = ob.getattr("right")?.extract()?;
                Ok(Expr::Gt(Box::new(left), Box::new(right)))
            }
            "lt" => {
                let left: Expr = ob.getattr("left")?.extract()?;
                let right: Expr = ob.getattr("right")?.extract()?;
                Ok(Expr::Lt(Box::new(left), Box::new(right)))
            }
            "rise" => {
                let inner: Expr = ob.getattr("inner")?.extract()?;
                Ok(Expr::Rise(Box::new(inner)))
            }
            "fall" => {
                let inner: Expr = ob.getattr("inner")?.extract()?;
                Ok(Expr::Fall(Box::new(inner)))
            }
            "bit_slice" => {
                let inner: Expr = ob.getattr("inner")?.extract()?;
                let high: u32 = ob.getattr("high")?.extract()?;
                let low: u32 = ob.getattr("low")?.extract()?;
                Ok(Expr::BitSlice(Box::new(inner), high, low))
            }
            "sequence" => {
                let a: Expr = ob.getattr("a")?.extract()?;
                let b: Expr = ob.getattr("b")?.extract()?;
                let within_ps: Option<u64> = ob.getattr("within_ps")?.extract()?;
                Ok(Expr::Sequence {
                    a: Box::new(a),
                    b: Box::new(b),
                    within_ps,
                })
            }
            "preceded_by" => {
                let a: Expr = ob.getattr("a")?.extract()?;
                let b: Expr = ob.getattr("b")?.extract()?;
                let within_ps: Option<u64> = ob.getattr("within_ps")?.extract()?;
                Ok(Expr::PrecededBy {
                    a: Box::new(a),
                    b: Box::new(b),
                    within_ps,
                })
            }
            _ => Err(PyValueError::new_err(format!("Unknown expr tag: {tag}"))),
        }
    }
}

/// Collect all signal paths referenced in an expression.
fn collect_signals(expr: &Expr, out: &mut Vec<String>) {
    match expr {
        Expr::Signal(path) => {
            if !out.contains(path) {
                out.push(path.clone());
            }
        }
        Expr::Const(_) => {}
        Expr::And(a, b)
        | Expr::Or(a, b)
        | Expr::Xor(a, b)
        | Expr::Eq(a, b)
        | Expr::Gt(a, b)
        | Expr::Lt(a, b) => {
            collect_signals(a, out);
            collect_signals(b, out);
        }
        Expr::Not(a) | Expr::Rise(a) | Expr::Fall(a) | Expr::BitSlice(a, _, _) => {
            collect_signals(a, out);
        }
        Expr::Sequence { a, b, .. } | Expr::PrecededBy { a, b, .. } => {
            collect_signals(a, out);
            collect_signals(b, out);
        }
    }
}

/// Evaluate a non-temporal expression at a given time table index.
/// Returns a numeric value (truthy = non-zero) or None if x/z.
fn eval_at(
    expr: &Expr,
    tt_idx: u32,
    prev_tt_idx: Option<u32>,
    signals: &HashMap<String, &wellen::Signal>,
) -> Option<u64> {
    match expr {
        Expr::Signal(path) => {
            let sig = signals.get(path.as_str())?;
            let val = get_signal_value_at_idx(sig, tt_idx)?;
            signal_value_to_u64(&val)
        }
        Expr::Const(v) => Some(*v),
        Expr::And(a, b) => {
            let va = eval_at(a, tt_idx, prev_tt_idx, signals)?;
            let vb = eval_at(b, tt_idx, prev_tt_idx, signals)?;
            Some(if va != 0 && vb != 0 { 1 } else { 0 })
        }
        Expr::Or(a, b) => {
            let va = eval_at(a, tt_idx, prev_tt_idx, signals)?;
            let vb = eval_at(b, tt_idx, prev_tt_idx, signals)?;
            Some(if va != 0 || vb != 0 { 1 } else { 0 })
        }
        Expr::Not(a) => {
            let va = eval_at(a, tt_idx, prev_tt_idx, signals)?;
            Some(if va == 0 { 1 } else { 0 })
        }
        Expr::Xor(a, b) => {
            let va = eval_at(a, tt_idx, prev_tt_idx, signals)?;
            let vb = eval_at(b, tt_idx, prev_tt_idx, signals)?;
            Some(if (va != 0) ^ (vb != 0) { 1 } else { 0 })
        }
        Expr::Eq(a, b) => {
            let va = eval_at(a, tt_idx, prev_tt_idx, signals)?;
            let vb = eval_at(b, tt_idx, prev_tt_idx, signals)?;
            Some(if va == vb { 1 } else { 0 })
        }
        Expr::Gt(a, b) => {
            let va = eval_at(a, tt_idx, prev_tt_idx, signals)?;
            let vb = eval_at(b, tt_idx, prev_tt_idx, signals)?;
            Some(if va > vb { 1 } else { 0 })
        }
        Expr::Lt(a, b) => {
            let va = eval_at(a, tt_idx, prev_tt_idx, signals)?;
            let vb = eval_at(b, tt_idx, prev_tt_idx, signals)?;
            Some(if va < vb { 1 } else { 0 })
        }
        Expr::Rise(inner) => {
            let prev = prev_tt_idx?;
            let curr_val = eval_at(inner, tt_idx, None, signals)?;
            let prev_val = eval_at(inner, prev, None, signals)?;
            Some(if prev_val == 0 && curr_val != 0 { 1 } else { 0 })
        }
        Expr::Fall(inner) => {
            let prev = prev_tt_idx?;
            let curr_val = eval_at(inner, tt_idx, None, signals)?;
            let prev_val = eval_at(inner, prev, None, signals)?;
            Some(if prev_val != 0 && curr_val == 0 { 1 } else { 0 })
        }
        Expr::BitSlice(inner, high, low) => {
            let val = eval_at(inner, tt_idx, prev_tt_idx, signals)?;
            let width = high - low + 1;
            let mask = if width >= 64 { u64::MAX } else { (1u64 << width) - 1 };
            Some((val >> low) & mask)
        }
        // Temporal operators (Sequence, PrecededBy) are handled at a higher level
        Expr::Sequence { .. } | Expr::PrecededBy { .. } => None,
    }
}

/// Check if an expression is temporal (contains Sequence or PrecededBy).
fn is_temporal(expr: &Expr) -> bool {
    match expr {
        Expr::Sequence { .. } | Expr::PrecededBy { .. } => true,
        Expr::And(a, b)
        | Expr::Or(a, b)
        | Expr::Xor(a, b)
        | Expr::Eq(a, b)
        | Expr::Gt(a, b)
        | Expr::Lt(a, b) => is_temporal(a) || is_temporal(b),
        Expr::Not(a) | Expr::Rise(a) | Expr::Fall(a) | Expr::BitSlice(a, _, _) => is_temporal(a),
        Expr::Signal(_) | Expr::Const(_) => false,
    }
}

/// Core scan: find all time points in [t0, t1] where expr evaluates to true.
fn scan_expr(
    wave: &mut wellen::simple::Waveform,
    expr: &Expr,
    t0_ps: u64,
    t1_ps: u64,
) -> PyResult<Vec<(u64, u64)>> {
    // Collect all referenced signals
    let mut signal_paths = Vec::new();
    collect_signals(expr, &mut signal_paths);

    let hier = wave.hierarchy();

    // Resolve and load signals
    let mut sig_refs_map: HashMap<String, SignalRef> = HashMap::new();
    let mut sig_refs: Vec<SignalRef> = Vec::new();

    for path in &signal_paths {
        let parts: Vec<&str> = path.split('.').collect();
        if parts.len() < 2 {
            return Err(PyValueError::new_err(format!("Invalid signal path: {path}")));
        }
        let scope_parts = &parts[..parts.len() - 1];
        let var_name = parts[parts.len() - 1];

        let var_ref = hier
            .lookup_var(
                &scope_parts.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
                &var_name.to_string(),
            )
            .ok_or_else(|| PyValueError::new_err(format!("Signal not found: {path}")))?;
        let var: &Var = hier.get(var_ref);
        let sr = var.signal_ref();
        sig_refs_map.insert(path.clone(), sr);
        if !sig_refs.contains(&sr) {
            sig_refs.push(sr);
        }
    }

    wave.load_signals(&sig_refs);

    let time_table = wave.time_table();

    // Build union of all transition time indices in range
    let mut transition_indices: BTreeSet<u32> = BTreeSet::new();
    for sr in &sig_refs {
        if let Some(signal) = wave.get_signal(*sr) {
            for &tt_idx in signal.time_indices() {
                let t = time_table[tt_idx as usize];
                if t >= t0_ps && t <= t1_ps {
                    transition_indices.insert(tt_idx);
                }
            }
        }
    }

    // Build signal lookup map
    let signal_map: HashMap<String, &wellen::Signal> = sig_refs_map
        .iter()
        .filter_map(|(path, sr)| wave.get_signal(*sr).map(|sig| (path.clone(), sig)))
        .collect();

    // Handle temporal expressions
    if is_temporal(expr) {
        let results = eval_temporal(expr, &transition_indices, time_table, &signal_map)?;
        wave.unload_signals(&sig_refs);
        return Ok(results);
    }

    // Evaluate at each transition point
    let mut results: Vec<(u64, u64)> = Vec::new();
    let indices: Vec<u32> = transition_indices.into_iter().collect();

    for (i, &tt_idx) in indices.iter().enumerate() {
        let prev_idx = if i > 0 { Some(indices[i - 1]) } else { None };
        if let Some(val) = eval_at(expr, tt_idx, prev_idx, &signal_map) {
            if val != 0 {
                let t = time_table[tt_idx as usize];
                results.push((t, val));
            }
        }
    }

    wave.unload_signals(&sig_refs);
    Ok(results)
}

/// Evaluate temporal expressions (Sequence, PrecededBy).
fn eval_temporal(
    expr: &Expr,
    transition_indices: &BTreeSet<u32>,
    time_table: &[Time],
    signals: &HashMap<String, &wellen::Signal>,
) -> PyResult<Vec<(u64, u64)>> {
    match expr {
        Expr::Sequence { a, b, within_ps } => {
            // Find all times where `a` is true, then for each, find next time where `b` is true
            let indices: Vec<u32> = transition_indices.iter().copied().collect();
            let mut a_times: Vec<u64> = Vec::new();

            for (i, &tt_idx) in indices.iter().enumerate() {
                let prev_idx = if i > 0 { Some(indices[i - 1]) } else { None };
                if let Some(val) = eval_at(a, tt_idx, prev_idx, signals) {
                    if val != 0 {
                        a_times.push(time_table[tt_idx as usize]);
                    }
                }
            }

            let mut results = Vec::new();
            for a_time in &a_times {
                for (i, &tt_idx) in indices.iter().enumerate() {
                    let t = time_table[tt_idx as usize];
                    if t <= *a_time {
                        continue;
                    }
                    if let Some(window) = within_ps {
                        if t > a_time + window {
                            break;
                        }
                    }
                    let prev_idx = if i > 0 { Some(indices[i - 1]) } else { None };
                    if let Some(val) = eval_at(b, tt_idx, prev_idx, signals) {
                        if val != 0 {
                            results.push((t, 1u64));
                            break;
                        }
                    }
                }
            }
            Ok(results)
        }
        Expr::PrecededBy { a, b, within_ps } => {
            // Find all times where `a` is true and `b` was true within the window before
            let indices: Vec<u32> = transition_indices.iter().copied().collect();
            let mut results = Vec::new();

            for (i, &tt_idx) in indices.iter().enumerate() {
                let t = time_table[tt_idx as usize];
                let prev_idx = if i > 0 { Some(indices[i - 1]) } else { None };

                // Check if `a` is true at this point
                let a_val = eval_at(a, tt_idx, prev_idx, signals);
                if a_val != Some(1) && a_val.map_or(true, |v| v == 0) {
                    continue;
                }

                // Look back for `b`
                let mut found_b = false;
                for j in (0..i).rev() {
                    let bt = time_table[indices[j] as usize];
                    if let Some(window) = within_ps {
                        if t - bt > *window {
                            break;
                        }
                    }
                    let bprev = if j > 0 { Some(indices[j - 1]) } else { None };
                    if let Some(bval) = eval_at(b, indices[j], bprev, signals) {
                        if bval != 0 {
                            found_b = true;
                            break;
                        }
                    }
                }

                if found_b {
                    results.push((t, 1u64));
                }
            }
            Ok(results)
        }
        _ => {
            // Non-temporal: evaluate directly
            let indices: Vec<u32> = transition_indices.iter().copied().collect();
            let mut results = Vec::new();
            for (i, &tt_idx) in indices.iter().enumerate() {
                let prev_idx = if i > 0 { Some(indices[i - 1]) } else { None };
                if let Some(val) = eval_at(expr, tt_idx, prev_idx, signals) {
                    if val != 0 {
                        results.push((time_table[tt_idx as usize], val));
                    }
                }
            }
            Ok(results)
        }
    }
}

#[pyfunction]
pub fn find_first(
    py: Python<'_>,
    handle: &WaveformHandle,
    expr: Expr,
    after_ps: u64,
) -> PyResult<PyObject> {
    handle.with_wave(|wave| {
        let time_table = wave.time_table();
        let t1 = time_table.last().copied().unwrap_or(0);
        let results = scan_expr(wave, &expr, after_ps, t1)?;
        match results.first() {
            Some((t, _)) => Ok(t.into_pyobject(py)?.into_any().unbind()),
            None => Ok(py.None()),
        }
    })
}

#[pyfunction]
pub fn find_all(
    py: Python<'_>,
    handle: &WaveformHandle,
    expr: Expr,
    t0_ps: u64,
    t1_ps: u64,
) -> PyResult<PyObject> {
    handle.with_wave(|wave| {
        let results = scan_expr(wave, &expr, t0_ps, t1_ps)?;
        let times: Vec<u64> = results.iter().map(|(t, _)| *t).collect();
        Ok(PyList::new(py, &times)?.into())
    })
}

#[pyfunction]
pub fn scan(
    py: Python<'_>,
    handle: &WaveformHandle,
    expr: Expr,
    t0_ps: u64,
    t1_ps: u64,
) -> PyResult<PyObject> {
    handle.with_wave(|wave| {
        let results = scan_expr(wave, &expr, t0_ps, t1_ps)?;
        let py_results: Vec<Bound<'_, PyDict>> = results
            .iter()
            .map(|(t, v)| {
                let d = PyDict::new(py);
                d.set_item("time", *t)?;
                d.set_item("value", *v)?;
                Ok(d)
            })
            .collect::<PyResult<Vec<_>>>()?;
        Ok(PyList::new(py, &py_results)?.into())
    })
}
