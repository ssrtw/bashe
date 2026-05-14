mod ast;
mod translate;

use pyo3::prelude::*;
use pyo3::types::PyList;
use tree_sitter::Parser;

use ast::register_types;
use translate::translate_root;

#[pyclass(name = "Bashe")]
struct Bashe {
    parser: Parser,
}

#[pymethods]
impl Bashe {
    #[new]
    fn new() -> PyResult<Self> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_php::LANGUAGE_PHP.into())
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{e}")))?;
        Ok(Self { parser })
    }

    #[pyo3(signature = (code, filename=None))]
    fn parse(
        &mut self,
        py: Python<'_>,
        code: &str,
        filename: Option<String>,
    ) -> PyResult<Py<PyAny>> {
        let bytes = code.as_bytes();
        let tree = self
            .parser
            .parse(bytes, None)
            .ok_or_else(|| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>("parse failed"))?;
        let root = tree.root_node();
        let result = translate_root(root, bytes, py, filename)?;
        match result.cast_bound::<PyList>(py) {
            Ok(list) => Ok(list.clone().into()),
            Err(_) => {
                let list = PyList::empty(py);
                list.append(result)?;
                Ok(list.into())
            }
        }
    }
}

#[pyfunction]
fn fixup_curly_offsets(py: Python<'_>, nodes: &Bound<'_, PyList>) -> PyResult<Py<PyAny>> {
    use pyo3::types::PyAny;
    let mut result = Vec::new();
    let list: Vec<Py<PyAny>> = nodes.iter().map(|x| x.into()).collect();
    let mut i = 0;
    while i < list.len() {
        let mut cur = list[i].clone_ref(py);
        i += 1;
        while i < list.len() {
            let nxt = list[i].bind(py);
            let cls_name = nxt
                .getattr("__class__")
                .and_then(|c| c.getattr("__name__"))
                .and_then(|n| n.extract::<String>())
                .unwrap_or_default();
            if cls_name != "Block" {
                break;
            }
            let inner: Option<Py<PyAny>> = nxt
                .getattr("nodes")
                .and_then(|nl: Bound<'_, PyAny>| {
                    let seq = nl.cast::<PyList>()?;
                    Ok(if seq.len() == 1 {
                        Some(seq.get_item(0)?.into())
                    } else {
                        None
                    })
                })
                .ok()
                .flatten();
            if let Some(expr) = inner {
                cur = Py::new(
                    py,
                    ast::ArrayOffset {
                        lineno: None,
                        node: cur,
                        expr,
                    },
                )?
                .into_any();
                i += 1;
            } else {
                break;
            }
        }
        result.push(cur);
    }
    let out = PyList::new(py, &result)?;
    Ok(out.into_any().unbind())
}

#[pymodule]
fn bashe(m: &Bound<'_, PyModule>) -> PyResult<()> {
    register_types(m)?;
    m.add_class::<Bashe>()?;
    m.add_function(pyo3::wrap_pyfunction!(fixup_curly_offsets, m)?)?;
    Ok(())
}
