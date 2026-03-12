use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};
use std::collections::HashMap;
use wellen::{GetItem, Time, Var};

use crate::query::{get_signal_value_at_idx, signal_value_to_u64, WaveformHandle};

/// Compute the dominant period from a list of intervals using a histogram approach.
fn compute_dominant_period(intervals: &[u64]) -> Option<u64> {
    if intervals.is_empty() {
        return None;
    }

    // Build histogram with 1% tolerance bucketing
    let mut buckets: HashMap<u64, usize> = HashMap::new();
    for &interval in intervals {
        // Round to nearest 1% bucket
        let bucket = if interval == 0 {
            0
        } else {
            let bucket_size = std::cmp::max(1, interval / 100);
            (interval / bucket_size) * bucket_size
        };
        *buckets.entry(bucket).or_insert(0) += 1;
    }

    // Find the most common bucket
    buckets
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(period, _)| period)
}

/// Compute duty cycle for a 1-bit signal.
fn compute_duty_cycle(
    signal: &wellen::Signal,
    time_table: &[Time],
    t0_ps: u64,
    t1_ps: u64,
) -> Option<f64> {
    let mut high_time: u64 = 0;
    let mut prev_time = t0_ps;
    let mut prev_val: Option<u64> = None;

    for &tt_idx in signal.time_indices() {
        let t = time_table[tt_idx as usize];
        if t > t1_ps {
            break;
        }
        if let Some(val) = get_signal_value_at_idx(signal, tt_idx) {
            let numeric = signal_value_to_u64(&val);
            if t >= t0_ps {
                if let Some(pv) = prev_val {
                    if pv != 0 {
                        high_time += t.saturating_sub(prev_time);
                    }
                }
                prev_time = t;
            }
            prev_val = numeric;
        }
    }

    // Account for time from last transition to t1
    if let Some(pv) = prev_val {
        if pv != 0 {
            high_time += t1_ps.saturating_sub(prev_time);
        }
    }

    let total = t1_ps.saturating_sub(t0_ps);
    if total == 0 {
        None
    } else {
        Some(high_time as f64 / total as f64)
    }
}

struct SummaryData {
    total_transitions: usize,
    dominant_period_ps: Option<u64>,
    duty_cycle: Option<f64>,
    value_histogram: HashMap<String, usize>,
    anomalies: Vec<AnomalyData>,
}

struct AnomalyData {
    time_ps: u64,
    kind: String,
    detail: String,
}

fn summarize_signal(
    wave: &mut wellen::simple::Waveform,
    signal_path: &str,
    t0_ps: u64,
    t1_ps: u64,
    expected_period_ps: Option<u64>,
) -> PyResult<SummaryData> {
    let hier = wave.hierarchy();
    let parts: Vec<&str> = signal_path.split('.').collect();
    if parts.len() < 2 {
        return Err(PyValueError::new_err(format!(
            "Invalid signal path: {signal_path}"
        )));
    }
    let scope_parts = &parts[..parts.len() - 1];
    let var_name = parts[parts.len() - 1];

    let var_ref = hier
        .lookup_var(
            &scope_parts.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            &var_name.to_string(),
        )
        .ok_or_else(|| PyValueError::new_err(format!("Signal not found: {signal_path}")))?;
    let var: &Var = hier.get(var_ref);
    let sig_ref = var.signal_ref();
    let is_1bit = var.length().unwrap_or(1) == 1;

    wave.load_signals(&[sig_ref]);

    let time_table = wave.time_table();
    let signal = wave
        .get_signal(sig_ref)
        .ok_or_else(|| PyValueError::new_err("Failed to load signal data"))?;

    let mut total_transitions = 0usize;
    let mut intervals: Vec<u64> = Vec::new();
    let mut prev_time: Option<u64> = None;
    let mut value_histogram: HashMap<String, usize> = HashMap::new();

    for &tt_idx in signal.time_indices() {
        let t = time_table[tt_idx as usize];
        if t < t0_ps {
            prev_time = Some(t);
            continue;
        }
        if t > t1_ps {
            break;
        }

        total_transitions += 1;

        if let Some(pt) = prev_time {
            if t > pt {
                intervals.push(t - pt);
            }
        }
        prev_time = Some(t);

        if let Some(val) = get_signal_value_at_idx(signal, tt_idx) {
            let hex = crate::query::signal_value_to_hex(&val);
            *value_histogram.entry(hex).or_insert(0) += 1;
        }
    }

    let dominant_period = expected_period_ps.or_else(|| compute_dominant_period(&intervals));
    let duty_cycle = if is_1bit {
        compute_duty_cycle(signal, time_table, t0_ps, t1_ps)
    } else {
        None
    };

    // Detect anomalies
    let anomalies = detect_anomalies(&intervals, dominant_period, signal, time_table, t0_ps, t1_ps);

    wave.unload_signals(&[sig_ref]);

    Ok(SummaryData {
        total_transitions,
        dominant_period_ps: dominant_period,
        duty_cycle,
        value_histogram,
        anomalies,
    })
}

fn detect_anomalies(
    _intervals: &[u64],
    dominant_period: Option<u64>,
    signal: &wellen::Signal,
    time_table: &[Time],
    t0_ps: u64,
    t1_ps: u64,
) -> Vec<AnomalyData> {
    let mut anomalies = Vec::new();

    let Some(period) = dominant_period else {
        return anomalies;
    };

    if period == 0 {
        return anomalies;
    }

    let glitch_threshold = period / 4; // Less than 25% of period = glitch
    let gap_threshold = period * 2; // More than 200% of period = gap

    let mut prev_time: Option<u64> = None;

    for &tt_idx in signal.time_indices() {
        let t = time_table[tt_idx as usize];
        if t < t0_ps {
            prev_time = Some(t);
            continue;
        }
        if t > t1_ps {
            break;
        }

        if let Some(pt) = prev_time {
            let interval = t - pt;
            if interval > 0 && interval < glitch_threshold {
                anomalies.push(AnomalyData {
                    time_ps: t,
                    kind: "glitch".to_string(),
                    detail: format!("interval={interval}ps, expected≈{period}ps"),
                });
            } else if interval > gap_threshold {
                anomalies.push(AnomalyData {
                    time_ps: t,
                    kind: "gap".to_string(),
                    detail: format!("gap={interval}ps, expected≈{period}ps"),
                });
            }
        }
        prev_time = Some(t);
    }

    // Check for stuck signal (no transitions for a long time at the end)
    if let Some(pt) = prev_time {
        let remaining = t1_ps.saturating_sub(pt);
        if remaining > gap_threshold && total_transitions_in_range(signal, time_table, t0_ps, t1_ps) > 2 {
            anomalies.push(AnomalyData {
                time_ps: pt,
                kind: "stuck".to_string(),
                detail: format!("no transitions for {remaining}ps after t={pt}"),
            });
        }
    }

    anomalies
}

fn total_transitions_in_range(
    signal: &wellen::Signal,
    time_table: &[Time],
    t0_ps: u64,
    t1_ps: u64,
) -> usize {
    signal
        .time_indices()
        .iter()
        .filter(|&&idx| {
            let t = time_table[idx as usize];
            t >= t0_ps && t <= t1_ps
        })
        .count()
}

fn summary_to_pydict<'py>(py: Python<'py>, data: &SummaryData) -> PyResult<Bound<'py, PyDict>> {
    let dict = PyDict::new(py);
    dict.set_item("total_transitions", data.total_transitions)?;
    dict.set_item("dominant_period_ps", data.dominant_period_ps)?;
    dict.set_item("duty_cycle", data.duty_cycle)?;

    let hist = PyDict::new(py);
    for (k, v) in &data.value_histogram {
        hist.set_item(k, *v)?;
    }
    dict.set_item("value_histogram", hist)?;

    let anomalies_list: Vec<Bound<'py, PyDict>> = data
        .anomalies
        .iter()
        .map(|a| {
            let d = PyDict::new(py);
            d.set_item("time_ps", a.time_ps)?;
            d.set_item("kind", &a.kind)?;
            d.set_item("detail", &a.detail)?;
            Ok(d)
        })
        .collect::<PyResult<Vec<_>>>()?;
    dict.set_item("anomalies", PyList::new(py, &anomalies_list)?)?;

    Ok(dict)
}

#[pyfunction]
pub fn summarize(
    py: Python<'_>,
    handle: &WaveformHandle,
    signal: &str,
    t0_ps: u64,
    t1_ps: u64,
) -> PyResult<PyObject> {
    handle.with_wave(|wave| {
        let data = summarize_signal(wave, signal, t0_ps, t1_ps, None)?;
        Ok(summary_to_pydict(py, &data)?.into())
    })
}

#[pyfunction]
pub fn summarize_window(
    py: Python<'_>,
    handle: &WaveformHandle,
    signals: Vec<String>,
    t0_ps: u64,
    t1_ps: u64,
) -> PyResult<PyObject> {
    handle.with_wave(|wave| {
        let result = PyDict::new(py);
        for sig_path in &signals {
            let data = summarize_signal(wave, sig_path, t0_ps, t1_ps, None)?;
            result.set_item(sig_path, summary_to_pydict(py, &data)?)?;
        }
        Ok(result.into())
    })
}

#[pyfunction]
#[pyo3(signature = (handle, signal, t0_ps, t1_ps, expected_period_ps=None))]
pub fn find_anomalies(
    py: Python<'_>,
    handle: &WaveformHandle,
    signal: &str,
    t0_ps: u64,
    t1_ps: u64,
    expected_period_ps: Option<u64>,
) -> PyResult<PyObject> {
    handle.with_wave(|wave| {
        let data = summarize_signal(wave, signal, t0_ps, t1_ps, expected_period_ps)?;
        let anomalies: Vec<Bound<'_, PyDict>> = data
            .anomalies
            .iter()
            .map(|a| {
                let d = PyDict::new(py);
                d.set_item("time_ps", a.time_ps)?;
                d.set_item("kind", &a.kind)?;
                d.set_item("detail", &a.detail)?;
                Ok(d)
            })
            .collect::<PyResult<Vec<_>>>()?;
        Ok(PyList::new(py, &anomalies)?.into())
    })
}
