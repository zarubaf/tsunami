use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};
use tsunami_core::predicate::{self, Expr};

use crate::py_query::PyWaveformHandle;

/// Convert a Python Expr dataclass (with .tag attribute) to a core Expr.
fn py_to_expr(ob: &Bound<'_, PyAny>) -> PyResult<Expr> {
    let tag: String = ob.getattr("tag")?.extract()?;
    match tag.as_str() {
        "signal" => {
            let path: String = ob.getattr("path")?.extract()?;
            Ok(Expr::Signal { path })
        }
        "const" => {
            let value: u64 = ob.getattr("value")?.extract()?;
            Ok(Expr::Const { value })
        }
        "and" => {
            let left = py_to_expr(&ob.getattr("left")?)?;
            let right = py_to_expr(&ob.getattr("right")?)?;
            Ok(Expr::And {
                left: Box::new(left),
                right: Box::new(right),
            })
        }
        "or" => {
            let left = py_to_expr(&ob.getattr("left")?)?;
            let right = py_to_expr(&ob.getattr("right")?)?;
            Ok(Expr::Or {
                left: Box::new(left),
                right: Box::new(right),
            })
        }
        "not" => {
            let inner = py_to_expr(&ob.getattr("inner")?)?;
            Ok(Expr::Not {
                inner: Box::new(inner),
            })
        }
        "xor" => {
            let left = py_to_expr(&ob.getattr("left")?)?;
            let right = py_to_expr(&ob.getattr("right")?)?;
            Ok(Expr::Xor {
                left: Box::new(left),
                right: Box::new(right),
            })
        }
        "eq" => {
            let left = py_to_expr(&ob.getattr("left")?)?;
            let right = py_to_expr(&ob.getattr("right")?)?;
            Ok(Expr::Eq {
                left: Box::new(left),
                right: Box::new(right),
            })
        }
        "gt" => {
            let left = py_to_expr(&ob.getattr("left")?)?;
            let right = py_to_expr(&ob.getattr("right")?)?;
            Ok(Expr::Gt {
                left: Box::new(left),
                right: Box::new(right),
            })
        }
        "lt" => {
            let left = py_to_expr(&ob.getattr("left")?)?;
            let right = py_to_expr(&ob.getattr("right")?)?;
            Ok(Expr::Lt {
                left: Box::new(left),
                right: Box::new(right),
            })
        }
        "rise" => {
            let inner = py_to_expr(&ob.getattr("inner")?)?;
            Ok(Expr::Rise {
                inner: Box::new(inner),
            })
        }
        "fall" => {
            let inner = py_to_expr(&ob.getattr("inner")?)?;
            Ok(Expr::Fall {
                inner: Box::new(inner),
            })
        }
        "bit_slice" => {
            let inner = py_to_expr(&ob.getattr("inner")?)?;
            let high: u32 = ob.getattr("high")?.extract()?;
            let low: u32 = ob.getattr("low")?.extract()?;
            Ok(Expr::BitSlice {
                inner: Box::new(inner),
                high,
                low,
            })
        }
        "sequence" => {
            let a = py_to_expr(&ob.getattr("a")?)?;
            let b = py_to_expr(&ob.getattr("b")?)?;
            let within_ps: Option<u64> = ob.getattr("within_ps")?.extract()?;
            Ok(Expr::Sequence {
                a: Box::new(a),
                b: Box::new(b),
                within_ps,
            })
        }
        "preceded_by" => {
            let a = py_to_expr(&ob.getattr("a")?)?;
            let b = py_to_expr(&ob.getattr("b")?)?;
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

fn err(e: String) -> PyErr {
    PyValueError::new_err(e)
}

#[pyfunction]
pub fn find_first(
    py: Python<'_>,
    handle: &PyWaveformHandle,
    expr: &Bound<'_, PyAny>,
    after_ps: u64,
) -> PyResult<Py<PyAny>> {
    let expr = py_to_expr(expr)?;
    let result = predicate::find_first(&handle.inner, &expr, after_ps).map_err(err)?;
    match result {
        Some(t) => Ok(t.into_pyobject(py)?.into_any().unbind()),
        None => Ok(py.None()),
    }
}

#[pyfunction]
pub fn find_all(
    py: Python<'_>,
    handle: &PyWaveformHandle,
    expr: &Bound<'_, PyAny>,
    t0_ps: u64,
    t1_ps: u64,
) -> PyResult<Py<PyAny>> {
    let expr = py_to_expr(expr)?;
    let times = predicate::find_all(&handle.inner, &expr, t0_ps, t1_ps).map_err(err)?;
    Ok(PyList::new(py, &times)?.into())
}

#[pyfunction]
pub fn scan(
    py: Python<'_>,
    handle: &PyWaveformHandle,
    expr: &Bound<'_, PyAny>,
    t0_ps: u64,
    t1_ps: u64,
) -> PyResult<Py<PyAny>> {
    let expr = py_to_expr(expr)?;
    let results = predicate::scan(&handle.inner, &expr, t0_ps, t1_ps).map_err(err)?;
    let py_results: Vec<Bound<'_, PyDict>> = results
        .iter()
        .map(|(t, v)| {
            let d = PyDict::new(py);
            d.set_item("time", *t)?;
            d.set_item("value", *v)?;
            Ok(d)
        })
        .collect::<PyResult<_>>()?;
    Ok(PyList::new(py, &py_results)?.into())
}
