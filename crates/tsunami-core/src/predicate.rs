use serde::Deserialize;
use std::collections::{BTreeSet, HashMap};
use wellen::{GetItem, SignalRef, Time, Var};

use crate::query::{
    get_signal_value_at_idx, resolve_signal, signal_value_to_u64, WaveformHandle,
};

/// Expression AST for predicate evaluation.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "tag")]
pub enum Expr {
    #[serde(rename = "signal")]
    Signal { path: String },
    #[serde(rename = "const")]
    Const { value: u64 },
    #[serde(rename = "and")]
    And {
        left: Box<Expr>,
        right: Box<Expr>,
    },
    #[serde(rename = "or")]
    Or {
        left: Box<Expr>,
        right: Box<Expr>,
    },
    #[serde(rename = "not")]
    Not { inner: Box<Expr> },
    #[serde(rename = "xor")]
    Xor {
        left: Box<Expr>,
        right: Box<Expr>,
    },
    #[serde(rename = "eq")]
    Eq {
        left: Box<Expr>,
        right: Box<Expr>,
    },
    #[serde(rename = "gt")]
    Gt {
        left: Box<Expr>,
        right: Box<Expr>,
    },
    #[serde(rename = "lt")]
    Lt {
        left: Box<Expr>,
        right: Box<Expr>,
    },
    #[serde(rename = "rise")]
    Rise { inner: Box<Expr> },
    #[serde(rename = "fall")]
    Fall { inner: Box<Expr> },
    #[serde(rename = "bit_slice")]
    BitSlice {
        inner: Box<Expr>,
        high: u32,
        low: u32,
    },
    #[serde(rename = "sequence")]
    Sequence {
        a: Box<Expr>,
        b: Box<Expr>,
        within_ps: Option<u64>,
    },
    #[serde(rename = "preceded_by")]
    PrecededBy {
        a: Box<Expr>,
        b: Box<Expr>,
        within_ps: Option<u64>,
    },
}

/// Collect all signal paths referenced in an expression.
fn collect_signals(expr: &Expr, out: &mut Vec<String>) {
    match expr {
        Expr::Signal { path } => {
            if !out.contains(path) {
                out.push(path.clone());
            }
        }
        Expr::Const { .. } => {}
        Expr::And { left, right }
        | Expr::Or { left, right }
        | Expr::Xor { left, right }
        | Expr::Eq { left, right }
        | Expr::Gt { left, right }
        | Expr::Lt { left, right } => {
            collect_signals(left, out);
            collect_signals(right, out);
        }
        Expr::Not { inner } | Expr::Rise { inner } | Expr::Fall { inner } | Expr::BitSlice { inner, .. } => {
            collect_signals(inner, out);
        }
        Expr::Sequence { a, b, .. } | Expr::PrecededBy { a, b, .. } => {
            collect_signals(a, out);
            collect_signals(b, out);
        }
    }
}

/// Evaluate a non-temporal expression at a given time table index.
fn eval_at(
    expr: &Expr,
    tt_idx: u32,
    prev_tt_idx: Option<u32>,
    signals: &HashMap<String, &wellen::Signal>,
) -> Option<u64> {
    match expr {
        Expr::Signal { path } => {
            let sig = signals.get(path.as_str())?;
            let val = get_signal_value_at_idx(sig, tt_idx)?;
            signal_value_to_u64(&val)
        }
        Expr::Const { value } => Some(*value),
        Expr::And { left, right } => {
            let va = eval_at(left, tt_idx, prev_tt_idx, signals)?;
            let vb = eval_at(right, tt_idx, prev_tt_idx, signals)?;
            Some(if va != 0 && vb != 0 { 1 } else { 0 })
        }
        Expr::Or { left, right } => {
            let va = eval_at(left, tt_idx, prev_tt_idx, signals)?;
            let vb = eval_at(right, tt_idx, prev_tt_idx, signals)?;
            Some(if va != 0 || vb != 0 { 1 } else { 0 })
        }
        Expr::Not { inner } => {
            let va = eval_at(inner, tt_idx, prev_tt_idx, signals)?;
            Some(if va == 0 { 1 } else { 0 })
        }
        Expr::Xor { left, right } => {
            let va = eval_at(left, tt_idx, prev_tt_idx, signals)?;
            let vb = eval_at(right, tt_idx, prev_tt_idx, signals)?;
            Some(if (va != 0) ^ (vb != 0) { 1 } else { 0 })
        }
        Expr::Eq { left, right } => {
            let va = eval_at(left, tt_idx, prev_tt_idx, signals)?;
            let vb = eval_at(right, tt_idx, prev_tt_idx, signals)?;
            Some(if va == vb { 1 } else { 0 })
        }
        Expr::Gt { left, right } => {
            let va = eval_at(left, tt_idx, prev_tt_idx, signals)?;
            let vb = eval_at(right, tt_idx, prev_tt_idx, signals)?;
            Some(if va > vb { 1 } else { 0 })
        }
        Expr::Lt { left, right } => {
            let va = eval_at(left, tt_idx, prev_tt_idx, signals)?;
            let vb = eval_at(right, tt_idx, prev_tt_idx, signals)?;
            Some(if va < vb { 1 } else { 0 })
        }
        Expr::Rise { inner } => {
            let prev = prev_tt_idx?;
            let curr_val = eval_at(inner, tt_idx, None, signals)?;
            let prev_val = eval_at(inner, prev, None, signals)?;
            Some(if prev_val == 0 && curr_val != 0 { 1 } else { 0 })
        }
        Expr::Fall { inner } => {
            let prev = prev_tt_idx?;
            let curr_val = eval_at(inner, tt_idx, None, signals)?;
            let prev_val = eval_at(inner, prev, None, signals)?;
            Some(if prev_val != 0 && curr_val == 0 { 1 } else { 0 })
        }
        Expr::BitSlice { inner, high, low } => {
            let val = eval_at(inner, tt_idx, prev_tt_idx, signals)?;
            let width = high - low + 1;
            let mask = if width >= 64 {
                u64::MAX
            } else {
                (1u64 << width) - 1
            };
            Some((val >> low) & mask)
        }
        Expr::Sequence { .. } | Expr::PrecededBy { .. } => None,
    }
}

/// Check if an expression is temporal (contains Sequence or PrecededBy).
fn is_temporal(expr: &Expr) -> bool {
    match expr {
        Expr::Sequence { .. } | Expr::PrecededBy { .. } => true,
        Expr::And { left, right }
        | Expr::Or { left, right }
        | Expr::Xor { left, right }
        | Expr::Eq { left, right }
        | Expr::Gt { left, right }
        | Expr::Lt { left, right } => is_temporal(left) || is_temporal(right),
        Expr::Not { inner } | Expr::Rise { inner } | Expr::Fall { inner } | Expr::BitSlice { inner, .. } => {
            is_temporal(inner)
        }
        Expr::Signal { .. } | Expr::Const { .. } => false,
    }
}

/// Core scan: find all time points in [t0, t1] where expr evaluates to true.
pub fn scan_expr(
    wave: &mut wellen::simple::Waveform,
    expr: &Expr,
    t0_ps: u64,
    t1_ps: u64,
) -> Result<Vec<(u64, u64)>, String> {
    let mut signal_paths = Vec::new();
    collect_signals(expr, &mut signal_paths);

    let hier = wave.hierarchy();

    let mut sig_refs_map: HashMap<String, SignalRef> = HashMap::new();
    let mut sig_refs: Vec<SignalRef> = Vec::new();

    for path in &signal_paths {
        let var_ref = resolve_signal(hier, path)?;
        let var: &Var = hier.get(var_ref);
        let sr = var.signal_ref();
        sig_refs_map.insert(path.clone(), sr);
        if !sig_refs.contains(&sr) {
            sig_refs.push(sr);
        }
    }

    wave.load_signals(&sig_refs);

    let time_table = wave.time_table();

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

    let signal_map: HashMap<String, &wellen::Signal> = sig_refs_map
        .iter()
        .filter_map(|(path, sr)| wave.get_signal(*sr).map(|sig| (path.clone(), sig)))
        .collect();

    if is_temporal(expr) {
        let results = eval_temporal(expr, &transition_indices, time_table, &signal_map)?;
        wave.unload_signals(&sig_refs);
        return Ok(results);
    }

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
) -> Result<Vec<(u64, u64)>, String> {
    match expr {
        Expr::Sequence { a, b, within_ps } => {
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
            let indices: Vec<u32> = transition_indices.iter().copied().collect();
            let mut results = Vec::new();

            for (i, &tt_idx) in indices.iter().enumerate() {
                let t = time_table[tt_idx as usize];
                let prev_idx = if i > 0 { Some(indices[i - 1]) } else { None };

                let a_val = eval_at(a, tt_idx, prev_idx, signals);
                if a_val != Some(1) && a_val.map_or(true, |v| v == 0) {
                    continue;
                }

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

pub fn find_first(
    handle: &WaveformHandle,
    expr: &Expr,
    after_ps: u64,
) -> Result<Option<u64>, String> {
    handle.with_wave(|wave| {
        let time_table = wave.time_table();
        let t1 = time_table.last().copied().unwrap_or(0);
        let results = scan_expr(wave, expr, after_ps, t1)?;
        Ok(results.first().map(|(t, _)| *t))
    })
}

pub fn find_all(
    handle: &WaveformHandle,
    expr: &Expr,
    t0_ps: u64,
    t1_ps: u64,
) -> Result<Vec<u64>, String> {
    handle.with_wave(|wave| {
        let results = scan_expr(wave, expr, t0_ps, t1_ps)?;
        Ok(results.iter().map(|(t, _)| *t).collect())
    })
}

pub fn scan(
    handle: &WaveformHandle,
    expr: &Expr,
    t0_ps: u64,
    t1_ps: u64,
) -> Result<Vec<(u64, u64)>, String> {
    handle.with_wave(|wave| scan_expr(wave, expr, t0_ps, t1_ps))
}
