use pyo3::prelude::*;

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
            fn py_new(
                $($field: Py<PyAny>,)*
                lineno: Option<usize>,
            ) -> Self {
                Self { lineno, $($field),* }
            }

            #[classattr]
            fn fields() -> Vec<&'static str> {
                vec![$(stringify!($field)),*]
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
