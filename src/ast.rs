use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyString, PyTuple};

macro_rules! field_name_str {
    (type_) => { "type" };
    ($f:ident) => { stringify!($f) };
}

macro_rules! ast_node {
    ($pyname:literal, $name:ident { $($field:ident),* $(,)? }) => {
        #[pyclass(get_all, name = $pyname)]
        pub struct $name {
            pub lineno: Option<usize>,
            $(pub $field: Py<PyAny>),*
        }

        #[pymethods]
        impl $name {
            #[new]
            #[pyo3(signature = ($($field,)* lineno=None))]
            fn py_new(
                $($field: Py<PyAny>,)*
                lineno: Option<usize>,
            ) -> Self {
                Self { lineno, $($field),* }
            }

            #[classattr]
            fn fields() -> Vec<&'static str> {
                vec![$(field_name_str!($field)),*]
            }

            fn __repr__(slf: &Bound<'_, Self>) -> PyResult<String> {
                let this = slf.borrow();
                let mut fields: Vec<String> = Vec::new();
                $(
                    let v = this.$field.bind(slf.py());
                    fields.push(v.repr()?.to_string());
                )*
                Ok(format!("{}({})", $pyname, fields.join(", ")))
            }

            fn __eq__(slf: &Bound<'_, Self>, other: &Bound<'_, PyAny>) -> PyResult<bool> {
                let py = slf.py();
                let rhs = match other.cast::<Self>() {
                    Ok(r) => r,
                    Err(_) => return Ok(false),
                };
                let this = slf.borrow();
                let rhs = rhs.borrow();
                $(
                    if !this.$field.bind(py).eq(rhs.$field.bind(py))? {
                        return Ok(false);
                    }
                )*
                Ok(true)
            }

            fn __ne__(slf: &Bound<'_, Self>, other: &Bound<'_, PyAny>) -> PyResult<bool> {
                Ok(!Self::__eq__(slf, other)?)
            }

            fn accept(slf: &Bound<'_, Self>, visitor: &Bound<'_, PyAny>) -> PyResult<()> {
                let py = slf.py();
                let this = slf.borrow();
                visitor.call1((slf.clone(),))?;
                $(
                    let val = this.$field.bind(py);
                    if val.hasattr("accept")? {
                        val.call_method1("accept", (visitor,))?;
                    } else if let Ok(list) = val.cast::<PyList>() {
                        for item in list.iter() {
                            if item.hasattr("accept")? {
                                item.call_method1("accept", (visitor,))?;
                            }
                        }
                    }
                )*
                Ok(())
            }

            #[pyo3(signature = (with_lineno=None))]
            fn generic(slf: &Bound<'_, Self>, with_lineno: Option<bool>) -> PyResult<Py<PyAny>> {
                let py = slf.py();
                let this = slf.borrow();
                let dict = PyDict::new(py);
                if with_lineno.unwrap_or(false) {
                    if let Some(l) = this.lineno {
                        dict.set_item("lineno", l)?;
                    }
                }
                $(
                    let val = this.$field.bind(py);
                    if val.hasattr("generic")? {
                        let gen = val.call_method1("generic", (with_lineno,))?;
                        dict.set_item(field_name_str!($field), gen)?;
                    } else if let Ok(list) = val.cast::<PyList>() {
                        let new_list = PyList::empty(py);
                        for item in list.iter() {
                            if item.hasattr("generic")? {
                                let gen = item.call_method1("generic", (with_lineno,))?;
                                new_list.append(gen)?;
                            } else {
                                new_list.append(item)?;
                            }
                        }
                        dict.set_item(field_name_str!($field), new_list)?;
                    } else {
                        dict.set_item(field_name_str!($field), val)?;
                    }
                )*
                let class_name = PyString::new(py, $pyname);
                let items: Vec<Py<PyAny>> = vec![
                    class_name.into_any().unbind(),
                    dict.unbind().into(),
                ];
                Ok(PyTuple::new(py, items)?.into_any().unbind())
            }
        }
    };
}

// ── top-level ──
ast_node!("InlineHTML", InlineHTML { data });
ast_node!("Block", Block { nodes });
ast_node!("Namespace", Namespace { name, nodes });

// ── statements ──
ast_node!("Assignment", Assignment { node, expr, is_ref });
ast_node!("ListAssignment", ListAssignment { nodes, expr });
ast_node!("New", New { name, params });
ast_node!("Clone", CloneNode { node });
ast_node!("Break", BreakNode { node });
ast_node!("Continue", ContinueNode { node });
ast_node!("Return", ReturnNode { node });
ast_node!("Yield", YieldNode { node });
ast_node!("Global", Global { nodes });
ast_node!("Static", Static { nodes });
ast_node!("Echo", Echo { nodes });
ast_node!("Print", Print { node });
ast_node!("Unset", Unset { nodes });
ast_node!(
    "Try",
    TryNode {
        nodes,
        catches,
        finally
    }
);
ast_node!("Catch", CatchNode { class_, var, nodes });
ast_node!("Finally", FinallyNode { nodes });
ast_node!("Throw", Throw { node });
ast_node!("Declare", Declare { directives, node });
ast_node!("Directive", Directive { name, node });

// ── functions / closures ──
ast_node!(
    "Function",
    Function {
        name,
        params,
        nodes,
        is_ref,
        return_type
    }
);
ast_node!(
    "Method",
    Method {
        name,
        modifiers,
        params,
        nodes,
        is_ref
    }
);
ast_node!(
    "Closure",
    Closure {
        params,
        vars,
        nodes,
        is_ref
    }
);

// ── class / trait ──
ast_node!(
    "Class",
    Class {
        name,
        type_,
        extends,
        implements,
        traits,
        nodes
    }
);
ast_node!(
    "Trait",
    Trait {
        name,
        traits,
        nodes
    }
);
ast_node!("ClassConstants", ClassConstants { nodes });
ast_node!("ClassConstant", ClassConstant { name, initial });
ast_node!("ClassVariables", ClassVariables { modifiers, nodes });
ast_node!("ClassVariable", ClassVariable { name, initial });
ast_node!(
    "Interface",
    Interface {
        name,
        extends,
        nodes
    }
);

// ── operators ──
ast_node!("AssignOp", AssignOp { op, left, right });
ast_node!("BinaryOp", BinaryOp { op, left, right });
ast_node!("UnaryOp", UnaryOp { op, expr });
ast_node!(
    "TernaryOp",
    TernaryOp {
        expr,
        iftrue,
        iffalse
    }
);
ast_node!("PreIncDecOp", PreIncDecOp { op, expr });
ast_node!("PostIncDecOp", PostIncDecOp { op, expr });
ast_node!("Cast", Cast { type_, expr });

// ── expressions ──
ast_node!("IsSet", IsSet { nodes });
ast_node!("Empty", Empty { expr });
ast_node!("Eval", Eval { expr });
ast_node!("Include", Include { expr, once });
ast_node!("Require", Require { expr, once });
ast_node!("Exit", Exit { expr, type_ });
ast_node!("Silence", Silence { expr });
ast_node!("MagicConstant", MagicConstant { name, value });
ast_node!("Constant", Constant { name });
ast_node!("Variable", Variable { name });
ast_node!("StaticVariable", StaticVariable { name, initial });
ast_node!("LexicalVariable", LexicalVariable { name, is_ref });
ast_node!(
    "FormalParameter",
    FormalParameter {
        name,
        default,
        is_ref,
        type_
    }
);
ast_node!("Parameter", Parameter { node, is_ref });

// ── calls / access ──
ast_node!("FunctionCall", FunctionCall { name, params });
ast_node!("Array", Array { nodes });
ast_node!("ArrayElement", ArrayElement { key, value, is_ref });
ast_node!("ArrayOffset", ArrayOffset { node, expr });
ast_node!("StringOffset", StringOffset { node, expr });
ast_node!("ObjectProperty", ObjectProperty { node, name });
ast_node!("StaticProperty", StaticProperty { node, name });
ast_node!("MethodCall", MethodCall { node, name, params });
ast_node!(
    "StaticMethodCall",
    StaticMethodCall {
        class_,
        name,
        params
    }
);

// ── flow control ──
ast_node!(
    "If",
    If {
        expr,
        node,
        elseifs,
        else_
    }
);
ast_node!("ElseIf", ElseIf { expr, node });
ast_node!("Else", Else { node });
ast_node!("While", While { expr, node });
ast_node!("DoWhile", DoWhile { node, expr });
ast_node!(
    "For",
    For {
        start,
        test,
        count,
        node
    }
);
ast_node!(
    "Foreach",
    Foreach {
        expr,
        keyvar,
        valvar,
        node
    }
);
ast_node!("ForeachVariable", ForeachVariable { name, is_ref });
ast_node!("Switch", Switch { expr, nodes });
ast_node!("Case", Case { expr, nodes });
ast_node!("Default", Default { nodes });

// ── namespace / use ──
ast_node!("UseDeclarations", UseDeclarations { nodes });
ast_node!("UseDeclaration", UseDeclaration { name, alias });
ast_node!("ConstantDeclarations", ConstantDeclarations { nodes });
ast_node!("ConstantDeclaration", ConstantDeclaration { name, initial });
ast_node!("TraitUse", TraitUse { name, renames });
ast_node!(
    "TraitModifier",
    TraitModifier {
        from,
        to,
        visibility
    }
);

// ── PHP 8.0+ ──
ast_node!("MatchExpr", MatchExpr { condition, arms });
ast_node!("MatchArm", MatchArm { pattern, body });
ast_node!("NamedArgument", NamedArgument { name, node });
ast_node!(
    "NullsafePropertyAccess",
    NullsafePropertyAccess { node, name }
);
ast_node!("NullsafeCall", NullsafeCall { node, name, params });
ast_node!(
    "ConstructorParameter",
    ConstructorParameter {
        modifiers,
        name,
        type_,
        default
    }
);

pub fn register_types(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<InlineHTML>()?;
    m.add_class::<Block>()?;
    m.add_class::<Namespace>()?;
    m.add_class::<Assignment>()?;
    m.add_class::<ListAssignment>()?;
    m.add_class::<New>()?;
    m.add_class::<CloneNode>()?;
    m.add_class::<BreakNode>()?;
    m.add_class::<ContinueNode>()?;
    m.add_class::<ReturnNode>()?;
    m.add_class::<YieldNode>()?;
    m.add_class::<Global>()?;
    m.add_class::<Static>()?;
    m.add_class::<Echo>()?;
    m.add_class::<Print>()?;
    m.add_class::<Unset>()?;
    m.add_class::<TryNode>()?;
    m.add_class::<CatchNode>()?;
    m.add_class::<FinallyNode>()?;
    m.add_class::<Throw>()?;
    m.add_class::<Declare>()?;
    m.add_class::<Directive>()?;
    m.add_class::<Function>()?;
    m.add_class::<Method>()?;
    m.add_class::<Closure>()?;
    m.add_class::<Class>()?;
    m.add_class::<Trait>()?;
    m.add_class::<ClassConstants>()?;
    m.add_class::<ClassConstant>()?;
    m.add_class::<ClassVariables>()?;
    m.add_class::<ClassVariable>()?;
    m.add_class::<Interface>()?;
    m.add_class::<AssignOp>()?;
    m.add_class::<BinaryOp>()?;
    m.add_class::<UnaryOp>()?;
    m.add_class::<TernaryOp>()?;
    m.add_class::<PreIncDecOp>()?;
    m.add_class::<PostIncDecOp>()?;
    m.add_class::<Cast>()?;
    m.add_class::<IsSet>()?;
    m.add_class::<Empty>()?;
    m.add_class::<Eval>()?;
    m.add_class::<Include>()?;
    m.add_class::<Require>()?;
    m.add_class::<Exit>()?;
    m.add_class::<Silence>()?;
    m.add_class::<MagicConstant>()?;
    m.add_class::<Constant>()?;
    m.add_class::<Variable>()?;
    m.add_class::<StaticVariable>()?;
    m.add_class::<LexicalVariable>()?;
    m.add_class::<FormalParameter>()?;
    m.add_class::<Parameter>()?;
    m.add_class::<FunctionCall>()?;
    m.add_class::<Array>()?;
    m.add_class::<ArrayElement>()?;
    m.add_class::<ArrayOffset>()?;
    m.add_class::<StringOffset>()?;
    m.add_class::<ObjectProperty>()?;
    m.add_class::<StaticProperty>()?;
    m.add_class::<MethodCall>()?;
    m.add_class::<StaticMethodCall>()?;
    m.add_class::<If>()?;
    m.add_class::<ElseIf>()?;
    m.add_class::<Else>()?;
    m.add_class::<While>()?;
    m.add_class::<DoWhile>()?;
    m.add_class::<For>()?;
    m.add_class::<Foreach>()?;
    m.add_class::<ForeachVariable>()?;
    m.add_class::<Switch>()?;
    m.add_class::<Case>()?;
    m.add_class::<Default>()?;
    m.add_class::<UseDeclarations>()?;
    m.add_class::<UseDeclaration>()?;
    m.add_class::<ConstantDeclarations>()?;
    m.add_class::<ConstantDeclaration>()?;
    m.add_class::<TraitUse>()?;
    m.add_class::<TraitModifier>()?;
    m.add_class::<MatchExpr>()?;
    m.add_class::<MatchArm>()?;
    m.add_class::<NamedArgument>()?;
    m.add_class::<NullsafePropertyAccess>()?;
    m.add_class::<NullsafeCall>()?;
    m.add_class::<ConstructorParameter>()?;
    Ok(())
}
