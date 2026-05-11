use pyo3::conversion::IntoPyObjectExt;
use pyo3::prelude::*;
use pyo3::types::PyList;
use pyo3::Py;
use tree_sitter::Node;

use crate::ast::*;

// ── context for tracking namespace/class/function ──────────────────

#[derive(Clone, Default)]
struct Ctx {
    namespace: Option<String>,
    function: Option<String>,
    class_: Option<String>,
    method: Option<String>,
    filename: Option<String>,
    string_mode: bool,
}

impl Ctx {
    fn resolve_magic(&self, txt: &str) -> Option<String> {
        match txt {
            "__FUNCTION__" => self.function.clone(),
            "__METHOD__" => self.method.clone(),
            "__CLASS__" => self.class_.clone(),
            "__NAMESPACE__" => self.namespace.clone(),
            "__FILE__" => self.filename.clone(),
            "__DIR__" => self
                .filename
                .as_ref()
                .and_then(|f| std::path::Path::new(f).parent())
                .map(|p| p.to_string_lossy().to_string()),
            "__TRAIT__" => None,
            _ => None,
        }
    }

    fn qualify_name(&self, name: &str) -> String {
        if let Some(ref ns) = self.namespace {
            if !ns.is_empty() {
                return format!("{}\\{}", ns, name);
            }
        }
        name.to_string()
    }
}

// ── helpers ──────────────────────────────────────────────────────────

fn lineno(node: &Node) -> Option<usize> {
    Some(node.start_position().row + 1)
}

fn text_of<'a>(node: &Node, source: &'a [u8]) -> &'a str {
    node.utf8_text(source).unwrap_or("")
}

fn unescape_string(s: &str, is_double: bool) -> String {
    if !s.contains('\\') {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len();
    if !is_double {
        let mut res = String::with_capacity(s.len());
        let mut i = 0;
        while i < len {
            if chars[i] == '\\' && i + 1 < len {
                let nxt = chars[i + 1];
                if nxt == '\\' {
                    res.push('\\');
                } else if nxt == '\'' {
                    res.push('\'');
                } else {
                    res.push('\\');
                    res.push(nxt);
                }
                i += 2;
            } else {
                res.push(chars[i]);
                i += 1;
            }
        }
        res
    } else {
        let mut res = String::with_capacity(s.len());
        let mut i = 0;
        while i < len {
            if chars[i] == '\\' && i + 1 < len {
                let nxt = chars[i + 1];
                match nxt {
                    'n' => res.push('\n'),
                    'r' => res.push('\r'),
                    't' => res.push('\t'),
                    'v' => res.push('\x0b'),
                    'e' => res.push('\x1b'),
                    'f' => res.push('\x0c'),
                    '\\' => res.push('\\'),
                    '"' => res.push('"'),
                    '$' => res.push('$'),
                    'x' => {
                        if i + 3 < len {
                            let hex: String = chars[i + 2..i + 4].iter().collect();
                            if let Ok(code) = u32::from_str_radix(&hex, 16) {
                                if let Some(c) = char::from_u32(code) {
                                    res.push(c);
                                    i += 4;
                                    continue;
                                }
                            }
                        }
                        res.push_str("\\x");
                        i += 2;
                        continue;
                    }
                    d if d.is_ascii_digit() => {
                        let mut oct = String::new();
                        let mut j = 0;
                        while i + 1 + j < len && j < 3 && chars[i + 1 + j].is_ascii_digit() {
                            oct.push(chars[i + 1 + j]);
                            j += 1;
                        }
                        if let Ok(code) = u32::from_str_radix(&oct, 8) {
                            if let Some(c) = char::from_u32(code) {
                                res.push(c);
                                i += 1 + j;
                                continue;
                            }
                        }
                        res.push(nxt);
                        i += 2;
                        continue;
                    }
                    _ => {
                        res.push(nxt);
                    }
                }
                i += 2;
            } else {
                res.push(chars[i]);
                i += 1;
            }
        }
        res
    }
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

fn process_encapsed_parts(
    node: &Node,
    source: &[u8],
    py: Python<'_>,
    is_double: bool,
    ctx: &mut Ctx,
) -> PyResult<Py<PyAny>> {
    let mut parts: Vec<Py<PyAny>> = Vec::new();
    let prev_string_mode = ctx.string_mode;
    ctx.string_mode = true;
    process_string_children(node, source, py, is_double, &mut parts, &mut None, ctx)?;
    ctx.string_mode = prev_string_mode;
    build_string_result(py, parts)
}

fn process_string_children(
    node: &Node,
    source: &[u8],
    py: Python<'_>,
    is_double: bool,
    parts: &mut Vec<Py<PyAny>>,
    prev_end: &mut Option<usize>,
    ctx: &mut Ctx,
) -> PyResult<()> {
    let child_count = node.child_count() as u32;
    let mut i: u32 = 0;
    while i < child_count {
        if let Some(child) = node.child(i) {
            let ck = child.kind();
            match ck {
                "string_content" | "escape_sequence" | "heredoc_content" | "nowdoc_content"
                | "nowdoc_string" => {
                    let start = prev_end.unwrap_or_else(|| child.start_byte());
                    let end = child.end_byte();
                    if start < end {
                        let txt = std::str::from_utf8(&source[start..end]).unwrap_or("");
                        let unescaped = unescape_string(txt, is_double);
                        if !unescaped.is_empty() || ck == "string_content" {
                            let merged = parts
                                .last()
                                .and_then(|p| p.bind(py).extract::<String>().ok())
                                .map(|s| s + &unescaped);
                            if let Some(s) = merged {
                                parts.pop();
                                parts.push(make_str(py, &s));
                            } else {
                                parts.push(make_str(py, &unescaped));
                            }
                        }
                    }
                    *prev_end = Some(end);
                }
                "heredoc_body" | "nowdoc_body" => {
                    process_string_children(&child, source, py, is_double, parts, prev_end, ctx)?;
                }
                "{" => {
                    // Check for { expr } triplet
                    if i + 2 < child_count {
                        if let (Some(inner), Some(close)) = (node.child(i + 1), node.child(i + 2)) {
                            if close.kind() == "}" {
                                if inner.is_named() {
                                    let res = translate_with_ctx(inner, source, py, ctx)?
                                        .unwrap_or_else(|| make_none(py));
                                    if inner.kind() == "dynamic_variable_name" {
                                        let v = Py::new(
                                            py,
                                            Variable {
                                                lineno: lineno(&inner),
                                                name: res,
                                            },
                                        )?;
                                        parts.push(v.into_any());
                                    } else {
                                        parts.push(res);
                                    }
                                }
                                *prev_end = Some(close.end_byte());
                                i += 3;
                                continue;
                            }
                        }
                    }
                }
                "}" => {
                    // standalone } - skip
                }
                "ERROR" => {
                    let start = prev_end.unwrap_or_else(|| child.start_byte());
                    let end = child.end_byte();
                    if start < end {
                        let txt = std::str::from_utf8(&source[start..end]).unwrap_or("");
                        let unescaped = unescape_string(txt, is_double);
                        if !unescaped.is_empty() {
                            let merged = parts
                                .last()
                                .and_then(|p| p.bind(py).extract::<String>().ok())
                                .map(|s| s + &unescaped);
                            if let Some(s) = merged {
                                parts.pop();
                                parts.push(make_str(py, &s));
                            } else {
                                parts.push(make_str(py, &unescaped));
                            }
                        }
                    }
                    *prev_end = Some(end);
                }
                _ if child.is_named()
                    && !is_skip(ck)
                    && !matches!(
                        ck,
                        "heredoc_start"
                            | "heredoc_end"
                            | "nowdoc_start"
                            | "nowdoc_end"
                            | "shell_command_start"
                            | "shell_command_end"
                    ) =>
                {
                    if let Some(obj) = translate_with_ctx(child, source, py, ctx)? {
                        parts.push(obj);
                    }
                    *prev_end = Some(child.end_byte());
                }
                _ => {
                    // skip without updating prev_end
                }
            }
        }
        i += 1;
    }
    Ok(())
}

fn build_string_result(py: Python<'_>, mut parts: Vec<Py<PyAny>>) -> PyResult<Py<PyAny>> {
    if parts.is_empty() {
        return Ok(make_str(py, ""));
    }
    if parts.len() == 1 {
        return Ok(parts.remove(0));
    }
    let all_scalar = parts.iter().all(|p| {
        let b = p.bind(py);
        b.extract::<String>().is_ok() || b.extract::<i64>().is_ok() || b.extract::<f64>().is_ok()
    });
    if all_scalar {
        let combined: String = parts
            .iter()
            .map(|p| {
                let b = p.bind(py);
                if let Ok(s) = b.extract::<String>() {
                    s
                } else if let Ok(v) = b.extract::<i64>() {
                    v.to_string()
                } else if let Ok(v) = b.extract::<f64>() {
                    v.to_string()
                } else {
                    String::new()
                }
            })
            .collect();
        return Ok(make_str(py, &combined));
    }
    let mut result = parts.remove(0);
    for part in parts {
        result = Py::new(
            py,
            BinaryOp {
                lineno: None,
                op: make_str(py, "."),
                left: result,
                right: part,
            },
        )?
        .into_any();
    }
    Ok(result)
}

// ── dv_translate ──────────────────────────────────────────────────
// Dynamic variable context translation — names become Variables
fn dv_translate(
    node: Node,
    source: &[u8],
    py: Python<'_>,
    ctx: &mut Ctx,
) -> PyResult<Option<Py<PyAny>>> {
    let kind = node.kind();
    let lno = lineno(&node);
    match kind {
        "name" => {
            let v = Py::new(
                py,
                Variable {
                    lineno: lno,
                    name: make_str(py, &format!("${}", text_of(&node, source))),
                },
            )?;
            Ok(Some(v.into_any()))
        }
        "subscript_expression" => {
            let obj = node
                .named_child(0)
                .and_then(|n| dv_translate(n, source, py, ctx).ok().flatten())
                .unwrap_or_else(|| make_none(py));
            let idx_node = node.named_child(1);
            let idx_is_name = idx_node.is_some_and(|n| {
                let k = n.kind();
                k == "name"
                    || k == "qualified_name"
                    || k == "fully_qualified_name"
                    || k == "relative_name"
            });
            let idx = idx_node
                .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten())
                .unwrap_or_else(|| make_none(py));
            let idx = if idx_is_name {
                idx.bind(py)
                    .getattr("name")
                    .map(|n| n.into())
                    .unwrap_or(idx)
            } else {
                idx
            };
            let a = Py::new(
                py,
                ArrayOffset {
                    lineno: lno,
                    node: obj,
                    expr: idx,
                },
            )?;
            Ok(Some(a.into_any()))
        }
        "member_access_expression" => {
            let obj = field_child(&node, "object")
                .and_then(|n| dv_translate(n, source, py, ctx).ok().flatten())
                .unwrap_or_else(|| make_none(py));
            let nm = field_child(&node, "name")
                .map(|n| make_str(py, text_of(&n, source)))
                .unwrap_or_else(|| make_str(py, ""));
            let op = Py::new(
                py,
                ObjectProperty {
                    lineno: lno,
                    node: obj,
                    name: nm,
                },
            )?;
            Ok(Some(op.into_any()))
        }
        "scoped_property_access_expression" => {
            let scope = field_child(&node, "scope")
                .map(|n| make_str(py, text_of(&n, source)))
                .unwrap_or_else(|| make_str(py, ""));
            let name_n = field_child(&node, "name");
            let name = if let Some(nn) = name_n {
                if nn.kind() == "variable_name" {
                    translate_with_ctx(nn, source, py, ctx)
                        .ok()
                        .and_then(|o| o)
                        .unwrap_or_else(|| make_none(py))
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
            Ok(Some(sp.into_any()))
        }
        "class_constant_access_expression" => {
            let scope = node
                .named_child(0)
                .map(|n| make_str(py, text_of(&n, source)))
                .unwrap_or_else(|| make_str(py, ""));
            let nm = node
                .named_child(1)
                .map(|n| make_str(py, text_of(&n, source)))
                .unwrap_or_else(|| make_str(py, ""));
            let sp = Py::new(
                py,
                StaticProperty {
                    lineno: lno,
                    node: scope,
                    name: nm,
                },
            )?;
            Ok(Some(sp.into_any()))
        }
        "member_call_expression" => {
            let obj = field_child(&node, "object")
                .and_then(|n| dv_translate(n, source, py, ctx).ok().flatten())
                .unwrap_or_else(|| make_none(py));
            let nm = field_child(&node, "name")
                .map(|n| make_str(py, text_of(&n, source)))
                .unwrap_or_else(|| make_str(py, ""));
            let args_node = field_child(&node, "arguments");
            let args = args_node
                .map(|a| {
                    let list = PyList::empty(py);
                    let mut cursor = a.walk();
                    for c in a.named_children(&mut cursor) {
                        if let Some(o) = translate_with_ctx(c, source, py, ctx).unwrap_or(None) {
                            list.append(o).ok();
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
                    name: nm,
                    params: args,
                },
            )?;
            Ok(Some(mc.into_any()))
        }
        "function_call_expression" => {
            let fname = field_child(&node, "function")
                .map(|n| make_str(py, text_of(&n, source)))
                .unwrap_or_else(|| make_str(py, ""));
            let args_node = field_child(&node, "arguments");
            let args = args_node
                .map(|a| {
                    let list = PyList::empty(py);
                    let mut cursor = a.walk();
                    for c in a.named_children(&mut cursor) {
                        if let Some(o) = translate_with_ctx(c, source, py, ctx).unwrap_or(None) {
                            list.append(o).ok();
                        }
                    }
                    list.into()
                })
                .unwrap_or_else(|| PyList::empty(py).into());
            let fc = Py::new(
                py,
                FunctionCall {
                    lineno: lno,
                    name: fname,
                    params: args,
                },
            )?;
            Ok(Some(fc.into_any()))
        }
        _ => translate_with_ctx(node, source, py, ctx),
    }
}

fn unwrap_block_body(py: Python<'_>, obj: Py<PyAny>) -> Py<PyAny> {
    obj.bind(py)
        .getattr("nodes")
        .map(|n| n.into())
        .unwrap_or(obj)
}

fn append_unwrapped(py: Python<'_>, list: &Bound<'_, PyList>, obj: Py<PyAny>) -> PyResult<()> {
    let nodz = unwrap_block_body(py, obj);
    if let Ok(seq) = nodz.bind(py).cast::<PyList>() {
        for i in 0..seq.len() {
            list.append(seq.get_item(i)?)?;
        }
    } else {
        list.append(nodz)?;
    }
    Ok(())
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

pub fn translate_root(
    root: Node,
    source: &[u8],
    py: Python<'_>,
    filename: Option<String>,
) -> PyResult<Py<PyAny>> {
    let list = PyList::empty(py);
    let mut echo_mode = false;
    let mut ctx = Ctx {
        filename,
        ..Ctx::default()
    };
    for i in 0..root.child_count() {
        if let Some(child) = root.child(i as u32) {
            process_program_child(child, source, py, &list, &mut echo_mode, &mut ctx)?;
        }
    }
    Ok(list.into())
}

fn process_program_child(
    child: Node,
    source: &[u8],
    py: Python<'_>,
    list: &Bound<'_, PyList>,
    echo_mode: &mut bool,
    ctx: &mut Ctx,
) -> PyResult<()> {
    let ckind = child.kind();
    if is_skip(ckind) {
        return Ok(());
    }
    let lno = lineno(&child);
    if ckind == "text" {
        let txt = text_of(&child, source);
        if !txt.is_empty() {
            if *echo_mode {
                let items = PyList::empty(py);
                items.append(make_str(py, txt))?;
                let echo = Py::new(
                    py,
                    Echo {
                        lineno: lno,
                        nodes: items.into(),
                    },
                )?;
                list.append(echo.into_any())?;
                *echo_mode = false;
            } else {
                let ih = Py::new(
                    py,
                    InlineHTML {
                        lineno: lno,
                        data: make_str(py, txt),
                    },
                )?;
                list.append(ih.into_any())?;
            }
        }
    } else if ckind == "php_tag" {
        let txt = text_of(&child, source).trim();
        *echo_mode = txt == "<?=";
    } else if ckind == "text_interpolation" {
        let mut ti_prev_end: Option<usize> = None;
        for j in 0..child.child_count() {
            if let Some(ti_child) = child.child(j as u32) {
                let tik = ti_child.kind();
                if tik == "text" {
                    let start = if let Some(prev) = ti_prev_end {
                        prev
                    } else {
                        ti_child.start_byte()
                    };
                    let end = ti_child.end_byte();
                    let txt = if start < end && start < source.len() && end <= source.len() {
                        std::str::from_utf8(&source[start..end]).unwrap_or("")
                    } else {
                        text_of(&ti_child, source)
                    };
                    if !txt.is_empty() {
                        let ih = Py::new(
                            py,
                            InlineHTML {
                                lineno: lno,
                                data: make_str(py, txt),
                            },
                        )?;
                        list.append(ih.into_any())?;
                    }
                } else if tik == "php_tag" {
                    let txt = text_of(&ti_child, source).trim();
                    *echo_mode = txt == "<?=";
                } else if tik == "php_end_tag" {
                    ti_prev_end = Some(ti_child.end_byte());
                } else if !is_skip(tik) {
                    if let Some(res) = translate_with_ctx(ti_child, source, py, ctx)? {
                        emit_result(res, py, list, echo_mode, lno)?;
                    }
                }
            }
        }
    } else if ckind == "php_end_tag" {
        // skip
    } else if let Some(obj) = translate_with_ctx(child, source, py, ctx)? {
        emit_result(obj, py, list, echo_mode, lno)?;
    }
    Ok(())
}

fn emit_result(
    obj: Py<PyAny>,
    py: Python<'_>,
    list: &Bound<'_, PyList>,
    echo_mode: &mut bool,
    lno: Option<usize>,
) -> PyResult<()> {
    if *echo_mode {
        let items = PyList::empty(py);
        items.append(obj)?;
        let echo = Py::new(
            py,
            Echo {
                lineno: lno,
                nodes: items.into(),
            },
        )?;
        list.append(echo.into_any())?;
        *echo_mode = false;
    } else if let Ok(seq) = obj.cast_bound::<PyList>(py) {
        for k in 0..seq.len() {
            list.append(seq.get_item(k)?)?;
        }
    } else {
        list.append(obj)?;
    }
    Ok(())
}

fn translate_with_ctx(
    node: Node,
    source: &[u8],
    py: Python<'_>,
    ctx: &mut Ctx,
) -> PyResult<Option<Py<PyAny>>> {
    let kind = node.kind();
    if is_skip(kind) {
        return Ok(None);
    }
    let lno = lineno(&node);

    let result: Option<Py<PyAny>> = match kind {
        // ── program / namespace / compound ──────────────────────
        "program" | "namespace_definition" | "declaration_list" => {
            let list = PyList::empty(py);
            let mut echo_mode = false;
            let ns_name = if kind == "namespace_definition" {
                let name_node = field_child(&node, "name");
                name_node.map(|n| text_of(&n, source).to_string())
            } else {
                None
            };
            let old_ns = ctx.namespace.clone();
            if let Some(ref ns) = ns_name {
                ctx.namespace = Some(ns.clone());
            }
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i as u32) {
                    process_program_child(child, source, py, &list, &mut echo_mode, ctx)?;
                }
            }
            if kind == "namespace_definition" {
                let has_decl_list = (0..node.child_count()).any(|i| {
                    node.child(i as u32)
                        .is_some_and(|c| c.kind() == "declaration_list")
                });
                if has_decl_list {
                    ctx.namespace = old_ns;
                }
                let ns_nodes = if list.len() == 1 {
                    let item: Py<PyAny> = list.get_item(0)?.into();
                    let cls_name = item
                        .bind(py)
                        .getattr("__class__")
                        .and_then(|c| c.getattr("__name__"))
                        .and_then(|n| n.extract::<String>())
                        .unwrap_or_default();
                    if cls_name == "Block" {
                        item.bind(py)
                            .getattr("nodes")
                            .map(|n| n.into())
                            .unwrap_or_else(|_| list.into())
                    } else {
                        list.into()
                    }
                } else {
                    list.into()
                };
                let n = Py::new(
                    py,
                    Namespace {
                        lineno: lno,
                        name: ns_name
                            .map(|n| make_str(py, &n))
                            .unwrap_or_else(|| make_none(py)),
                        nodes: ns_nodes,
                    },
                )?;
                Some(n.into_any())
            } else {
                Some(list.into())
            }
        }
        "compound_statement" | "colon_block" => {
            let list = PyList::empty(py);
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                if child.kind() == "{" || child.kind() == "}" {
                    continue;
                }
                if let Some(obj) = translate_with_ctx(child, source, py, ctx)? {
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
            .and_then(|c| translate_with_ctx(c, source, py, ctx).transpose())
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
        "string" => {
            let is_double = text_of(&node, source).trim_start().starts_with('"');
            Some(process_encapsed_parts(&node, source, py, is_double, ctx)?)
        }
        "encapsed_string" => {
            let is_double = text_of(&node, source).trim_start().starts_with('"');
            Some(process_encapsed_parts(&node, source, py, is_double, ctx)?)
        }
        "heredoc" => Some(process_encapsed_parts(&node, source, py, true, ctx)?),
        "nowdoc" => Some(process_encapsed_parts(&node, source, py, false, ctx)?),
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
                .and_then(|c| translate_with_ctx(c, source, py, ctx).ok().flatten())
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
            if lower == "die" {
                let e = Py::new(
                    py,
                    Exit {
                        lineno: lno,
                        expr: make_none(py),
                        type_: make_str(py, "die"),
                    },
                )?;
                Some(e.into_any())
            } else if lower == "static" || lower == "self" || lower == "parent" {
                Some(make_str(py, &lower))
            } else if txt.len() > 2
                && txt.starts_with("__")
                && txt.ends_with("__")
                && txt.chars().all(|c| c == '_' || c.is_ascii_uppercase())
            {
                let value = if txt == "__LINE__" {
                    lno.map(|ln| make_int(py, ln as i64))
                } else {
                    ctx.resolve_magic(txt).map(|v| make_str(py, &v))
                };
                let mc = Py::new(
                    py,
                    MagicConstant {
                        lineno: lno,
                        name: make_str(py, txt),
                        value: value.unwrap_or_else(|| make_none(py)),
                    },
                )?;
                Some(mc.into_any())
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
                let key =
                    key_node.and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten());
                let val = val_node
                    .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten())
                    .or_else(|| {
                        child
                            .named_child(0)
                            .and_then(|c| translate_with_ctx(c, source, py, ctx).ok().flatten())
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
                if let Some(obj) = translate_with_ctx(child, source, py, ctx)? {
                    items.append(obj)?;
                }
            }
            Some(items.into())
        }

        // ── operators ───────────────────────────────────────────
        "assignment_expression" | "augmented_assignment_expression" => {
            let left_node = field_child(&node, "left");
            let left_kind = left_node.map(|n| n.kind().to_string()).unwrap_or_default();
            let right_node = field_child(&node, "right");
            let left =
                left_node.and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten());
            let right =
                right_node.and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten());
            // Extract operator: text between left child end and right child start
            let op = if let (Some(ln), Some(rn)) = (left_node, right_node) {
                let left_end = ln.end_byte();
                let right_start = rn.start_byte();
                if right_start > left_end {
                    source[left_end..right_start]
                        .iter()
                        .map(|&b| b as char)
                        .collect::<String>()
                        .trim()
                        .to_string()
                } else {
                    String::new()
                }
            } else {
                String::new()
            };
            let has_amp = op.contains("&");
            let left_unwrap = left.unwrap_or_else(|| make_none(py));
            let right_unwrap = right.unwrap_or_else(|| make_none(py));
            if left_kind == "list_literal" {
                let la = Py::new(
                    py,
                    ListAssignment {
                        lineno: lno,
                        nodes: left_unwrap,
                        expr: right_unwrap,
                    },
                )?;
                return Ok(Some(la.into_any()));
            }
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
                let final_op = if op.ends_with('=') {
                    op.clone()
                } else {
                    format!("{op}=")
                };
                let a = Py::new(
                    py,
                    AssignOp {
                        lineno: lno,
                        op: make_str(py, &final_op),
                        left: left_unwrap,
                        right: right_unwrap,
                    },
                )?;
                Some(a.into_any())
            }
        }
        "binary_expression" => {
            let left = field_child(&node, "left")
                .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten())
                .unwrap_or_else(|| make_none(py));
            let right = field_child(&node, "right")
                .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten())
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
                .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten())
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
                .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten());
            let body = field_child(&node, "body")
                .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten());
            let alt = field_child(&node, "alternative")
                .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten());
            let iftrue = body
                .or_else(|| {
                    field_child(&node, "condition")
                        .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten())
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
                .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten())
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
                .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten())
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
                .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten())
                .unwrap_or_else(|| make_none(py));
            let right = node
                .named_child(1)
                .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten())
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
                .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten())
                .unwrap_or_else(|| make_none(py));
            let name_n = field_child(&node, "name");
            let name = if let Some(nn) = name_n {
                if nn.kind() == "variable_name" || nn.kind() == "variable_variable" {
                    translate_with_ctx(nn, source, py, ctx)?.unwrap_or_else(|| make_none(py))
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
            let obj_node = node.named_child(0);
            let obj = obj_node
                .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten())
                .unwrap_or_else(|| make_none(py));
            let idx_node = node.named_child(1);
            let idx_is_name = idx_node.is_some_and(|n| {
                let k = n.kind();
                k == "name"
                    || k == "qualified_name"
                    || k == "fully_qualified_name"
                    || k == "relative_name"
            });
            let idx = idx_node
                .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten())
                .unwrap_or_else(|| make_none(py));
            let idx = if idx_is_name {
                idx.bind(py)
                    .getattr("name")
                    .map(|n| n.into())
                    .unwrap_or(idx)
            } else {
                idx
            };
            // Check if obj_node is member_access_expression
            if let Some(on) = obj_node {
                if on.kind() == "member_access_expression" {
                    let name_n = field_child(&on, "name");
                    if let Some(nn) = name_n {
                        if nn.kind() == "variable_name" {
                            let obj_part = field_child(&on, "object")
                                .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten())
                                .unwrap_or_else(|| make_none(py));
                            let name_part = translate_with_ctx(nn, source, py, ctx)
                                .ok()
                                .and_then(|o| o)
                                .unwrap_or_else(|| make_none(py));
                            let new_name = Py::new(
                                py,
                                ArrayOffset {
                                    lineno: lno,
                                    node: name_part,
                                    expr: idx.clone_ref(py),
                                },
                            )?
                            .into_any();
                            let op = Py::new(
                                py,
                                ObjectProperty {
                                    lineno: lno,
                                    node: obj_part,
                                    name: new_name,
                                },
                            )?;
                            return Ok(Some(op.into_any()));
                        }
                    }
                }
            }
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
                .map(|n| {
                    let kind = n.kind();
                    let txt = text_of(&n, source);
                    if kind == "variable_name"
                        || kind == "dynamic_variable_name"
                        || txt.starts_with('$')
                    {
                        if txt.starts_with('$') {
                            Py::new(
                                py,
                                Variable {
                                    lineno: lno,
                                    name: make_str(py, txt),
                                },
                            )
                            .map(|v| v.into_any())
                            .ok()
                            .unwrap_or_else(|| make_none(py))
                        } else {
                            translate_with_ctx(n, source, py, ctx)
                                .ok()
                                .and_then(|o| o)
                                .unwrap_or_else(|| make_none(py))
                        }
                    } else {
                        let lower = txt.to_lowercase();
                        if lower == "static" || lower == "self" || lower == "parent" {
                            make_str(py, &lower)
                        } else {
                            make_str(py, txt)
                        }
                    }
                })
                .unwrap_or_else(|| make_str(py, ""));
            let name_n = field_child(&node, "name");
            let name = if let Some(nn) = name_n {
                let kind = nn.kind();
                let txt = text_of(&nn, source);
                if kind == "variable_name" || kind == "variable_variable" || txt.starts_with('$') {
                    if txt.starts_with('$') || txt.starts_with('{') {
                        Py::new(
                            py,
                            Variable {
                                lineno: lno,
                                name: make_str(py, txt),
                            },
                        )
                        .map(|v| v.into_any())
                        .ok()
                        .unwrap_or_else(|| make_none(py))
                    } else {
                        translate_with_ctx(nn, source, py, ctx)
                            .ok()
                            .and_then(|o| o)
                            .unwrap_or_else(|| make_none(py))
                    }
                } else {
                    make_str(py, txt)
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
                .map(|n| {
                    let kind = n.kind();
                    let txt = text_of(&n, source);
                    if kind == "variable_name" || txt.starts_with('$') {
                        translate_with_ctx(n, source, py, ctx)
                            .ok()
                            .and_then(|o| o)
                            .unwrap_or_else(|| make_none(py))
                    } else {
                        let lower = txt.to_lowercase();
                        if lower == "static" || lower == "self" || lower == "parent" {
                            make_str(py, &lower)
                        } else {
                            make_str(py, txt)
                        }
                    }
                })
                .unwrap_or_else(|| make_str(py, ""));
            let name_n = node.named_child(1);
            let name_text = name_n
                .map(|n| text_of(&n, source).to_string())
                .unwrap_or_default();
            if name_text == "class" {
                let scope_text = scope_n
                    .map(|n| text_of(&n, source).to_string())
                    .unwrap_or_default();
                Some(make_str(py, &scope_text))
            } else {
                let name = if let Some(nn) = name_n {
                    if nn.kind() == "name" && nn.named_child_count() > 0 {
                        let inner = nn.named_child(0).unwrap_or(nn);
                        if inner.kind() == "variable_name" {
                            translate_with_ctx(inner, source, py, ctx)
                                .ok()
                                .and_then(|o| o)
                                .unwrap_or_else(|| make_str(py, text_of(&nn, source)))
                        } else {
                            make_str(py, text_of(&nn, source))
                        }
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
        }
        "function_call_expression" => {
            let fn_node = field_child(&node, "function");
            let fn_val = fn_node
                .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten())
                .unwrap_or_else(|| make_none(py));
            let fn_text_opt =
                fn_node.map(|n| text_of(&n, source).trim().to_lowercase().to_string());
            let args_node = field_child(&node, "arguments");
            let args: Py<PyAny> = args_node
                .map(|a| {
                    let list = PyList::empty(py);
                    let mut cursor = a.walk();
                    for child in a.named_children(&mut cursor) {
                        if let Some(obj) =
                            translate_with_ctx(child, source, py, ctx).unwrap_or(None)
                        {
                            list.append(obj).ok();
                        }
                    }
                    list.into()
                })
                .unwrap_or_else(|| PyList::empty(py).into());

            let args_bound = args.bind(py);
            let args_list: Option<&Bound<'_, PyList>> = args_bound.cast::<PyList>().ok();
            let args_len = args_list.map(|l| l.len()).unwrap_or(0);

            match fn_text_opt.as_deref() {
                Some("die") | Some("exit") => {
                    let expr = if args_len > 0 {
                        let first = args_list.unwrap().get_item(0)?;
                        if let Ok(param) = first.cast::<Parameter>() {
                            param.borrow().node.clone_ref(py)
                        } else {
                            first.into()
                        }
                    } else {
                        make_none(py)
                    };
                    let e = Py::new(
                        py,
                        Exit {
                            lineno: lno,
                            expr,
                            type_: make_str(py, fn_text_opt.as_deref().unwrap_or("exit")),
                        },
                    )?;
                    return Ok(Some(e.into_any()));
                }
                Some("isset") => {
                    let vars = PyList::empty(py);
                    if let Some(list) = args_list {
                        for item in list.iter() {
                            if let Ok(param) = item.cast::<Parameter>() {
                                vars.append(param.borrow().node.clone_ref(py))?;
                            } else {
                                vars.append(item)?;
                            }
                        }
                    }
                    let s = Py::new(
                        py,
                        IsSet {
                            lineno: lno,
                            nodes: vars.into(),
                        },
                    )?;
                    return Ok(Some(s.into_any()));
                }
                Some("empty") => {
                    let expr = if args_len > 0 {
                        let first = args_list.unwrap().get_item(0)?;
                        if let Ok(param) = first.cast::<Parameter>() {
                            param.borrow().node.clone_ref(py)
                        } else {
                            first.into()
                        }
                    } else {
                        make_none(py)
                    };
                    let e = Py::new(py, Empty { lineno: lno, expr })?;
                    return Ok(Some(e.into_any()));
                }
                Some("eval") => {
                    let expr = if args_len > 0 {
                        let first = args_list.unwrap().get_item(0)?;
                        if let Ok(param) = first.cast::<Parameter>() {
                            param.borrow().node.clone_ref(py)
                        } else {
                            first.into()
                        }
                    } else {
                        make_none(py)
                    };
                    let ev = Py::new(py, Eval { lineno: lno, expr })?;
                    return Ok(Some(ev.into_any()));
                }
                _ => {}
            }

            let fn_is_name = fn_node.is_some_and(|n| {
                let k = n.kind();
                k == "name"
                    || k == "qualified_name"
                    || k == "fully_qualified_name"
                    || k == "relative_name"
            });
            let fn_name = if fn_val.is_none(py) {
                fn_node
                    .map(|n| make_str(py, text_of(&n, source)))
                    .unwrap_or_else(|| make_none(py))
            } else if fn_is_name {
                fn_val
                    .bind(py)
                    .getattr("name")
                    .map(|n| n.into())
                    .unwrap_or(fn_val)
            } else {
                fn_val
            };
            let fc = Py::new(
                py,
                FunctionCall {
                    lineno: lno,
                    name: fn_name,
                    params: args,
                },
            )?;
            Some(fc.into_any())
        }
        "scoped_call_expression" => {
            let scope_n = field_child(&node, "scope");
            let scope = scope_n
                .map(|n| {
                    let kind = n.kind();
                    if kind == "variable_name" || kind == "dynamic_variable_name" {
                        translate_with_ctx(n, source, py, ctx)
                            .ok()
                            .and_then(|o| o)
                            .unwrap_or_else(|| make_none(py))
                    } else {
                        let txt = text_of(&n, source);
                        let lower = txt.to_lowercase();
                        if lower == "static" || lower == "self" || lower == "parent" {
                            make_str(py, &lower)
                        } else {
                            make_str(py, txt)
                        }
                    }
                })
                .unwrap_or_else(|| make_str(py, ""));
            let name_n = field_child(&node, "name");
            let method_name = if let Some(nn) = name_n {
                if nn.kind() == "variable_name" || nn.kind() == "variable_variable" {
                    translate_with_ctx(nn, source, py, ctx)?.unwrap_or_else(|| make_none(py))
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
                        if let Some(obj) =
                            translate_with_ctx(child, source, py, ctx).unwrap_or(None)
                        {
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
                .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten())
                .unwrap_or_else(|| make_none(py));
            let name_n = field_child(&node, "name");
            let name = if let Some(nn) = name_n {
                if nn.kind() == "variable_name" || nn.kind() == "variable_variable" {
                    translate_with_ctx(nn, source, py, ctx)?.unwrap_or_else(|| make_none(py))
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
                        if let Some(obj) =
                            translate_with_ctx(child, source, py, ctx).unwrap_or(None)
                        {
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
                .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten())
                .unwrap_or_else(|| make_none(py));
            let name_n = field_child(&node, "name");
            let name = if let Some(nn) = name_n {
                if nn.kind() == "variable_name" {
                    translate_with_ctx(nn, source, py, ctx)?.unwrap_or_else(|| make_none(py))
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
                .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten())
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
                        if let Some(obj) =
                            translate_with_ctx(child, source, py, ctx).unwrap_or(None)
                        {
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
                        if let Some(obj) =
                            translate_with_ctx(child, source, py, ctx).unwrap_or(None)
                        {
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
                    .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten())
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
                    .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten())
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
                .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten());
            let body = field_child(&node, "body")
                .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten());
            let elseifs = PyList::empty(py);
            let mut else_node = None;
            let mut extra_body: Vec<Py<PyAny>> = Vec::new();
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                if child.kind() == "else_if_clause" {
                    let e_cond = field_child(&child, "condition")
                        .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten());
                    let e_body = field_child(&child, "body")
                        .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten());
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
                        .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten());
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
                } else if child.kind() == "text_interpolation" {
                    let res = translate_with_ctx(child, source, py, ctx)?;
                    extra_body.push(res.unwrap_or_else(|| make_none(py)));
                }
            }
            let i_body = if extra_body.is_empty() {
                body.unwrap_or_else(|| make_none(py))
            } else {
                let body_list = PyList::empty(py);
                if let Some(ref b) = body {
                    if let Ok(seq) = b.bind(py).cast::<PyList>() {
                        for k in 0..seq.len() {
                            body_list.append(seq.get_item(k)?)?;
                        }
                    } else {
                        let cls_name = b
                            .bind(py)
                            .getattr("__class__")
                            .and_then(|c| c.getattr("__name__"))
                            .and_then(|n| n.extract::<String>())
                            .unwrap_or_default();
                        if cls_name == "Block" {
                            let nodes = b.bind(py).getattr("nodes")?;
                            if let Ok(seq) = nodes.cast::<PyList>() {
                                for k in 0..seq.len() {
                                    body_list.append(seq.get_item(k)?)?;
                                }
                            }
                        } else {
                            body_list.append(b.clone_ref(py))?;
                        }
                    }
                }
                for eb in extra_body {
                    body_list.append(eb)?;
                }
                Py::new(
                    py,
                    Block {
                        lineno: None,
                        nodes: body_list.into(),
                    },
                )?
                .into_any()
            };
            let i = Py::new(
                py,
                If {
                    lineno: lno,
                    expr: cond.unwrap_or_else(|| make_none(py)),
                    node: i_body,
                    elseifs: elseifs.into(),
                    else_: else_node.unwrap_or_else(|| make_none(py)),
                },
            )?;
            Some(i.into_any())
        }
        "while_statement" => {
            let cond = node
                .named_child(0)
                .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten());
            let body = node
                .named_child(1)
                .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten())
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
                .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten())
                .unwrap_or_else(|| make_none(py));
            let cond = node
                .named_child(1)
                .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten());
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
                .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten());
            let test = node
                .named_child(1)
                .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten());
            let count = node
                .named_child(2)
                .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten());
            let body = node
                .named_child(3)
                .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten())
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
                .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten());
            let second = node.named_child(1);
            let (key, val_obj, _is_ref_val) = if let Some(sec) = second {
                if sec.kind() == "pair" {
                    let k = sec
                        .named_child(0)
                        .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten())
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
                        .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten())
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
                        .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten())
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
                    let v =
                        translate_with_ctx(sec, source, py, ctx)?.unwrap_or_else(|| make_none(py));
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
                .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten())
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
                .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten());
            let switch_block = node.named_child(1);
            let cases = switch_block
                .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten())
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
                if let Some(obj) = translate_with_ctx(child, source, py, ctx)? {
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
                if let Some(obj) = translate_with_ctx(child, source, py, ctx)? {
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
                    .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten())
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
                .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten());
            let body_node = field_child(&node, "body");
            let arms = body_node
                .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten())
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
                if let Some(obj) = translate_with_ctx(child, source, py, ctx)? {
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
                    if let Some(obj) = translate_with_ctx(child, source, py, ctx)? {
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
                .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten());
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
                .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten());
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
                .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten());
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
                .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten());
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
                .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten());
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
                .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten());
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
                expr = translate_with_ctx(child, source, py, ctx).ok().flatten();
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
                arg = translate_with_ctx(child, source, py, ctx).ok().flatten();
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
                .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten());
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
                .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten());
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
                if let Some(obj) = translate_with_ctx(child, source, py, ctx)? {
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
        "parenthesized_expression" => {
            translate_with_ctx(node.named_child(0).unwrap_or(node), source, py, ctx)?
        }
        "error_suppression_expression" => {
            let expr = node
                .named_child(0)
                .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten());
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
                .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten());
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
        "echo_statement" | "echo_stdout" => translate_echo(node, source, py, ctx)?,
        "global_statement" | "global_declaration" => {
            let vars = PyList::empty(py);
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                let t = text_of(&child, source).to_lowercase();
                if t == "global" {
                    continue;
                }
                if let Some(obj) = translate_with_ctx(child, source, py, ctx)? {
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
                        .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten())
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
                        .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten());
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
                } else if let Some(obj) = translate_with_ctx(child, source, py, ctx)? {
                    body_nodes.append(obj)?;
                }
            }
            let body = if body_nodes.is_empty() {
                make_none(py)
            } else if body_nodes.len() == 1 {
                let item: Py<PyAny> = body_nodes.get_item(0)?.into();
                let cls_name = item
                    .bind(py)
                    .getattr("__class__")
                    .and_then(|c| c.getattr("__name__"))
                    .and_then(|n| n.extract::<String>())
                    .unwrap_or_default();
                if cls_name == "Block" {
                    item
                } else {
                    let block = Py::new(
                        py,
                        Block {
                            lineno: lno,
                            nodes: body_nodes.into(),
                        },
                    )?;
                    block.into_any()
                }
            } else {
                let block = Py::new(
                    py,
                    Block {
                        lineno: lno,
                        nodes: body_nodes.into(),
                    },
                )?;
                block.into_any()
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
                    if let Some(obj) = translate_with_ctx(child, source, py, ctx)? {
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
                            if let Some(obj) = translate_with_ctx(sub, source, py, ctx)? {
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
            let original_child = node.named_child(0);
            let original = if let Some(n) = original_child {
                let kind = n.kind();
                let txt = text_of(&n, source);
                if kind == "scoped_property_access_expression" || kind == "scoped_call_expression" {
                    translate_with_ctx(n, source, py, ctx)?.unwrap_or_else(|| make_none(py))
                } else if let Some(pos) = txt.find("::") {
                    let sp = Py::new(
                        py,
                        StaticProperty {
                            lineno: lno,
                            node: make_str(py, &txt[..pos]),
                            name: make_str(py, &txt[pos + 2..]),
                        },
                    )?;
                    sp.into_any()
                } else {
                    make_str(py, txt)
                }
            } else {
                make_none(py)
            };
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
                    from_: original,
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
                    if let Some(obj) = translate_with_ctx(child, source, py, ctx)? {
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
            let fn_name_str = name.bind(py).extract::<String>().unwrap_or_default();
            let old_fn = ctx.function.clone();
            let full_name = ctx.qualify_name(&fn_name_str);
            ctx.function = Some(full_name);
            let params_node = field_child(&node, "parameters");
            let params = params_node
                .map(|pn| {
                    let list = PyList::empty(py);
                    let mut cursor = pn.walk();
                    for child in pn.named_children(&mut cursor) {
                        if let Some(obj) =
                            translate_with_ctx(child, source, py, ctx).unwrap_or(None)
                        {
                            list.append(obj).ok();
                        }
                    }
                    list.into()
                })
                .unwrap_or_else(|| PyList::empty(py).into());
            let body_block = field_child(&node, "body")
                .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten())
                .unwrap_or_else(|| make_none(py));
            let body = unwrap_block_body(py, body_block);
            ctx.function = old_fn;
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
                        if let Some(obj) = translate_with_ctx(p, source, py, ctx)? {
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
                    if let Some(bres) = translate_with_ctx(child, source, py, ctx)? {
                        let unwrapped = unwrap_block_body(py, bres);
                        if let Ok(list) = unwrapped.bind(py).cast::<PyList>() {
                            body_stmts = list.clone();
                        } else {
                            body_stmts = PyList::empty(py);
                            body_stmts.append(unwrapped)?;
                        }
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
            let default_val =
                default_node.and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten());
            let has_ref = (0..node.child_count()).any(|i| {
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
            let type_node = field_child(&node, "type");
            let type_name = type_node.map(|n| make_str(py, text_of(&n, source)));
            let mut name = String::new();
            let default_val: Option<Py<PyAny>>;
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                let kind = child.kind();
                if kind == "visibility_modifier" {
                    modifiers
                        .append(make_str(py, text_of(&child, source).trim()))
                        .ok();
                } else if kind == "variable_name" {
                    name = text_of(&child, source).to_string();
                }
            }
            let def_node = field_child(&node, "default_value");
            default_val =
                def_node.and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten());
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
            let method_name_str = name.bind(py).extract::<String>().unwrap_or_default();
            let old_method = ctx.method.clone();
            let old_fn = ctx.function.clone();
            if let Some(ref cls) = ctx.class_ {
                ctx.method = Some(format!("{}::{}", cls, method_name_str));
                ctx.function = Some(format!("{}::{}", cls, method_name_str));
            }
            let params_node = field_child(&node, "parameters");
            let params = params_node
                .map(|pn| {
                    let list = PyList::empty(py);
                    let mut c = pn.walk();
                    for child in pn.named_children(&mut c) {
                        if let Some(obj) =
                            translate_with_ctx(child, source, py, ctx).unwrap_or(None)
                        {
                            list.append(obj).ok();
                        }
                    }
                    list.into()
                })
                .unwrap_or_else(|| PyList::empty(py).into());
            let body_block = field_child(&node, "body")
                .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten())
                .unwrap_or_else(|| make_none(py));
            let body = unwrap_block_body(py, body_block);
            // If body is None but method expects empty list, return empty list
            let body = if body.bind(py).is_none() {
                PyList::empty(py).into()
            } else {
                body
            };
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
            ctx.method = old_method;
            ctx.function = old_fn;
            Some(m.into_any())
        }
        "class_declaration" => {
            let name = field_child(&node, "name")
                .map(|n| make_str(py, text_of(&n, source)))
                .unwrap_or_else(|| make_str(py, ""));
            let class_name_str = name.bind(py).extract::<String>().unwrap_or_default();
            let old_class = ctx.class_.clone();
            ctx.class_ = Some(ctx.qualify_name(&class_name_str));
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
                                    elem_node.named_child(1).and_then(|n| {
                                        translate_with_ctx(n, source, py, ctx).ok().flatten()
                                    })
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
                        if let Some(res) = translate_with_ctx(child, source, py, ctx)? {
                            uses.append(res)?;
                        }
                    } else if let Some(res) = translate_with_ctx(child, source, py, ctx)? {
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
            ctx.class_ = old_class;
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
                        if let Some(res) = translate_with_ctx(child, source, py, ctx)? {
                            uses.append(res)?;
                        }
                    } else if let Some(res) = translate_with_ctx(child, source, py, ctx)? {
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
                    if let Some(res) = translate_with_ctx(child, source, py, ctx)? {
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
                let val =
                    value_node.and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten());
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
                        let val = val_n
                            .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten());
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
                if let Some(obj) = translate_with_ctx(bn, source, py, ctx)? {
                    append_unwrapped(py, &body_items, obj)?;
                }
            }
            let catches = PyList::empty(py);
            let mut finally_block: Option<Py<PyAny>> = None;
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "catch_clause" {
                    if let Some(obj) = translate_with_ctx(child, source, py, ctx)? {
                        catches.append(obj)?;
                    }
                } else if child.kind() == "finally_clause" {
                    if let Some(obj) = translate_with_ctx(child, source, py, ctx)? {
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
                    finally_: finally_block.unwrap_or_else(|| make_none(py)),
                },
            )?;
            Some(t.into_any())
        }
        "catch_clause" => {
            let type_name = field_child(&node, "type")
                .map(|n| make_str(py, text_of(&n, source)))
                .unwrap_or_else(|| make_str(py, ""));
            let var = field_child(&node, "name")
                .and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten());
            let body_n = field_child(&node, "body");
            let body = PyList::empty(py);
            if let Some(bn) = body_n {
                if let Some(obj) = translate_with_ctx(bn, source, py, ctx)? {
                    append_unwrapped(py, &body, obj)?;
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
                if let Some(obj) = translate_with_ctx(bn, source, py, ctx)? {
                    append_unwrapped(py, &body, obj)?;
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
            let cleaned = txt
                .trim()
                .trim_start_matches("?>")
                .trim_end_matches("?>")
                .trim_start_matches("<?=")
                .trim_end_matches("<?=")
                .trim_start_matches("<?php")
                .trim_end_matches("<?php")
                .trim_start_matches("<?")
                .trim_end_matches("<?")
                .trim();
            if cleaned.is_empty()
                || cleaned == "?>"
                || cleaned == "<?php"
                || cleaned == "<?="
                || cleaned == "<?"
            {
                None
            } else {
                let ih = Py::new(
                    py,
                    InlineHTML {
                        lineno: lno,
                        data: make_str(py, cleaned),
                    },
                )?;
                Some(ih.into_any())
            }
        }
        "shell_command_expression" => {
            let shell_arg = process_encapsed_parts(&node, source, py, true, ctx)?;
            let p = Py::new(
                py,
                Parameter {
                    lineno: lno,
                    node: shell_arg,
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
            let result: Option<Py<PyAny>> = if let Some(inr) = inner {
                if inr.kind() == "name" {
                    let v = Py::new(
                        py,
                        Variable {
                            lineno: lno,
                            name: make_str(py, &format!("${}", text_of(&inr, source))),
                        },
                    )?;
                    Some(v.into_any())
                } else if inr.kind() == "dynamic_variable_name" {
                    let inner_val =
                        translate_with_ctx(inr, source, py, ctx)?.unwrap_or_else(|| make_none(py));
                    let v = Py::new(
                        py,
                        Variable {
                            lineno: lno,
                            name: inner_val,
                        },
                    )?;
                    Some(v.into_any())
                } else if inr.kind() == "variable_name" {
                    let inner_res =
                        translate_with_ctx(inr, source, py, ctx)?.unwrap_or_else(|| make_none(py));
                    if ctx.string_mode {
                        Some(inner_res)
                    } else {
                        let v = Py::new(
                            py,
                            Variable {
                                lineno: lno,
                                name: inner_res,
                            },
                        )?;
                        Some(v.into_any())
                    }
                } else {
                    // Other inner types: use dv_translate
                    let inner_res =
                        dv_translate(inr, source, py, ctx)?.unwrap_or_else(|| make_none(py));
                    if ctx.string_mode {
                        Some(inner_res)
                    } else {
                        let v = Py::new(
                            py,
                            Variable {
                                lineno: lno,
                                name: inner_res,
                            },
                        )?;
                        Some(v.into_any())
                    }
                }
            } else {
                None
            };
            result
        }
        "array_element_initializer" => {
            let last = node.named_child(node.named_child_count().saturating_sub(1) as u32);
            last.and_then(|n| translate_with_ctx(n, source, py, ctx).ok().flatten())
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
                    if let Some(obj) = translate_with_ctx(child, source, py, ctx)? {
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

fn translate_echo(
    node: Node,
    source: &[u8],
    py: Python<'_>,
    ctx: &mut Ctx,
) -> PyResult<Option<Py<PyAny>>> {
    let items = PyList::empty(py);
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == "echo" {
            continue;
        }
        if let Some(obj) = translate_with_ctx(child, source, py, ctx)? {
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
