use pyo3::prelude::*;

mod query;
mod predicate;
mod summarise;

use query::{
    WaveformHandle, waveform_info, list_signals, list_scopes,
    get_value, get_snapshot, get_transitions, find_next_edge,
};
use predicate::{find_first, find_all, scan};
use summarise::{summarize, summarize_window, find_anomalies};

#[pymodule]
fn _engine(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<WaveformHandle>()?;
    m.add_function(wrap_pyfunction!(open, m)?)?;
    m.add_function(wrap_pyfunction!(waveform_info, m)?)?;
    m.add_function(wrap_pyfunction!(list_signals, m)?)?;
    m.add_function(wrap_pyfunction!(list_scopes, m)?)?;
    m.add_function(wrap_pyfunction!(get_value, m)?)?;
    m.add_function(wrap_pyfunction!(get_snapshot, m)?)?;
    m.add_function(wrap_pyfunction!(get_transitions, m)?)?;
    m.add_function(wrap_pyfunction!(find_next_edge, m)?)?;
    m.add_function(wrap_pyfunction!(find_first, m)?)?;
    m.add_function(wrap_pyfunction!(find_all, m)?)?;
    m.add_function(wrap_pyfunction!(scan, m)?)?;
    m.add_function(wrap_pyfunction!(summarize, m)?)?;
    m.add_function(wrap_pyfunction!(summarize_window, m)?)?;
    m.add_function(wrap_pyfunction!(find_anomalies, m)?)?;
    Ok(())
}

#[pyfunction]
fn open(path: &str) -> PyResult<WaveformHandle> {
    WaveformHandle::open(path)
}
