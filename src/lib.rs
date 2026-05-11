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

#[pymodule]
fn bashe(m: &Bound<'_, PyModule>) -> PyResult<()> {
    register_types(m)?;
    m.add_class::<Bashe>()?;
    Ok(())
}
