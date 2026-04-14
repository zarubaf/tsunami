use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};
use tsunami_core::query;

/// Thin PyO3 wrapper around tsunami_core::WaveformHandle.
#[pyclass(name = "WaveformHandle")]
pub struct PyWaveformHandle {
    pub inner: query::WaveformHandle,
}

impl PyWaveformHandle {
    pub fn open(path: &str) -> PyResult<Self> {
        let inner = query::WaveformHandle::open(path)
            .map_err(|e| PyValueError::new_err(e))?;
        Ok(Self { inner })
    }
}

fn err(e: String) -> PyErr {
    PyValueError::new_err(e)
}

fn value_info_to_dict<'py>(py: Python<'py>, v: &query::ValueInfo) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    d.set_item("hex", &v.hex)?;
    d.set_item("is_x", v.is_x)?;
    d.set_item("is_z", v.is_z)?;
    Ok(d)
}

fn signal_info_to_dict<'py>(
    py: Python<'py>,
    s: &query::SignalInfo,
) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    d.set_item("path", &s.path)?;
    d.set_item("width", s.width)?;
    d.set_item("type", &s.var_type)?;
    d.set_item("direction", &s.direction)?;
    match &s.index {
        Some(idx) => d.set_item("index", idx)?,
        None => d.set_item("index", py.None())?,
    }
    Ok(d)
}

#[pyfunction]
pub fn waveform_info(py: Python<'_>, handle: &PyWaveformHandle) -> PyResult<Py<PyAny>> {
    let info = query::waveform_info(&handle.inner).map_err(err)?;
    let dict = PyDict::new(py);
    dict.set_item("timescale_factor", info.timescale_factor)?;
    dict.set_item("timescale_unit", &info.timescale_unit)?;
    dict.set_item("duration", info.duration)?;
    dict.set_item("num_signals", info.num_signals)?;
    dict.set_item("num_time_points", info.num_time_points)?;
    dict.set_item("file_format", &info.file_format)?;
    Ok(dict.into())
}

#[pyfunction]
pub fn get_waveform_length(py: Python<'_>, handle: &PyWaveformHandle) -> PyResult<Py<PyAny>> {
    let len = query::get_waveform_length(&handle.inner).map_err(err)?;
    let dict = PyDict::new(py);
    dict.set_item("start_time", len.start_time)?;
    dict.set_item("end_time", len.end_time)?;
    dict.set_item("time_steps", len.time_steps)?;
    match &len.timescale {
        Some(ts) => dict.set_item("timescale", ts)?,
        None => dict.set_item("timescale", py.None())?,
    }
    Ok(dict.into())
}

#[pyfunction]
#[pyo3(signature = (handle, pattern="*"))]
pub fn list_signals(py: Python<'_>, handle: &PyWaveformHandle, pattern: &str) -> PyResult<Py<PyAny>> {
    let results = query::list_signals(&handle.inner, pattern).map_err(err)?;
    let py_list: Vec<Bound<'_, PyDict>> = results
        .iter()
        .map(|s| signal_info_to_dict(py, s))
        .collect::<PyResult<_>>()?;
    Ok(PyList::new(py, &py_list)?.into())
}

#[pyfunction]
pub fn get_signal_info(py: Python<'_>, handle: &PyWaveformHandle, signal: &str) -> PyResult<Py<PyAny>> {
    let info = query::get_signal_info(&handle.inner, signal).map_err(err)?;
    Ok(signal_info_to_dict(py, &info)?.into())
}

#[pyfunction]
#[pyo3(signature = (handle, prefix=""))]
pub fn list_scopes(py: Python<'_>, handle: &PyWaveformHandle, prefix: &str) -> PyResult<Py<PyAny>> {
    let results = query::list_scopes(&handle.inner, prefix).map_err(err)?;
    Ok(PyList::new(py, &results)?.into())
}

#[pyfunction]
pub fn get_value(
    py: Python<'_>,
    handle: &PyWaveformHandle,
    signal: &str,
    time_ps: u64,
) -> PyResult<Py<PyAny>> {
    let info = query::get_value(&handle.inner, signal, time_ps).map_err(err)?;
    Ok(value_info_to_dict(py, &info)?.into())
}

#[pyfunction]
pub fn get_snapshot(
    py: Python<'_>,
    handle: &PyWaveformHandle,
    signals: Vec<String>,
    time_ps: u64,
) -> PyResult<Py<PyAny>> {
    let result = query::get_snapshot(&handle.inner, &signals, time_ps).map_err(err)?;
    let dict = PyDict::new(py);
    for (path, val) in &result {
        dict.set_item(path, value_info_to_dict(py, val)?)?;
    }
    Ok(dict.into())
}

#[pyfunction]
#[pyo3(signature = (handle, signal, t0_ps, t1_ps, max_edges=1000))]
pub fn get_transitions(
    py: Python<'_>,
    handle: &PyWaveformHandle,
    signal: &str,
    t0_ps: u64,
    t1_ps: u64,
    max_edges: usize,
) -> PyResult<Py<PyAny>> {
    let tr = query::get_transitions(&handle.inner, signal, t0_ps, t1_ps, max_edges).map_err(err)?;
    let transitions: Vec<Bound<'_, PyDict>> = tr
        .transitions
        .iter()
        .map(|t| {
            let d = PyDict::new(py);
            d.set_item("time", t.time)?;
            d.set_item("value", &t.value)?;
            Ok(d)
        })
        .collect::<PyResult<_>>()?;

    let result = PyDict::new(py);
    result.set_item("signal", &tr.signal)?;
    result.set_item("t0_ps", tr.t0_ps)?;
    result.set_item("t1_ps", tr.t1_ps)?;
    result.set_item("total_transitions", tr.total_transitions)?;
    result.set_item("truncated", tr.truncated)?;
    result.set_item("transitions", PyList::new(py, &transitions)?)?;
    Ok(result.into())
}

#[pyfunction]
pub fn find_next_edge(
    py: Python<'_>,
    handle: &PyWaveformHandle,
    signal: &str,
    direction: &str,
    after_ps: u64,
) -> PyResult<Py<PyAny>> {
    let matches = query::find_edges(
        &handle.inner,
        signal,
        direction,
        after_ps.saturating_add(1),
        None,
        1,
    )
    .map_err(err)?;

    if let Some(&t) = matches.first() {
        Ok(t.into_pyobject(py)?.into_any().unbind())
    } else {
        Ok(py.None())
    }
}

#[pyfunction]
#[pyo3(signature = (handle, signal, direction, start_ps=0, end_ps=None, limit=50))]
pub fn find_edges(
    py: Python<'_>,
    handle: &PyWaveformHandle,
    signal: &str,
    direction: &str,
    start_ps: u64,
    end_ps: Option<u64>,
    limit: usize,
) -> PyResult<Py<PyAny>> {
    let matches =
        query::find_edges(&handle.inner, signal, direction, start_ps, end_ps, limit).map_err(err)?;
    Ok(PyList::new(py, matches)?.into())
}

#[pyfunction]
#[pyo3(signature = (handle, signal, condition, value, start_ps=0, end_ps=None, limit=50))]
pub fn find_value(
    py: Python<'_>,
    handle: &PyWaveformHandle,
    signal: &str,
    condition: &str,
    value: &str,
    start_ps: u64,
    end_ps: Option<u64>,
    limit: usize,
) -> PyResult<Py<PyAny>> {
    let matches = query::find_value(&handle.inner, signal, condition, value, start_ps, end_ps, limit)
        .map_err(err)?;
    let py_matches: Vec<Bound<'_, PyDict>> = matches
        .iter()
        .map(|m| {
            let d = PyDict::new(py);
            d.set_item("start", m.start)?;
            d.set_item("end", m.end)?;
            d.set_item("value", &m.value)?;
            Ok(d)
        })
        .collect::<PyResult<_>>()?;
    Ok(PyList::new(py, &py_matches)?.into())
}
