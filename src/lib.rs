use pyo3::prelude::*;

mod py_query;
mod py_predicate;
mod py_summarise;

use py_query::{
    PyWaveformHandle, waveform_info, get_waveform_length, list_signals, list_scopes,
    get_signal_info, get_value, get_snapshot, get_transitions, find_next_edge, find_edges,
    find_value,
};
use py_predicate::{find_first, find_all, scan};
use py_summarise::{summarize, summarize_window, find_anomalies};

#[pymodule]
fn _engine(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyWaveformHandle>()?;
    m.add_function(wrap_pyfunction!(open, m)?)?;
    m.add_function(wrap_pyfunction!(waveform_info, m)?)?;
    m.add_function(wrap_pyfunction!(get_waveform_length, m)?)?;
    m.add_function(wrap_pyfunction!(list_signals, m)?)?;
    m.add_function(wrap_pyfunction!(list_scopes, m)?)?;
    m.add_function(wrap_pyfunction!(get_signal_info, m)?)?;
    m.add_function(wrap_pyfunction!(get_value, m)?)?;
    m.add_function(wrap_pyfunction!(get_snapshot, m)?)?;
    m.add_function(wrap_pyfunction!(get_transitions, m)?)?;
    m.add_function(wrap_pyfunction!(find_next_edge, m)?)?;
    m.add_function(wrap_pyfunction!(find_edges, m)?)?;
    m.add_function(wrap_pyfunction!(find_value, m)?)?;
    m.add_function(wrap_pyfunction!(find_first, m)?)?;
    m.add_function(wrap_pyfunction!(find_all, m)?)?;
    m.add_function(wrap_pyfunction!(scan, m)?)?;
    m.add_function(wrap_pyfunction!(summarize, m)?)?;
    m.add_function(wrap_pyfunction!(summarize_window, m)?)?;
    m.add_function(wrap_pyfunction!(find_anomalies, m)?)?;
    Ok(())
}

#[pyfunction]
fn open(path: &str) -> PyResult<PyWaveformHandle> {
    PyWaveformHandle::open(path)
}
