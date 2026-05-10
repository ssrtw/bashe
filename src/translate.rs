use pyo3::conversion::IntoPyObjectExt;
use pyo3::prelude::*;
use pyo3::types::PyList;
use pyo3::Py;
use tree_sitter::Node;

use crate::ast::*;

// ── helpers ──────────────────────────────────────────────────────────

fn lineno(node: &Node) -> Option<usize> {
    Some(node.start_position().row + 1)
}

fn text_of<'a>(node: &Node, source: &'a [u8]) -> &'a str {
    node.utf8_text(source).unwrap_or("")
}

fn field_child<'a>(node: &Node<'a>, field: &str) -> Option<Node<'a>> {
    node.child_by_field_name(field)
}

#[allow(dead_code)]
fn parse_int(text: &str) -> i64 {
    let t = text.replace('_', "").to_lowercase();
    if let Some(stripped) = t.strip_prefix("0x") {
        return i64::from_str_radix(stripped, 16).unwrap_or(0);
    }
    if let Some(stripped) = t.strip_prefix("0b") {
        return i64::from_str_radix(stripped, 2).unwrap_or(0);
    }
    if let Some(stripped) = t.strip_prefix("0o") {
        return i64::from_str_radix(stripped, 8).unwrap_or(0);
    }
    if t.starts_with("0") && t.len() > 1 && !t.contains('.') && !t.contains('e') {
        if let Ok(n) = i64::from_str_radix(&t, 8) {
            return n;
        }
    }
    t.parse().unwrap_or(0)
}

fn make_int(py: Python<'_>, n: i64) -> Py<PyAny> {
    n.into_pyobject(py).unwrap().into_any().unbind()
}

fn make_str(py: Python<'_>, s: &str) -> Py<PyAny> {
    s.into_pyobject(py).unwrap().into_any().unbind()
}

fn make_bool(py: Python<'_>, v: bool) -> Py<PyAny> {
    v.into_py_any(py).unwrap()
}

fn make_none(py: Python<'_>) -> Py<PyAny> {
    py.None()
}

const SKIP_TYPES: &[&str] = &[
    "?>",
    "<?php",
    "{",
    "}",
    "namespace",
    ":",
    "endif",
    "endforeach",
    "enddeclare",
    "endfor",
    "endswitch",
    "endwhile",
    "namespace_name",
];

fn is_skip(kind: &str) -> bool {
    SKIP_TYPES.contains(&kind)
}

// ── main translate ───────────────────────────────────────────────────

pub fn translate_root(root: Node, source: &[u8], py: Python<'_>) -> PyResult<Py<PyAny>> {
    let list = PyList::empty(py);
    let mut cursor = root.walk();
    for child in root.named_children(&mut cursor) {
        if let Some(obj) = translate(child, source, py)? {
            list.append(obj)?;
        }
    }
    Ok(list.into())
}

fn translate(node: Node, source: &[u8], py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
    let kind = node.kind();
    if is_skip(kind) {
        return Ok(None);
    }
    let lno = lineno(&node);

    let result: Option<Py<PyAny>> = match kind {
        // ── program / namespace / compound ──────────────────────
        "program" | "namespace_definition" | "declaration_list" => {
            let list = PyList::empty(py);
            if kind == "namespace_definition" {
                let name_node = field_child(&node, "name");
                let _ns_name = name_node.map(|n| text_of(&n, source).to_string());
                // TODO: handle namespace wrapping
            }
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                if is_skip(child.kind()) {
                    continue;
                }
                if let Some(obj) = translate(child, source, py)? {
                    if let Ok(seq) = obj.cast_bound::<PyList>(py) {
                        for i in 0..seq.len() {
                            list.append(seq.get_item(i)?)?;
                        }
                    } else {
                        list.append(obj)?;
                    }
                }
            }
            if kind == "namespace_definition" {
                // Simplify: just return inner block
            }
            Some(list.into())
        }
        "compound_statement" | "colon_block" => {
            let list = PyList::empty(py);
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                if child.kind() == "{" || child.kind() == "}" {
                    continue;
                }
                if let Some(obj) = translate(child, source, py)? {
                    if let Ok(seq) = obj.cast_bound::<PyList>(py) {
                        for i in 0..seq.len() {
                            list.append(seq.get_item(i)?)?;
                        }
                    } else {
                        list.append(obj)?;
                    }
                }
            }
            let block = Py::new(
                py,
                Block {
                    lineno: lno,
                    nodes: list.into(),
                },
            )?;
            Some(block.into_any())
        }
        "expression_statement" => node
            .named_child(0)
            .and_then(|c| translate(c, source, py).transpose())
            .transpose()?,

        // ── literals ─────────────────────────────────────────────
        "integer" => {
            let n = parse_int(text_of(&node, source));
            Some(make_int(py, n))
        }
        "float" => {
            let text = text_of(&node, source);
            let n: f64 = text.parse().unwrap_or(0.0);
            Some(n.into_pyobject(py).unwrap().into_any().unbind())
        }
        "string" | "encapsed_string" | "heredoc" | "nowdoc" => {
            let text = text_of(&node, source);
            Some(make_str(py, text))
        }
        "variable_name" => {
            let name = text_of(&node, source);
            let v = Py::new(
                py,
                Variable {
                    lineno: lno,
                    name: make_str(py, name),
                },
            )?;
            Some(v.into_any())
        }
        "variable_variable" => {
            let inner = node
                .named_child(0)
                .and_then(|c| translate(c, source, py).ok().flatten())
                .unwrap_or_else(|| make_none(py));
            let v = Py::new(
                py,
                Variable {
                    lineno: lno,
                    name: inner,
                },
            )?;
            Some(v.into_any())
        }
        "name" | "qualified_name" | "fully_qualified_name" | "relative_name" => {
            let txt = text_of(&node, source);
            let lower = txt.to_lowercase();
            if lower == "static" || lower == "self" || lower == "parent" {
                Some(make_str(py, &lower))
            } else {
                let c = Py::new(
                    py,
                    Constant {
                        lineno: lno,
                        name: make_str(py, txt),
                    },
                )?;
                Some(c.into_any())
            }
        }
        "array_creation_expression" => {
            let elements = PyList::empty(py);
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                if child.kind() != "array_element_initializer" {
                    continue;
                }
                let key_node = field_child(&child, "key");
                let val_node = field_child(&child, "value");
                let is_ref_val = text_of(&child, source).contains("&");
                let key = key_node.and_then(|n| translate(n, source, py).ok().flatten());
                let val = val_node
                    .and_then(|n| translate(n, source, py).ok().flatten())
                    .or_else(|| {
                        child
                            .named_child(0)
                            .and_then(|c| translate(c, source, py).ok().flatten())
                    });
                let ae = Py::new(
                    py,
                    ArrayElement {
                        lineno: lineno(&child),
                        key: key.unwrap_or_else(|| make_none(py)),
                        value: val.unwrap_or_else(|| make_none(py)),
                        is_ref: make_bool(py, is_ref_val),
                    },
                )?;
                elements.append(ae.into_any())?;
            }
            let arr = Py::new(
                py,
                Array {
                    lineno: lno,
                    nodes: elements.into(),
                },
            )?;
            Some(arr.into_any())
        }
        "list_literal" => {
            let items = PyList::empty(py);
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                if let Some(obj) = translate(child, source, py)? {
                    items.append(obj)?;
                }
            }
            Some(items.into())
        }

        // ── operators ───────────────────────────────────────────
        "assignment_expression" | "augmented_assignment_expression" => {
            let left_node = field_child(&node, "left");
            let right_node = field_child(&node, "right");
            let left = left_node.and_then(|n| translate(n, source, py).ok().flatten());
            let right = right_node.and_then(|n| translate(n, source, py).ok().flatten());
            let op_text = text_of(&node, source);
            let op = if op_text.contains("=&") {
                "="
            } else if op_text.contains("=") {
                op_text.split('=').next().unwrap_or("=").trim()
            } else {
                "="
            };
            let has_amp = op_text.contains("&");
            let left_unwrap = left.unwrap_or_else(|| make_none(py));
            let right_unwrap = right.unwrap_or_else(|| make_none(py));
            if op == "=" && !has_amp {
                let a = Py::new(
                    py,
                    Assignment {
                        lineno: lno,
                        node: left_unwrap,
                        expr: right_unwrap,
                        is_ref: make_bool(py, false),
                    },
                )?;
                Some(a.into_any())
            } else if has_amp && op == "=" {
                let a = Py::new(
                    py,
                    Assignment {
                        lineno: lno,
                        node: left_unwrap,
                        expr: right_unwrap,
                        is_ref: make_bool(py, true),
                    },
                )?;
                Some(a.into_any())
            } else {
                let a = Py::new(
                    py,
                    AssignOp {
                        lineno: lno,
                        op: make_str(py, &format!("{op}=")),
                        left: left_unwrap,
                        right: right_unwrap,
                    },
                )?;
                Some(a.into_any())
            }
        }
        "binary_expression" => {
            let left = field_child(&node, "left")
                .and_then(|n| translate(n, source, py).ok().flatten())
                .unwrap_or_else(|| make_none(py));
            let right = field_child(&node, "right")
                .and_then(|n| translate(n, source, py).ok().flatten())
                .unwrap_or_else(|| make_none(py));
            let mut op = String::new();
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if !child.is_named() {
                    op.push_str(text_of(&child, source));
                }
            }
            let op = op.trim().to_string();
            let op_final = if op == "instanceof" {
                op
            } else {
                op.to_lowercase()
            };
            let b = Py::new(
                py,
                BinaryOp {
                    lineno: lno,
                    op: make_str(py, &op_final),
                    left,
                    right,
                },
            )?;
            Some(b.into_any())
        }
        "unary_op_expression" => {
            let first = node.child(0);
            let op = first
                .map(|n| text_of(&n, source).trim().to_string())
                .unwrap_or_default();
            let expr = node
                .named_child(0)
                .and_then(|n| translate(n, source, py).ok().flatten())
                .unwrap_or_else(|| make_none(py));
            let u = Py::new(
                py,
                UnaryOp {
                    lineno: lno,
                    op: make_str(py, &op),
                    expr,
                },
            )?;
            Some(u.into_any())
        }
        "conditional_expression" => {
            let cond = field_child(&node, "condition")
                .and_then(|n| translate(n, source, py).ok().flatten());
            let body =
                field_child(&node, "body").and_then(|n| translate(n, source, py).ok().flatten());
            let alt = field_child(&node, "alternative")
                .and_then(|n| translate(n, source, py).ok().flatten());
            let iftrue = body
                .or_else(|| {
                    field_child(&node, "condition")
                        .and_then(|n| translate(n, source, py).ok().flatten())
                })
                .unwrap_or_else(|| make_none(py));
            let t = Py::new(
                py,
                TernaryOp {
                    lineno: lno,
                    expr: cond.unwrap_or_else(|| make_none(py)),
                    iftrue,
                    iffalse: alt.unwrap_or_else(|| make_none(py)),
                },
            )?;
            Some(t.into_any())
        }
        "cast_expression" => {
            let mut type_name = String::new();
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "cast_type" {
                    type_name = text_of(&child, source).to_lowercase();
                    break;
                }
            }
            let mapping: &[(&str, &str)] = &[
                ("boolean", "bool"),
                ("real", "double"),
                ("float", "double"),
                ("integer", "int"),
            ];
            for (from, to) in mapping {
                if type_name == *from {
                    type_name = to.to_string();
                    break;
                }
            }
            let expr = node
                .named_child(node.named_child_count().saturating_sub(1) as u32)
                .and_then(|n| translate(n, source, py).ok().flatten())
                .unwrap_or_else(|| make_none(py));
            let c = Py::new(
                py,
                Cast {
                    lineno: lno,
                    type_: make_str(py, &type_name),
                    expr,
                },
            )?;
            Some(c.into_any())
        }
        "update_expression" => {
            let mut op = String::new();
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if !child.is_named() {
                    op.push_str(text_of(&child, source));
                }
            }
            let var = node
                .named_child(0)
                .and_then(|n| translate(n, source, py).ok().flatten())
                .unwrap_or_else(|| make_none(py));
            if node.child(0).is_some_and(|c| c.is_named()) {
                let p = Py::new(
                    py,
                    PostIncDecOp {
                        lineno: lno,
                        op: make_str(py, &op),
                        expr: var,
                    },
                )?;
                Some(p.into_any())
            } else {
                let p = Py::new(
                    py,
                    PreIncDecOp {
                        lineno: lno,
                        op: make_str(py, &op),
                        expr: var,
                    },
                )?;
                Some(p.into_any())
            }
        }
        "reference_assignment_expression" => {
            let left = node
                .named_child(0)
                .and_then(|n| translate(n, source, py).ok().flatten())
                .unwrap_or_else(|| make_none(py));
            let right = node
                .named_child(1)
                .and_then(|n| translate(n, source, py).ok().flatten())
                .unwrap_or_else(|| make_none(py));
            let a = Py::new(
                py,
                Assignment {
                    lineno: lno,
                    node: left,
                    expr: right,
                    is_ref: make_bool(py, true),
                },
            )?;
            Some(a.into_any())
        }

        // ── access / calls ──────────────────────────────────────
        "member_access_expression" => {
            let obj = field_child(&node, "object")
                .and_then(|n| translate(n, source, py).ok().flatten())
                .unwrap_or_else(|| make_none(py));
            let name_n = field_child(&node, "name");
            let name = if let Some(nn) = name_n {
                if nn.kind() == "variable_name" || nn.kind() == "variable_variable" {
                    translate(nn, source, py)?.unwrap_or_else(|| make_none(py))
                } else {
                    make_str(py, text_of(&nn, source))
                }
            } else {
                make_str(py, "")
            };
            let op = Py::new(
                py,
                ObjectProperty {
                    lineno: lno,
                    node: obj,
                    name,
                },
            )?;
            Some(op.into_any())
        }
        "subscript_expression" => {
            let obj = node
                .named_child(0)
                .and_then(|n| translate(n, source, py).ok().flatten())
                .unwrap_or_else(|| make_none(py));
            let idx = node
                .named_child(1)
                .and_then(|n| translate(n, source, py).ok().flatten())
                .unwrap_or_else(|| make_none(py));
            let a = Py::new(
                py,
                ArrayOffset {
                    lineno: lno,
                    node: obj,
                    expr: idx,
                },
            )?;
            Some(a.into_any())
        }
        "scoped_property_access_expression" => {
            let scope_n = field_child(&node, "scope");
            let scope = scope_n
                .map(|n| make_str(py, text_of(&n, source)))
                .unwrap_or_else(|| make_str(py, ""));
            let name_n = field_child(&node, "name");
            let name = if let Some(nn) = name_n {
                if nn.kind() == "variable_name" || nn.kind() == "variable_variable" {
                    translate(nn, source, py)?.unwrap_or_else(|| make_none(py))
                } else {
                    make_str(py, text_of(&nn, source))
                }
            } else {
                make_str(py, "")
            };
            let sp = Py::new(
                py,
                StaticProperty {
                    lineno: lno,
                    node: scope,
                    name,
                },
            )?;
            Some(sp.into_any())
        }
        "class_constant_access_expression" => {
            let scope_n = node.named_child(0);
            let scope = scope_n
                .map(|n| make_str(py, text_of(&n, source)))
                .unwrap_or_else(|| make_str(py, ""));
            let name_n = node.named_child(1);
            let name = name_n
                .map(|n| make_str(py, text_of(&n, source)))
                .unwrap_or_else(|| make_str(py, ""));
            let sp = Py::new(
                py,
                StaticProperty {
                    lineno: lno,
                    node: scope,
                    name,
                },
            )?;
            Some(sp.into_any())
        }
        "function_call_expression" => {
            let fn_node = field_child(&node, "function");
            let args_node = field_child(&node, "arguments");
            let fn_val = fn_node
                .and_then(|n| translate(n, source, py).ok().flatten())
                .unwrap_or_else(|| make_none(py));
            let args = args_node
                .map(|a| {
                    let list = PyList::empty(py);
                    let mut cursor = a.walk();
                    for child in a.named_children(&mut cursor) {
                        if let Some(obj) = translate(child, source, py).unwrap_or(None) {
                            list.append(obj).ok();
                        }
                    }
                    list.into()
                })
                .unwrap_or_else(|| PyList::empty(py).into());
            let fc = Py::new(
                py,
                FunctionCall {
                    lineno: lno,
                    name: fn_val,
                    params: args,
                },
            )?;
            Some(fc.into_any())
        }
        "scoped_call_expression" => {
            let scope_n = field_child(&node, "scope");
            let scope = scope_n
                .map(|n| make_str(py, text_of(&n, source)))
                .unwrap_or_else(|| make_str(py, ""));
            let name_n = field_child(&node, "name");
            let method_name = if let Some(nn) = name_n {
                if nn.kind() == "variable_name" || nn.kind() == "variable_variable" {
                    translate(nn, source, py)?.unwrap_or_else(|| make_none(py))
                } else {
                    make_str(py, text_of(&nn, source))
                }
            } else {
                make_str(py, "")
            };
            let args_node = field_child(&node, "arguments");
            let args = args_node
                .map(|a| {
                    let list = PyList::empty(py);
                    let mut cursor = a.walk();
                    for child in a.named_children(&mut cursor) {
                        if let Some(obj) = translate(child, source, py).unwrap_or(None) {
                            list.append(obj).ok();
                        }
                    }
                    list.into()
                })
                .unwrap_or_else(|| PyList::empty(py).into());
            let sm = Py::new(
                py,
                StaticMethodCall {
                    lineno: lno,
                    class_: scope,
                    name: method_name,
                    params: args,
                },
            )?;
            Some(sm.into_any())
        }
        "member_call_expression" => {
            let obj = field_child(&node, "object")
                .and_then(|n| translate(n, source, py).ok().flatten())
                .unwrap_or_else(|| make_none(py));
            let name_n = field_child(&node, "name");
            let name = if let Some(nn) = name_n {
                if nn.kind() == "variable_name" || nn.kind() == "variable_variable" {
                    translate(nn, source, py)?.unwrap_or_else(|| make_none(py))
                } else {
                    make_str(py, text_of(&nn, source))
                }
            } else {
                make_str(py, "")
            };
            let args_node = field_child(&node, "arguments");
            let args = args_node
                .map(|a| {
                    let list = PyList::empty(py);
                    let mut cursor = a.walk();
                    for child in a.named_children(&mut cursor) {
                        if let Some(obj) = translate(child, source, py).unwrap_or(None) {
                            list.append(obj).ok();
                        }
                    }
                    list.into()
                })
                .unwrap_or_else(|| PyList::empty(py).into());
            let mc = Py::new(
                py,
                MethodCall {
                    lineno: lno,
                    node: obj,
                    name,
                    params: args,
                },
            )?;
            Some(mc.into_any())
        }
        "nullsafe_member_access_expression" => {
            let obj = field_child(&node, "object")
                .and_then(|n| translate(n, source, py).ok().flatten())
                .unwrap_or_else(|| make_none(py));
            let name_n = field_child(&node, "name");
            let name = if let Some(nn) = name_n {
                if nn.kind() == "variable_name" {
                    translate(nn, source, py)?.unwrap_or_else(|| make_none(py))
                } else {
                    make_str(py, text_of(&nn, source))
                }
            } else {
                make_str(py, "")
            };
            let np = Py::new(
                py,
                NullsafePropertyAccess {
                    lineno: lno,
                    node: obj,
                    name,
                },
            )?;
            Some(np.into_any())
        }
        "nullsafe_member_call_expression" => {
            let obj = field_child(&node, "object")
                .and_then(|n| translate(n, source, py).ok().flatten())
                .unwrap_or_else(|| make_none(py));
            let name_n = field_child(&node, "name");
            let name = name_n
                .map(|n| make_str(py, text_of(&n, source)))
                .unwrap_or_else(|| make_str(py, ""));
            let args_node = field_child(&node, "arguments");
            let args = args_node
                .map(|a| {
                    let list = PyList::empty(py);
                    let mut cursor = a.walk();
                    for child in a.named_children(&mut cursor) {
                        if let Some(obj) = translate(child, source, py).unwrap_or(None) {
                            list.append(obj).ok();
                        }
                    }
                    list.into()
                })
                .unwrap_or_else(|| PyList::empty(py).into());
            let nc = Py::new(
                py,
                NullsafeCall {
                    lineno: lno,
                    node: obj,
                    name,
                    params: args,
                },
            )?;
            Some(nc.into_any())
        }
        "object_creation_expression" => {
            let mut cls_name = String::new();
            let mut args_node: Option<Node> = None;
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.is_named() && child.kind() != "arguments" {
                    cls_name = text_of(&child, source).to_string();
                } else if child.kind() == "arguments" {
                    args_node = Some(child);
                }
            }
            let args = args_node
                .map(|a| {
                    let list = PyList::empty(py);
                    let mut cursor2 = a.walk();
                    for child in a.named_children(&mut cursor2) {
                        if let Some(obj) = translate(child, source, py).unwrap_or(None) {
                            list.append(obj).ok();
                        }
                    }
                    list.into()
                })
                .unwrap_or_else(|| PyList::empty(py).into());
            let n = Py::new(
                py,
                New {
                    lineno: lno,
                    name: make_str(py, &cls_name),
                    params: args,
                },
            )?;
            Some(n.into_any())
        }
        "argument" => {
            let name_node = field_child(&node, "name");
            if let Some(nn) = name_node {
                let value = node
                    .named_child(node.named_child_count().saturating_sub(1) as u32)
                    .and_then(|n| translate(n, source, py).ok().flatten())
                    .unwrap_or_else(|| make_none(py));
                let na = Py::new(
                    py,
                    NamedArgument {
                        lineno: lno,
                        name: make_str(py, text_of(&nn, source)),
                        node: value,
                    },
                )?;
                Some(na.into_any())
            } else {
                let last = node.named_child(node.named_child_count().saturating_sub(1) as u32);
                let value = last
                    .and_then(|n| translate(n, source, py).ok().flatten())
                    .unwrap_or_else(|| make_none(py));
                let has_ref = text_of(&node, source).contains("&");
                let p = Py::new(
                    py,
                    Parameter {
                        lineno: lno,
                        node: value,
                        is_ref: make_bool(py, has_ref),
                    },
                )?;
                Some(p.into_any())
            }
        }

        // ── control flow ─────────────────────────────────────────
        "if_statement" => {
            let cond = field_child(&node, "condition")
                .and_then(|n| translate(n, source, py).ok().flatten());
            let body =
                field_child(&node, "body").and_then(|n| translate(n, source, py).ok().flatten());
            let elseifs = PyList::empty(py);
            let mut else_node = None;
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                if child.kind() == "else_if_clause" {
                    let e_cond = field_child(&child, "condition")
                        .and_then(|n| translate(n, source, py).ok().flatten());
                    let e_body = field_child(&child, "body")
                        .and_then(|n| translate(n, source, py).ok().flatten());
                    let ei = Py::new(
                        py,
                        ElseIf {
                            lineno: lineno(&child),
                            expr: e_cond.unwrap_or_else(|| make_none(py)),
                            node: e_body.unwrap_or_else(|| make_none(py)),
                        },
                    )?;
                    elseifs.append(ei.into_any())?;
                } else if child.kind() == "else_clause" {
                    let e_body = field_child(&child, "body")
                        .and_then(|n| translate(n, source, py).ok().flatten());
                    else_node = Some(
                        Py::new(
                            py,
                            Else {
                                lineno: lineno(&child),
                                node: e_body.unwrap_or_else(|| make_none(py)),
                            },
                        )?
                        .into_any(),
                    );
                }
            }
            let i = Py::new(
                py,
                If {
                    lineno: lno,
                    expr: cond.unwrap_or_else(|| make_none(py)),
                    node: body.unwrap_or_else(|| make_none(py)),
                    elseifs: elseifs.into(),
                    else_: else_node.unwrap_or_else(|| make_none(py)),
                },
            )?;
            Some(i.into_any())
        }
        "while_statement" => {
            let cond = node
                .named_child(0)
                .and_then(|n| translate(n, source, py).ok().flatten());
            let body = node
                .named_child(1)
                .and_then(|n| translate(n, source, py).ok().flatten())
                .unwrap_or_else(|| {
                    let block = Py::new(
                        py,
                        Block {
                            lineno: lno,
                            nodes: PyList::empty(py).into(),
                        },
                    )
                    .ok()
                    .map(|b| b.into_any());
                    block.unwrap_or_else(|| make_none(py))
                });
            let w = Py::new(
                py,
                While {
                    lineno: lno,
                    expr: cond.unwrap_or_else(|| make_none(py)),
                    node: body,
                },
            )?;
            Some(w.into_any())
        }
        "do_statement" => {
            let body = node
                .named_child(0)
                .and_then(|n| translate(n, source, py).ok().flatten())
                .unwrap_or_else(|| make_none(py));
            let cond = node
                .named_child(1)
                .and_then(|n| translate(n, source, py).ok().flatten());
            let dw = Py::new(
                py,
                DoWhile {
                    lineno: lno,
                    node: body,
                    expr: cond.unwrap_or_else(|| make_none(py)),
                },
            )?;
            Some(dw.into_any())
        }
        "for_statement" => {
            let start = node
                .named_child(0)
                .and_then(|n| translate(n, source, py).ok().flatten());
            let test = node
                .named_child(1)
                .and_then(|n| translate(n, source, py).ok().flatten());
            let count = node
                .named_child(2)
                .and_then(|n| translate(n, source, py).ok().flatten());
            let body = node
                .named_child(3)
                .and_then(|n| translate(n, source, py).ok().flatten())
                .unwrap_or_else(|| make_none(py));
            let f = Py::new(
                py,
                For {
                    lineno: lno,
                    start: start.unwrap_or_else(|| make_none(py)),
                    test: test.unwrap_or_else(|| make_none(py)),
                    count: count.unwrap_or_else(|| make_none(py)),
                    node: body,
                },
            )?;
            Some(f.into_any())
        }
        "foreach_statement" => {
            let arr = node
                .named_child(0)
                .and_then(|n| translate(n, source, py).ok().flatten());
            let second = node.named_child(1);
            let (key, val_obj, _is_ref_val) = if let Some(sec) = second {
                if sec.kind() == "pair" {
                    let k = sec
                        .named_child(0)
                        .and_then(|n| translate(n, source, py).ok().flatten())
                        .unwrap_or_else(|| make_none(py));
                    let inner = sec.named_child(1);
                    let (ref_flag, vn) = if let Some(inn) = inner {
                        if inn.kind() == "by_ref" {
                            (true, inn.named_child(0))
                        } else {
                            (false, Some(inn))
                        }
                    } else {
                        (false, None)
                    };
                    let v = vn
                        .and_then(|n| translate(n, source, py).ok().flatten())
                        .unwrap_or_else(|| make_none(py));
                    let fv = Py::new(
                        py,
                        ForeachVariable {
                            lineno: lineno(&vn.unwrap_or(sec)),
                            name: v,
                            is_ref: make_bool(py, ref_flag),
                        },
                    )?;
                    (Some(k), fv.into_any(), ref_flag)
                } else if sec.kind() == "by_ref" {
                    let vn = sec.named_child(0);
                    let v = vn
                        .and_then(|n| translate(n, source, py).ok().flatten())
                        .unwrap_or_else(|| make_none(py));
                    let fv = Py::new(
                        py,
                        ForeachVariable {
                            lineno: lineno(&vn.unwrap_or(sec)),
                            name: v,
                            is_ref: make_bool(py, true),
                        },
                    )?;
                    (None, fv.into_any(), true)
                } else {
                    let v = translate(sec, source, py)?.unwrap_or_else(|| make_none(py));
                    let fv = Py::new(
                        py,
                        ForeachVariable {
                            lineno: lineno(&sec),
                            name: v,
                            is_ref: make_bool(py, false),
                        },
                    )?;
                    (None, fv.into_any(), false)
                }
            } else {
                (None, make_none(py), false)
            };
            let body_node = node
                .named_child(2)
                .and_then(|n| translate(n, source, py).ok().flatten())
                .unwrap_or_else(|| make_none(py));
            let fe = Py::new(
                py,
                Foreach {
                    lineno: lno,
                    expr: arr.unwrap_or_else(|| make_none(py)),
                    keyvar: key.unwrap_or_else(|| make_none(py)),
                    valvar: val_obj,
                    node: body_node,
                },
            )?;
            Some(fe.into_any())
        }
        "switch_statement" => {
            let expr = node
                .named_child(0)
                .and_then(|n| translate(n, source, py).ok().flatten());
            let switch_block = node.named_child(1);
            let cases = switch_block
                .and_then(|n| translate(n, source, py).ok().flatten())
                .unwrap_or_else(|| PyList::empty(py).into());
            let sw = Py::new(
                py,
                Switch {
                    lineno: lno,
                    expr: expr.unwrap_or_else(|| make_none(py)),
                    nodes: cases,
                },
            )?;
            Some(sw.into_any())
        }
        "switch_block" => {
            let list = PyList::empty(py);
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                if let Some(obj) = translate(child, source, py)? {
                    list.append(obj)?;
                }
            }
            Some(list.into())
        }
        "case_statement" | "default_statement" => {
            let body_nodes = PyList::empty(py);
            let mut cursor = node.walk();
            let mut first_expr = true;
            for child in node.named_children(&mut cursor) {
                if kind == "case_statement" && first_expr {
                    first_expr = false;
                    continue;
                }
                if let Some(obj) = translate(child, source, py)? {
                    if let Ok(seq) = obj.cast_bound::<PyList>(py) {
                        for i in 0..seq.len() {
                            body_nodes.append(seq.get_item(i)?)?;
                        }
                    } else {
                        body_nodes.append(obj)?;
                    }
                }
            }
            if kind == "case_statement" {
                let expr_n = field_child(&node, "value");
                let expr = expr_n
                    .and_then(|n| translate(n, source, py).ok().flatten())
                    .unwrap_or_else(|| make_none(py));
                let c = Py::new(
                    py,
                    Case {
                        lineno: lno,
                        expr,
                        nodes: body_nodes.into(),
                    },
                )?;
                Some(c.into_any())
            } else {
                let d = Py::new(
                    py,
                    Default {
                        lineno: lno,
                        nodes: body_nodes.into(),
                    },
                )?;
                Some(d.into_any())
            }
        }
        "match_expression" => {
            let cond = field_child(&node, "condition")
                .and_then(|n| translate(n, source, py).ok().flatten());
            let body_node = field_child(&node, "body");
            let arms = body_node
                .and_then(|n| translate(n, source, py).ok().flatten())
                .unwrap_or_else(|| PyList::empty(py).into());
            let m = Py::new(
                py,
                MatchExpr {
                    lineno: lno,
                    condition: cond.unwrap_or_else(|| make_none(py)),
                    arms,
                },
            )?;
            Some(m.into_any())
        }
        "match_block" => {
            let list = PyList::empty(py);
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                if let Some(obj) = translate(child, source, py)? {
                    list.append(obj)?;
                }
            }
            Some(list.into())
        }
        "match_conditional_expression" => {
            let cond_n = field_child(&node, "conditional_expressions");
            let pattern: Py<PyAny> = if let Some(cn) = cond_n {
                let items = PyList::empty(py);
                let mut cursor = cn.walk();
                for child in cn.named_children(&mut cursor) {
                    if let Some(obj) = translate(child, source, py)? {
                        items.append(obj)?;
                    }
                }
                if items.len() == 1 {
                    items.get_item(0)?.into()
                } else {
                    items.into()
                }
            } else {
                make_none(py)
            };
            let body = field_child(&node, "return_expression")
                .and_then(|n| translate(n, source, py).ok().flatten());
            let ma = Py::new(
                py,
                MatchArm {
                    lineno: lno,
                    pattern,
                    body: body.unwrap_or_else(|| make_none(py)),
                },
            )?;
            Some(ma.into_any())
        }
        "match_default_expression" => {
            let body = field_child(&node, "return_expression")
                .and_then(|n| translate(n, source, py).ok().flatten());
            let ma = Py::new(
                py,
                MatchArm {
                    lineno: lno,
                    pattern: make_none(py),
                    body: body.unwrap_or_else(|| make_none(py)),
                },
            )?;
            Some(ma.into_any())
        }

        // ── return / break / continue / throw ───────────────────
        "return_statement" => {
            let expr = node
                .named_child(0)
                .and_then(|n| translate(n, source, py).ok().flatten());
            let r = Py::new(
                py,
                ReturnNode {
                    lineno: lno,
                    node: expr.unwrap_or_else(|| make_none(py)),
                },
            )?;
            Some(r.into_any())
        }
        "break_statement" => {
            let depth = node
                .named_child(0)
                .and_then(|n| translate(n, source, py).ok().flatten());
            let b = Py::new(
                py,
                BreakNode {
                    lineno: lno,
                    node: depth.unwrap_or_else(|| make_none(py)),
                },
            )?;
            Some(b.into_any())
        }
        "continue_statement" => {
            let depth = node
                .named_child(0)
                .and_then(|n| translate(n, source, py).ok().flatten());
            let c = Py::new(
                py,
                ContinueNode {
                    lineno: lno,
                    node: depth.unwrap_or_else(|| make_none(py)),
                },
            )?;
            Some(c.into_any())
        }
        "throw_expression" => {
            let expr = node
                .named_child(0)
                .and_then(|n| translate(n, source, py).ok().flatten());
            let t = Py::new(
                py,
                Throw {
                    lineno: lno,
                    node: expr.unwrap_or_else(|| make_none(py)),
                },
            )?;
            Some(t.into_any())
        }
        "yield_expression" => {
            let mut expr = None;
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                if child.kind() == "yield" {
                    continue;
                }
                expr = translate(child, source, py).ok().flatten();
                break;
            }
            let y = Py::new(
                py,
                YieldNode {
                    lineno: lno,
                    node: expr.unwrap_or_else(|| make_none(py)),
                },
            )?;
            Some(y.into_any())
        }
        "exit_statement" => {
            let mut arg = None;
            let mut cursor = node.walk();
            if let Some(child) = node.named_children(&mut cursor).next() {
                arg = translate(child, source, py).ok().flatten();
            }
            let et = if text_of(&node, source).to_lowercase().contains("exit") {
                "exit"
            } else {
                "die"
            };
            let e = Py::new(
                py,
                Exit {
                    lineno: lno,
                    expr: arg.unwrap_or_else(|| make_none(py)),
                    type_: make_str(py, et),
                },
            )?;
            Some(e.into_any())
        }
        "print_intrinsic" => {
            let expr = node
                .named_child(0)
                .and_then(|n| translate(n, source, py).ok().flatten());
            let p = Py::new(
                py,
                Print {
                    lineno: lno,
                    node: expr.unwrap_or_else(|| make_none(py)),
                },
            )?;
            Some(p.into_any())
        }
        "clone_expression" => {
            let expr = node
                .named_child(0)
                .and_then(|n| translate(n, source, py).ok().flatten());
            let c = Py::new(
                py,
                CloneNode {
                    lineno: lno,
                    node: expr.unwrap_or_else(|| make_none(py)),
                },
            )?;
            Some(c.into_any())
        }
        "unset_statement" => {
            let vars = PyList::empty(py);
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                if let Some(obj) = translate(child, source, py)? {
                    vars.append(obj)?;
                }
            }
            let u = Py::new(
                py,
                Unset {
                    lineno: lno,
                    nodes: vars.into(),
                },
            )?;
            Some(u.into_any())
        }
        "parenthesized_expression" => translate(node.named_child(0).unwrap_or(node), source, py)?,
        "error_suppression_expression" => {
            let expr = node
                .named_child(0)
                .and_then(|n| translate(n, source, py).ok().flatten());
            let s = Py::new(
                py,
                Silence {
                    lineno: lno,
                    expr: expr.unwrap_or_else(|| make_none(py)),
                },
            )?;
            Some(s.into_any())
        }

        // ── include / require / echo / global ───────────────────
        "include_expression"
        | "include_once_expression"
        | "require_expression"
        | "require_once_expression" => {
            let expr = node
                .named_child(0)
                .and_then(|n| translate(n, source, py).ok().flatten());
            let once = kind.contains("once");
            if kind.starts_with("include") {
                let i = Py::new(
                    py,
                    Include {
                        lineno: lno,
                        expr: expr.unwrap_or_else(|| make_none(py)),
                        once: make_bool(py, once),
                    },
                )?;
                Some(i.into_any())
            } else {
                let r = Py::new(
                    py,
                    Require {
                        lineno: lno,
                        expr: expr.unwrap_or_else(|| make_none(py)),
                        once: make_bool(py, once),
                    },
                )?;
                Some(r.into_any())
            }
        }
        "echo_statement" | "echo_stdout" => translate_echo(node, source, py)?,
        "global_statement" | "global_declaration" => {
            let vars = PyList::empty(py);
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                let t = text_of(&child, source).to_lowercase();
                if t == "global" {
                    continue;
                }
                if let Some(obj) = translate(child, source, py)? {
                    vars.append(obj)?;
                }
            }
            let g = Py::new(
                py,
                Global {
                    lineno: lno,
                    nodes: vars.into(),
                },
            )?;
            Some(g.into_any())
        }
        "const_declaration" => {
            let consts = PyList::empty(py);
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                if child.kind() != "const_element" {
                    continue;
                }
                let nc = child.named_child_count();
                let name = child
                    .named_child(0)
                    .map(|n| make_str(py, text_of(&n, source)))
                    .unwrap_or_else(|| make_str(py, ""));
                let value = if nc > 1 {
                    child
                        .named_child(1)
                        .and_then(|n| translate(n, source, py).ok().flatten())
                } else {
                    None
                };
                let cd = Py::new(
                    py,
                    ConstantDeclaration {
                        lineno: lno,
                        name,
                        initial: value.unwrap_or_else(|| make_none(py)),
                    },
                )?;
                consts.append(cd.into_any())?;
            }
            let cd_outer = Py::new(
                py,
                ConstantDeclarations {
                    lineno: lno,
                    nodes: consts.into(),
                },
            )?;
            Some(cd_outer.into_any())
        }
        "declare_statement" => {
            let directives = PyList::empty(py);
            let body_nodes = PyList::empty(py);
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                if child.kind() == "declare_directive" {
                    let txt = text_of(&child, source);
                    let parts: Vec<&str> = txt.splitn(2, '=').collect();
                    let dname = parts.first().map(|s| s.trim()).unwrap_or("");
                    let dvalue = child
                        .named_child(0)
                        .and_then(|n| translate(n, source, py).ok().flatten());
                    let d = Py::new(
                        py,
                        Directive {
                            lineno: lineno(&child),
                            name: make_str(py, dname),
                            node: dvalue.unwrap_or_else(|| {
                                make_str(py, parts.get(1).map(|s| s.trim()).unwrap_or(""))
                            }),
                        },
                    )?;
                    directives.append(d.into_any())?;
                } else if let Some(obj) = translate(child, source, py)? {
                    body_nodes.append(obj)?;
                }
            }
            let body = if body_nodes.is_empty() {
                make_none(py)
            } else {
                body_nodes.into()
            };
            let dec = Py::new(
                py,
                Declare {
                    lineno: lno,
                    directives: directives.into(),
                    node: body,
                },
            )?;
            Some(dec.into_any())
        }
        "namespace_use_declaration" => {
            let decls = PyList::empty(py);
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "namespace_use_clause" {
                    if let Some(obj) = translate(child, source, py)? {
                        decls.append(obj)?;
                    }
                }
            }
            let ud = Py::new(
                py,
                UseDeclarations {
                    lineno: lno,
                    nodes: decls.into(),
                },
            )?;
            Some(ud.into_any())
        }
        "use_declaration" => {
            let mut name = String::new();
            let modifiers = PyList::empty(py);
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                if child.kind() == "name" {
                    name = text_of(&child, source).to_string();
                } else if child.kind() == "use_list" {
                    let mut cursor2 = child.walk();
                    for sub in child.named_children(&mut cursor2) {
                        if sub.kind() == "use_as_clause" {
                            if let Some(obj) = translate(sub, source, py)? {
                                modifiers.append(obj)?;
                            }
                        }
                    }
                }
            }
            let tu = Py::new(
                py,
                TraitUse {
                    lineno: lno,
                    name: make_str(py, &name),
                    renames: modifiers.into(),
                },
            )?;
            Some(tu.into_any())
        }
        "use_as_clause" => {
            let _named_count = node.named_child_count();
            let original = node
                .named_child(0)
                .and_then(|n| translate(n, source, py).ok().flatten());
            let mut alias = None;
            let mut visibility = None;
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor).skip(1) {
                if child.kind() == "visibility_modifier" {
                    visibility = Some(text_of(&child, source).trim().to_lowercase());
                } else if child.kind() == "name" || child.kind() == "qualified_name" {
                    alias = Some(text_of(&child, source).to_string());
                }
            }
            let tm = Py::new(
                py,
                TraitModifier {
                    lineno: lno,
                    from: original.unwrap_or_else(|| make_none(py)),
                    to: alias
                        .map(|a| make_str(py, &a))
                        .unwrap_or_else(|| make_none(py)),
                    visibility: visibility
                        .map(|v| make_str(py, &v))
                        .unwrap_or_else(|| make_none(py)),
                },
            )?;
            Some(tm.into_any())
        }
        "use_list" => {
            let list = PyList::empty(py);
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                if child.kind() == "use_as_clause" {
                    if let Some(obj) = translate(child, source, py)? {
                        list.append(obj)?;
                    }
                }
            }
            Some(list.into())
        }
        "namespace_use_clause" => {
            let named = node.named_child(0);
            let name = named
                .map(|n| make_str(py, text_of(&n, source)))
                .unwrap_or_else(|| make_str(py, ""));
            let alias_node = field_child(&node, "alias");
            let alias = alias_node.map(|n| make_str(py, text_of(&n, source)));
            let d = Py::new(
                py,
                UseDeclaration {
                    lineno: lno,
                    name,
                    alias: alias.unwrap_or_else(|| make_none(py)),
                },
            )?;
            Some(d.into_any())
        }

        // ── function / class / trait ────────────────────────────
        "function_definition" => {
            let name = field_child(&node, "name")
                .map(|n| make_str(py, text_of(&n, source)))
                .unwrap_or_else(|| make_str(py, ""));
            let params_node = field_child(&node, "parameters");
            let params = params_node
                .map(|pn| {
                    let list = PyList::empty(py);
                    let mut cursor = pn.walk();
                    for child in pn.named_children(&mut cursor) {
                        if let Some(obj) = translate(child, source, py).unwrap_or(None) {
                            list.append(obj).ok();
                        }
                    }
                    list.into()
                })
                .unwrap_or_else(|| PyList::empty(py).into());
            let body = field_child(&node, "body")
                .and_then(|n| translate(n, source, py).ok().flatten())
                .unwrap_or_else(|| make_none(py));
            let return_type_node = field_child(&node, "return_type");
            let return_type = return_type_node
                .map(|n| make_str(py, text_of(&n, source)))
                .unwrap_or_else(|| make_none(py));
            let f = Py::new(
                py,
                Function {
                    lineno: lno,
                    name,
                    params,
                    nodes: body,
                    is_ref: make_bool(py, false),
                    return_type,
                },
            )?;
            Some(f.into_any())
        }
        "anonymous_function" => {
            let params = PyList::empty(py);
            let uses = PyList::empty(py);
            let mut is_ref = false;
            let mut body_stmts = PyList::empty(py);
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "reference_modifier" && child.is_named() {
                    is_ref = true;
                } else if child.kind() == "formal_parameters" {
                    let mut c2 = child.walk();
                    for p in child.named_children(&mut c2) {
                        if let Some(obj) = translate(p, source, py)? {
                            params.append(obj)?;
                        }
                    }
                } else if child.kind() == "anonymous_function_use_clause" {
                    let mut c2 = child.walk();
                    for uc in child.children(&mut c2) {
                        if uc.kind() == "variable_name" {
                            let lv = Py::new(
                                py,
                                LexicalVariable {
                                    lineno: lineno(&uc),
                                    name: make_str(py, text_of(&uc, source)),
                                    is_ref: make_bool(py, false),
                                },
                            )?;
                            uses.append(lv.into_any())?;
                        } else if uc.kind() == "by_ref" {
                            let vn = uc.named_child(0);
                            let vn_text = vn
                                .map(|n| text_of(&n, source).to_string())
                                .unwrap_or_default();
                            let lv = Py::new(
                                py,
                                LexicalVariable {
                                    lineno: lineno(&uc),
                                    name: make_str(py, &vn_text),
                                    is_ref: make_bool(py, true),
                                },
                            )?;
                            uses.append(lv.into_any())?;
                        }
                    }
                } else if child.kind() == "compound_statement" {
                    if let Some(bres) = translate(child, source, py)? {
                        body_stmts = PyList::empty(py);
                        body_stmts.append(bres)?;
                    }
                }
            }
            let c = Py::new(
                py,
                Closure {
                    lineno: lno,
                    params: params.into(),
                    vars: uses.into(),
                    nodes: body_stmts.into(),
                    is_ref: make_bool(py, is_ref),
                },
            )?;
            Some(c.into_any())
        }
        "simple_parameter" => {
            let type_node = field_child(&node, "type");
            let name_node = field_child(&node, "name");
            let default_node = field_child(&node, "default_value");
            let type_name = type_node.map(|n| make_str(py, text_of(&n, source)));
            let name = name_node
                .map(|n| make_str(py, text_of(&n, source)))
                .unwrap_or_else(|| make_str(py, ""));
            let default_val = default_node.and_then(|n| translate(n, source, py).ok().flatten());
            let has_ref = text_of(&node, source).contains("&")
                || (0..node.child_count()).any(|i| {
                    node.child(i as u32)
                        .is_some_and(|c| c.kind() == "reference_modifier")
                });
            let fp = Py::new(
                py,
                FormalParameter {
                    lineno: lno,
                    name,
                    default: default_val.unwrap_or_else(|| make_none(py)),
                    is_ref: make_bool(py, has_ref),
                    type_: type_name.unwrap_or_else(|| make_none(py)),
                },
            )?;
            Some(fp.into_any())
        }
        "property_promotion_parameter" => {
            let modifiers = PyList::empty(py);
            let mut type_name: Option<Py<PyAny>> = None;
            let mut name = String::new();
            let default_val: Option<Py<PyAny>>;
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "visibility_modifier" {
                    modifiers
                        .append(make_str(py, text_of(&child, source).trim()))
                        .ok();
                } else if child.kind() == "name" {
                    // type
                    type_name = Some(make_str(py, text_of(&child, source)));
                } else if child.kind() == "variable_name" {
                    name = text_of(&child, source).to_string();
                }
            }
            let def_node = field_child(&node, "default_value");
            default_val = def_node.and_then(|n| translate(n, source, py).ok().flatten());
            let cp = Py::new(
                py,
                ConstructorParameter {
                    lineno: lno,
                    modifiers: modifiers.into(),
                    name: make_str(py, &name),
                    type_: type_name.unwrap_or_else(|| make_none(py)),
                    default: default_val.unwrap_or_else(|| make_none(py)),
                },
            )?;
            Some(cp.into_any())
        }
        "method_declaration" => {
            let modifiers = PyList::empty(py);
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                match child.kind() {
                    "visibility_modifier" => {
                        modifiers
                            .append(make_str(py, text_of(&child, source).trim()))
                            .ok();
                    }
                    "static_modifier" => {
                        modifiers.append(make_str(py, "static")).ok();
                    }
                    "final_modifier" => {
                        modifiers.append(make_str(py, "final")).ok();
                    }
                    "abstract_modifier" => {
                        modifiers.append(make_str(py, "abstract")).ok();
                    }
                    _ => {}
                }
            }
            let name = field_child(&node, "name")
                .map(|n| make_str(py, text_of(&n, source)))
                .unwrap_or_else(|| make_str(py, ""));
            let params_node = field_child(&node, "parameters");
            let params = params_node
                .map(|pn| {
                    let list = PyList::empty(py);
                    let mut c = pn.walk();
                    for child in pn.named_children(&mut c) {
                        if let Some(obj) = translate(child, source, py).unwrap_or(None) {
                            list.append(obj).ok();
                        }
                    }
                    list.into()
                })
                .unwrap_or_else(|| PyList::empty(py).into());
            let body = field_child(&node, "body")
                .and_then(|n| translate(n, source, py).ok().flatten())
                .unwrap_or_else(|| make_none(py));
            let m = Py::new(
                py,
                Method {
                    lineno: lno,
                    name,
                    modifiers: modifiers.into(),
                    params,
                    nodes: body,
                    is_ref: make_bool(py, false),
                },
            )?;
            Some(m.into_any())
        }
        "class_declaration" => {
            let name = field_child(&node, "name")
                .map(|n| make_str(py, text_of(&n, source)))
                .unwrap_or_else(|| make_str(py, ""));
            let mods = PyList::empty(py);
            let mut base_name: Option<Py<PyAny>> = None;
            let interfaces = PyList::empty(py);
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                match child.kind() {
                    "final_modifier" | "readonly_modifier" => {
                        mods.append(make_str(
                            py,
                            text_of(&child, source).trim().to_lowercase().as_str(),
                        ))
                        .ok();
                    }
                    "base_clause" => {
                        if let Some(bc) = child.named_children(&mut child.walk()).next() {
                            base_name = Some(make_str(py, text_of(&bc, source)));
                        }
                    }
                    "class_interface_clause" => {
                        let mut c2 = child.walk();
                        for ic in child.named_children(&mut c2) {
                            interfaces.append(make_str(py, text_of(&ic, source))).ok();
                        }
                    }
                    _ => {}
                }
            }
            let body_node = field_child(&node, "body");
            let body = PyList::empty(py);
            let uses = PyList::empty(py);
            if let Some(bn) = body_node {
                let mut c = bn.walk();
                for child in bn.children(&mut c) {
                    if child.kind() == "{"
                        || child.kind() == "}"
                        || child.kind() == ";"
                        || child.kind() == ""
                    {
                        continue;
                    }
                    if child.kind() == "const_declaration" {
                        let consts = PyList::empty(py);
                        let mut c2 = child.walk();
                        for elem_node in child.named_children(&mut c2) {
                            if elem_node.kind() == "const_element" {
                                let nm = elem_node
                                    .named_child(0)
                                    .map(|n| make_str(py, text_of(&n, source)))
                                    .unwrap_or_else(|| make_str(py, ""));
                                let val = if elem_node.named_child_count() > 1 {
                                    elem_node
                                        .named_child(1)
                                        .and_then(|n| translate(n, source, py).ok().flatten())
                                } else {
                                    None
                                };
                                let cc = Py::new(
                                    py,
                                    ClassConstant {
                                        lineno: lno,
                                        name: nm,
                                        initial: val.unwrap_or_else(|| make_none(py)),
                                    },
                                )?;
                                consts.append(cc.into_any())?;
                            }
                        }
                        if consts.len() > 0 {
                            let ccs = Py::new(
                                py,
                                ClassConstants {
                                    lineno: lno,
                                    nodes: consts.into(),
                                },
                            )?;
                            body.append(ccs.into_any())?;
                        }
                    } else if child.kind() == "use_declaration" {
                        if let Some(res) = translate(child, source, py)? {
                            uses.append(res)?;
                        }
                    } else if let Some(res) = translate(child, source, py)? {
                        if let Ok(seq) = res.cast_bound::<PyList>(py) {
                            for i in 0..seq.len() {
                                body.append(seq.get_item(i)?)?;
                            }
                        } else {
                            body.append(res)?;
                        }
                    }
                }
            }
            let mod_type = if mods.len() > 0 {
                Some(mods.get_item(0)?.into())
            } else {
                None
            };
            let c_decl = Py::new(
                py,
                Class {
                    lineno: lno,
                    name,
                    type_: mod_type.unwrap_or_else(|| make_none(py)),
                    extends: base_name.unwrap_or_else(|| make_none(py)),
                    implements: interfaces.into(),
                    traits: uses.into(),
                    nodes: body.into(),
                },
            )?;
            Some(c_decl.into_any())
        }
        "trait_declaration" => {
            let name = field_child(&node, "name")
                .map(|n| make_str(py, text_of(&n, source)))
                .unwrap_or_else(|| make_str(py, ""));
            let body_node = field_child(&node, "body");
            let body = PyList::empty(py);
            let uses = PyList::empty(py);
            if let Some(bn) = body_node {
                let mut c = bn.walk();
                for child in bn.children(&mut c) {
                    if child.kind() == "{" || child.kind() == "}" || child.kind() == ";" {
                        continue;
                    }
                    if child.kind() == "use_declaration" {
                        if let Some(res) = translate(child, source, py)? {
                            uses.append(res)?;
                        }
                    } else if let Some(res) = translate(child, source, py)? {
                        if let Ok(seq) = res.cast_bound::<PyList>(py) {
                            for i in 0..seq.len() {
                                body.append(seq.get_item(i)?)?;
                            }
                        } else {
                            body.append(res)?;
                        }
                    }
                }
            }
            let t = Py::new(
                py,
                Trait {
                    lineno: lno,
                    name,
                    traits: uses.into(),
                    nodes: body.into(),
                },
            )?;
            Some(t.into_any())
        }
        "interface_declaration" => {
            let name = field_child(&node, "name")
                .map(|n| make_str(py, text_of(&n, source)))
                .unwrap_or_else(|| make_str(py, ""));
            let mut extends: Option<Py<PyAny>> = None;
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                if child.kind() == "base_clause" {
                    let ext_names: Vec<&str> = {
                        let mut c2 = child.walk();
                        child
                            .named_children(&mut c2)
                            .map(|nc| text_of(&nc, source))
                            .collect()
                    };
                    extends = if ext_names.len() == 1 {
                        Some(make_str(py, ext_names[0]))
                    } else {
                        Some(make_str(py, &ext_names.join(", ")))
                    };
                    break;
                }
            }
            let body_node = field_child(&node, "body");
            let body = PyList::empty(py);
            if let Some(bn) = body_node {
                let mut c = bn.walk();
                for child in bn.children(&mut c) {
                    if child.kind() == "{" || child.kind() == "}" || child.kind() == ";" {
                        continue;
                    }
                    if let Some(res) = translate(child, source, py)? {
                        if let Ok(seq) = res.cast_bound::<PyList>(py) {
                            for i in 0..seq.len() {
                                body.append(seq.get_item(i)?)?;
                            }
                        } else {
                            body.append(res)?;
                        }
                    }
                }
            }
            let i = Py::new(
                py,
                Interface {
                    lineno: lno,
                    name,
                    extends: extends.unwrap_or_else(|| make_none(py)),
                    nodes: body.into(),
                },
            )?;
            Some(i.into_any())
        }
        "function_static_declaration" => {
            let vars = PyList::empty(py);
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                let name_node = field_child(&child, "name");
                let value_node = field_child(&child, "value");
                let nm = name_node.map(|n| make_str(py, text_of(&n, source)));
                let val = value_node.and_then(|n| translate(n, source, py).ok().flatten());
                let sv = Py::new(
                    py,
                    StaticVariable {
                        lineno: lineno(&child),
                        name: nm.unwrap_or_else(|| make_str(py, "")),
                        initial: val.unwrap_or_else(|| make_none(py)),
                    },
                )?;
                vars.append(sv.into_any())?;
            }
            let s = Py::new(
                py,
                Static {
                    lineno: lno,
                    nodes: vars.into(),
                },
            )?;
            Some(s.into_any())
        }
        "property_declaration" => {
            let modifiers = PyList::empty(py);
            let elements = PyList::empty(py);
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                match child.kind() {
                    "visibility_modifier" => {
                        modifiers
                            .append(make_str(py, text_of(&child, source).trim()))
                            .ok();
                    }
                    "static_modifier" => {
                        modifiers.append(make_str(py, "static")).ok();
                    }
                    "readonly_modifier" => {
                        modifiers.append(make_str(py, "readonly")).ok();
                    }
                    "property_element" => {
                        let name_n = field_child(&child, "name");
                        let val_n = field_child(&child, "default_value");
                        let nm = name_n.map(|n| make_str(py, text_of(&n, source)));
                        let val = val_n.and_then(|n| translate(n, source, py).ok().flatten());
                        let cv = Py::new(
                            py,
                            ClassVariable {
                                lineno: lno,
                                name: nm.unwrap_or_else(|| make_str(py, "")),
                                initial: val.unwrap_or_else(|| make_none(py)),
                            },
                        )?;
                        elements.append(cv.into_any())?;
                    }
                    _ => {}
                }
            }
            let cv = Py::new(
                py,
                ClassVariables {
                    lineno: lno,
                    modifiers: modifiers.into(),
                    nodes: elements.into(),
                },
            )?;
            Some(cv.into_any())
        }
        "try_statement" => {
            let body_n = field_child(&node, "body");
            let body_items = PyList::empty(py);
            if let Some(bn) = body_n {
                if let Some(obj) = translate(bn, source, py)? {
                    if let Ok(seq) = obj.cast_bound::<PyList>(py) {
                        for i in 0..seq.len() {
                            body_items.append(seq.get_item(i)?)?;
                        }
                    } else {
                        body_items.append(obj)?;
                    }
                }
            }
            let catches = PyList::empty(py);
            let mut finally_block: Option<Py<PyAny>> = None;
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "catch_clause" {
                    if let Some(obj) = translate(child, source, py)? {
                        catches.append(obj)?;
                    }
                } else if child.kind() == "finally_clause" {
                    if let Some(obj) = translate(child, source, py)? {
                        finally_block = Some(obj);
                    }
                }
            }
            let t = Py::new(
                py,
                TryNode {
                    lineno: lno,
                    nodes: body_items.into(),
                    catches: catches.into(),
                    finally: finally_block.unwrap_or_else(|| make_none(py)),
                },
            )?;
            Some(t.into_any())
        }
        "catch_clause" => {
            let type_name = field_child(&node, "type")
                .map(|n| make_str(py, text_of(&n, source)))
                .unwrap_or_else(|| make_str(py, ""));
            let var =
                field_child(&node, "name").and_then(|n| translate(n, source, py).ok().flatten());
            let body_n = field_child(&node, "body");
            let body = PyList::empty(py);
            if let Some(bn) = body_n {
                if let Some(obj) = translate(bn, source, py)? {
                    if let Ok(seq) = obj.cast_bound::<PyList>(py) {
                        for i in 0..seq.len() {
                            body.append(seq.get_item(i)?)?;
                        }
                    } else {
                        body.append(obj)?;
                    }
                }
            }
            let c = Py::new(
                py,
                CatchNode {
                    lineno: lno,
                    class_: type_name,
                    var: var.unwrap_or_else(|| make_none(py)),
                    nodes: body.into(),
                },
            )?;
            Some(c.into_any())
        }
        "finally_clause" => {
            let body_n = field_child(&node, "body");
            let body = PyList::empty(py);
            if let Some(bn) = body_n {
                if let Some(obj) = translate(bn, source, py)? {
                    if let Ok(seq) = obj.cast_bound::<PyList>(py) {
                        for i in 0..seq.len() {
                            body.append(seq.get_item(i)?)?;
                        }
                    } else {
                        body.append(obj)?;
                    }
                }
            }
            let f = Py::new(
                py,
                FinallyNode {
                    lineno: lno,
                    nodes: body.into(),
                },
            )?;
            Some(f.into_any())
        }

        // ── text / shell ─────────────────────────────────────────
        "text_interpolation" => {
            let txt = text_of(&node, source);
            if txt.is_empty() {
                None
            } else {
                let ih = Py::new(
                    py,
                    InlineHTML {
                        lineno: lno,
                        data: make_str(py, txt),
                    },
                )?;
                Some(ih.into_any())
            }
        }
        "shell_command_expression" => {
            let txt = text_of(&node, source);
            let p = Py::new(
                py,
                Parameter {
                    lineno: lno,
                    node: make_str(py, txt),
                    is_ref: make_bool(py, false),
                },
            )?;
            let params = PyList::empty(py);
            params.append(p.into_any())?;
            let fc = Py::new(
                py,
                FunctionCall {
                    lineno: lno,
                    name: make_str(py, "shell_exec"),
                    params: params.into(),
                },
            )?;
            Some(fc.into_any())
        }

        // ── dynamic variable ────────────────────────────────────
        "dynamic_variable_name" => {
            let inner = node.named_child(0);
            let name = if let Some(inr) = inner {
                if inr.kind() == "name" {
                    make_str(py, &format!("${}", text_of(&inr, source)))
                } else if inr.kind() == "variable_name" {
                    translate(inr, source, py)?.unwrap_or_else(|| make_none(py))
                } else {
                    translate(inr, source, py)?.unwrap_or_else(|| make_none(py))
                }
            } else {
                make_none(py)
            };
            let v = Py::new(py, Variable { lineno: lno, name })?;
            Some(v.into_any())
        }
        "array_element_initializer" => {
            let last = node.named_child(node.named_child_count().saturating_sub(1) as u32);
            last.and_then(|n| translate(n, source, py).ok().flatten())
        }

        // ── fallback ─────────────────────────────────────────────
        _ => {
            if node.named_child_count() > 0 {
                let list = PyList::empty(py);
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    if is_skip(child.kind()) {
                        continue;
                    }
                    if let Some(obj) = translate(child, source, py)? {
                        list.append(obj)?;
                    }
                }
                if list.len() > 0 {
                    Some(list.into())
                } else {
                    None
                }
            } else {
                None
            }
        }
    };

    Ok(result)
}

fn translate_echo(node: Node, source: &[u8], py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
    let items = PyList::empty(py);
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == "echo" {
            continue;
        }
        if let Some(obj) = translate(child, source, py)? {
            items.append(obj)?;
        }
    }
    let lno = lineno(&node);
    let echo = Py::new(
        py,
        Echo {
            lineno: lno,
            nodes: items.into(),
        },
    )?;
    Ok(Some(echo.into_any()))
}
