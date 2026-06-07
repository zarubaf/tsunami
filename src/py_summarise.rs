use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};
use tsunami_core::summarise as core_summarise;

use crate::py_query::PyWaveformHandle;

fn err(e: String) -> PyErr {
    PyValueError::new_err(e)
}

fn summary_to_pydict<'py>(
    py: Python<'py>,
    data: &core_summarise::SummaryData,
) -> PyResult<Bound<'py, PyDict>> {
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
    handle: &PyWaveformHandle,
    signal: &str,
    t0_ps: u64,
    t1_ps: u64,
) -> PyResult<Py<PyAny>> {
    let data = core_summarise::summarize(&handle.inner, signal, t0_ps, t1_ps).map_err(err)?;
    Ok(summary_to_pydict(py, &data)?.into())
}

#[pyfunction]
pub fn summarize_window(
    py: Python<'_>,
    handle: &PyWaveformHandle,
    signals: Vec<String>,
    t0_ps: u64,
    t1_ps: u64,
) -> PyResult<Py<PyAny>> {
    let data =
        core_summarise::summarize_window(&handle.inner, &signals, t0_ps, t1_ps).map_err(err)?;
    let result = PyDict::new(py);
    for (sig_path, summary) in &data {
        result.set_item(sig_path, summary_to_pydict(py, summary)?)?;
    }
    Ok(result.into())
}

#[pyfunction]
#[pyo3(signature = (handle, signal, t0_ps, t1_ps, expected_period_ps=None))]
pub fn find_anomalies(
    py: Python<'_>,
    handle: &PyWaveformHandle,
    signal: &str,
    t0_ps: u64,
    t1_ps: u64,
    expected_period_ps: Option<u64>,
) -> PyResult<Py<PyAny>> {
    let anomalies = core_summarise::find_anomalies(
        &handle.inner,
        signal,
        t0_ps,
        t1_ps,
        expected_period_ps,
    )
    .map_err(err)?;

    let py_anomalies: Vec<Bound<'_, PyDict>> = anomalies
        .iter()
        .map(|a| {
            let d = PyDict::new(py);
            d.set_item("time_ps", a.time_ps)?;
            d.set_item("kind", &a.kind)?;
            d.set_item("detail", &a.detail)?;
            Ok(d)
        })
        .collect::<PyResult<Vec<_>>>()?;
    Ok(PyList::new(py, &py_anomalies)?.into())
}
