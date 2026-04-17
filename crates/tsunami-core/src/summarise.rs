use serde::Serialize;
use std::collections::HashMap;
use wellen::{GetItem, Time, Var};

use crate::query::{
    get_signal_value_at_idx, resolve_signal, signal_value_to_hex, signal_value_to_u64,
    WaveformHandle,
};

#[derive(Debug, Clone, Serialize)]
pub struct AnomalyData {
    pub time_ps: u64,
    pub kind: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SummaryData {
    pub total_transitions: usize,
    pub dominant_period_ps: Option<u64>,
    pub duty_cycle: Option<f64>,
    pub value_histogram: HashMap<String, usize>,
    pub anomalies: Vec<AnomalyData>,
}

/// Compute the dominant period from a list of intervals using a histogram approach.
fn compute_dominant_period(intervals: &[u64]) -> Option<u64> {
    if intervals.is_empty() {
        return None;
    }

    let mut buckets: HashMap<u64, usize> = HashMap::new();
    for &interval in intervals {
        let bucket = if interval == 0 {
            0
        } else {
            let bucket_size = std::cmp::max(1, interval / 100);
            (interval / bucket_size) * bucket_size
        };
        *buckets.entry(bucket).or_insert(0) += 1;
    }

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

    let glitch_threshold = period / 4;
    let gap_threshold = period * 2;

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

    if let Some(pt) = prev_time {
        let remaining = t1_ps.saturating_sub(pt);
        if remaining > gap_threshold
            && total_transitions_in_range(signal, time_table, t0_ps, t1_ps) > 2
        {
            anomalies.push(AnomalyData {
                time_ps: pt,
                kind: "stuck".to_string(),
                detail: format!("no transitions for {remaining}ps after t={pt}"),
            });
        }
    }

    anomalies
}

pub fn summarize_signal(
    wave: &mut wellen::simple::Waveform,
    signal_path: &str,
    t0_ps: u64,
    t1_ps: u64,
    expected_period_ps: Option<u64>,
) -> Result<SummaryData, String> {
    let hier = wave.hierarchy();
    let var_ref = resolve_signal(hier, signal_path)?;
    let var: &Var = hier.get(var_ref);
    let sig_ref = var.signal_ref();
    let is_1bit = var.length().unwrap_or(1) == 1;

    wave.load_signals(&[sig_ref]);

    let time_table = wave.time_table();
    let signal = wave
        .get_signal(sig_ref)
        .ok_or_else(|| "Failed to load signal data".to_string())?;

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
            let hex = signal_value_to_hex(&val);
            *value_histogram.entry(hex).or_insert(0) += 1;
        }
    }

    let dominant_period = expected_period_ps.or_else(|| compute_dominant_period(&intervals));
    let duty_cycle = if is_1bit {
        compute_duty_cycle(signal, time_table, t0_ps, t1_ps)
    } else {
        None
    };

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

pub fn summarize(
    handle: &WaveformHandle,
    signal: &str,
    t0_ps: u64,
    t1_ps: u64,
) -> Result<SummaryData, String> {
    handle.with_wave(|wave| summarize_signal(wave, signal, t0_ps, t1_ps, None))
}

pub fn summarize_window(
    handle: &WaveformHandle,
    signals: &[String],
    t0_ps: u64,
    t1_ps: u64,
) -> Result<HashMap<String, SummaryData>, String> {
    handle.with_wave(|wave| {
        let mut result = HashMap::new();
        for sig_path in signals {
            let data = summarize_signal(wave, sig_path, t0_ps, t1_ps, None)?;
            result.insert(sig_path.clone(), data);
        }
        Ok(result)
    })
}

pub fn find_anomalies(
    handle: &WaveformHandle,
    signal: &str,
    t0_ps: u64,
    t1_ps: u64,
    expected_period_ps: Option<u64>,
) -> Result<Vec<AnomalyData>, String> {
    handle.with_wave(|wave| {
        let data = summarize_signal(wave, signal, t0_ps, t1_ps, expected_period_ps)?;
        Ok(data.anomalies)
    })
}
