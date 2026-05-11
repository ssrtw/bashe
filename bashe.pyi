from typing import Any, Callable

class Bashe:
    def __init__(self) -> None: ...
    def parse(self, code: str, filename: str | None = None) -> list: ...

class _Node:
    lineno: int | None
    @classmethod
    def fields(cls) -> list[str]: ...
    def generic(self, with_lineno: bool = False) -> tuple[str, dict[str, Any]]: ...
    def accept(self, visitor: Callable[..., Any]) -> None: ...
    def __eq__(self, other: Any) -> bool: ...
    def __ne__(self, other: Any) -> bool: ...

# ── top-level ──
class InlineHTML(_Node):
    data: Any
    def __init__(self, data: Any, *, lineno: int | None = None) -> None: ...

class Block(_Node):
    nodes: Any
    def __init__(self, nodes: Any, *, lineno: int | None = None) -> None: ...

class Namespace(_Node):
    name: Any
    nodes: Any
    def __init__(self, name: Any, nodes: Any, *, lineno: int | None = None) -> None: ...

# ── statements ──
class Assignment(_Node):
    node: Any
    expr: Any
    is_ref: Any
    def __init__(
        self, node: Any, expr: Any, is_ref: Any, *, lineno: int | None = None
    ) -> None: ...

class ListAssignment(_Node):
    nodes: Any
    expr: Any
    def __init__(self, nodes: Any, expr: Any, *, lineno: int | None = None) -> None: ...

class New(_Node):
    name: Any
    params: Any
    def __init__(
        self, name: Any, params: Any, *, lineno: int | None = None
    ) -> None: ...

class Clone(_Node):
    node: Any
    def __init__(self, node: Any, *, lineno: int | None = None) -> None: ...

class Break(_Node):
    node: Any
    def __init__(self, node: Any, *, lineno: int | None = None) -> None: ...

class Continue(_Node):
    node: Any
    def __init__(self, node: Any, *, lineno: int | None = None) -> None: ...

class Return(_Node):
    node: Any
    def __init__(self, node: Any, *, lineno: int | None = None) -> None: ...

class Yield(_Node):
    node: Any
    def __init__(self, node: Any, *, lineno: int | None = None) -> None: ...

class Global(_Node):
    nodes: Any
    def __init__(self, nodes: Any, *, lineno: int | None = None) -> None: ...

class Static(_Node):
    nodes: Any
    def __init__(self, nodes: Any, *, lineno: int | None = None) -> None: ...

class Echo(_Node):
    nodes: Any
    def __init__(self, nodes: Any, *, lineno: int | None = None) -> None: ...

class Print(_Node):
    node: Any
    def __init__(self, node: Any, *, lineno: int | None = None) -> None: ...

class Unset(_Node):
    nodes: Any
    def __init__(self, nodes: Any, *, lineno: int | None = None) -> None: ...

class Try(_Node):
    nodes: Any
    catches: Any
    finally_: Any
    def __init__(
        self, nodes: Any, catches: Any, finally_: Any, *, lineno: int | None = None
    ) -> None: ...

class Catch(_Node):
    class_: Any
    var: Any
    nodes: Any
    def __init__(
        self, class_: Any, var: Any, nodes: Any, *, lineno: int | None = None
    ) -> None: ...

class Finally(_Node):
    nodes: Any
    def __init__(self, nodes: Any, *, lineno: int | None = None) -> None: ...

class Throw(_Node):
    node: Any
    def __init__(self, node: Any, *, lineno: int | None = None) -> None: ...

class Declare(_Node):
    directives: Any
    node: Any
    def __init__(
        self, directives: Any, node: Any, *, lineno: int | None = None
    ) -> None: ...

class Directive(_Node):
    name: Any
    node: Any
    def __init__(self, name: Any, node: Any, *, lineno: int | None = None) -> None: ...

# ── functions / closures ──
class Function(_Node):
    name: Any
    params: Any
    nodes: Any
    is_ref: Any
    return_type: Any
    def __init__(
        self,
        name: Any,
        params: Any,
        nodes: Any,
        is_ref: Any,
        return_type: Any,
        *,
        lineno: int | None = None,
    ) -> None: ...

class Method(_Node):
    name: Any
    modifiers: Any
    params: Any
    nodes: Any
    is_ref: Any
    def __init__(
        self,
        name: Any,
        modifiers: Any,
        params: Any,
        nodes: Any,
        is_ref: Any,
        *,
        lineno: int | None = None,
    ) -> None: ...

class Closure(_Node):
    params: Any
    vars: Any
    nodes: Any
    is_ref: Any
    def __init__(
        self,
        params: Any,
        vars: Any,
        nodes: Any,
        is_ref: Any,
        *,
        lineno: int | None = None,
    ) -> None: ...

# ── class / trait ──
class Class(_Node):
    name: Any
    type_: Any
    extends: Any
    implements: Any
    traits: Any
    nodes: Any
    def __init__(
        self,
        name: Any,
        type_: Any,
        extends: Any,
        implements: Any,
        traits: Any,
        nodes: Any,
        *,
        lineno: int | None = None,
    ) -> None: ...

class Trait(_Node):
    name: Any
    traits: Any
    nodes: Any
    def __init__(
        self, name: Any, traits: Any, nodes: Any, *, lineno: int | None = None
    ) -> None: ...

class ClassConstants(_Node):
    nodes: Any
    def __init__(self, nodes: Any, *, lineno: int | None = None) -> None: ...

class ClassConstant(_Node):
    name: Any
    initial: Any
    def __init__(
        self, name: Any, initial: Any, *, lineno: int | None = None
    ) -> None: ...

class ClassVariables(_Node):
    modifiers: Any
    nodes: Any
    def __init__(
        self, modifiers: Any, nodes: Any, *, lineno: int | None = None
    ) -> None: ...

class ClassVariable(_Node):
    name: Any
    initial: Any
    def __init__(
        self, name: Any, initial: Any, *, lineno: int | None = None
    ) -> None: ...

class Interface(_Node):
    name: Any
    extends: Any
    nodes: Any
    def __init__(
        self, name: Any, extends: Any, nodes: Any, *, lineno: int | None = None
    ) -> None: ...

# ── operators ──
class AssignOp(_Node):
    op: Any
    left: Any
    right: Any
    def __init__(
        self, op: Any, left: Any, right: Any, *, lineno: int | None = None
    ) -> None: ...

class BinaryOp(_Node):
    op: Any
    left: Any
    right: Any
    def __init__(
        self, op: Any, left: Any, right: Any, *, lineno: int | None = None
    ) -> None: ...

class UnaryOp(_Node):
    op: Any
    expr: Any
    def __init__(self, op: Any, expr: Any, *, lineno: int | None = None) -> None: ...

class TernaryOp(_Node):
    expr: Any
    iftrue: Any
    iffalse: Any
    def __init__(
        self, expr: Any, iftrue: Any, iffalse: Any, *, lineno: int | None = None
    ) -> None: ...

class PreIncDecOp(_Node):
    op: Any
    expr: Any
    def __init__(self, op: Any, expr: Any, *, lineno: int | None = None) -> None: ...

class PostIncDecOp(_Node):
    op: Any
    expr: Any
    def __init__(self, op: Any, expr: Any, *, lineno: int | None = None) -> None: ...

class Cast(_Node):
    type_: Any
    expr: Any
    def __init__(self, type_: Any, expr: Any, *, lineno: int | None = None) -> None: ...

# ── expressions ──
class IsSet(_Node):
    nodes: Any
    def __init__(self, nodes: Any, *, lineno: int | None = None) -> None: ...

class Empty(_Node):
    expr: Any
    def __init__(self, expr: Any, *, lineno: int | None = None) -> None: ...

class Eval(_Node):
    expr: Any
    def __init__(self, expr: Any, *, lineno: int | None = None) -> None: ...

class Include(_Node):
    expr: Any
    once: Any
    def __init__(self, expr: Any, once: Any, *, lineno: int | None = None) -> None: ...

class Require(_Node):
    expr: Any
    once: Any
    def __init__(self, expr: Any, once: Any, *, lineno: int | None = None) -> None: ...

class Exit(_Node):
    expr: Any
    type_: Any
    def __init__(self, expr: Any, type_: Any, *, lineno: int | None = None) -> None: ...

class Silence(_Node):
    expr: Any
    def __init__(self, expr: Any, *, lineno: int | None = None) -> None: ...

class MagicConstant(_Node):
    name: Any
    value: Any
    def __init__(self, name: Any, value: Any, *, lineno: int | None = None) -> None: ...

class Constant(_Node):
    name: Any
    def __init__(self, name: Any, *, lineno: int | None = None) -> None: ...

class Variable(_Node):
    name: Any
    def __init__(self, name: Any, *, lineno: int | None = None) -> None: ...

class StaticVariable(_Node):
    name: Any
    initial: Any
    def __init__(
        self, name: Any, initial: Any, *, lineno: int | None = None
    ) -> None: ...

class LexicalVariable(_Node):
    name: Any
    is_ref: Any
    def __init__(
        self, name: Any, is_ref: Any, *, lineno: int | None = None
    ) -> None: ...

class FormalParameter(_Node):
    name: Any
    default: Any
    is_ref: Any
    type_: Any
    def __init__(
        self,
        name: Any,
        default: Any,
        is_ref: Any,
        type_: Any,
        *,
        lineno: int | None = None,
    ) -> None: ...

class Parameter(_Node):
    node: Any
    is_ref: Any
    def __init__(
        self, node: Any, is_ref: Any, *, lineno: int | None = None
    ) -> None: ...

# ── calls / access ──
class FunctionCall(_Node):
    name: Any
    params: Any
    def __init__(
        self, name: Any, params: Any, *, lineno: int | None = None
    ) -> None: ...

class Array(_Node):
    nodes: Any
    def __init__(self, nodes: Any, *, lineno: int | None = None) -> None: ...

class ArrayElement(_Node):
    key: Any
    value: Any
    is_ref: Any
    def __init__(
        self, key: Any, value: Any, is_ref: Any, *, lineno: int | None = None
    ) -> None: ...

class ArrayOffset(_Node):
    node: Any
    expr: Any
    def __init__(self, node: Any, expr: Any, *, lineno: int | None = None) -> None: ...

class ObjectProperty(_Node):
    node: Any
    name: Any
    def __init__(self, node: Any, name: Any, *, lineno: int | None = None) -> None: ...

class StaticProperty(_Node):
    node: Any
    name: Any
    def __init__(self, node: Any, name: Any, *, lineno: int | None = None) -> None: ...

class MethodCall(_Node):
    node: Any
    name: Any
    params: Any
    def __init__(
        self, node: Any, name: Any, params: Any, *, lineno: int | None = None
    ) -> None: ...

class StaticMethodCall(_Node):
    class_: Any
    name: Any
    params: Any
    def __init__(
        self, class_: Any, name: Any, params: Any, *, lineno: int | None = None
    ) -> None: ...

# ── flow control ──
class If(_Node):
    expr: Any
    node: Any
    elseifs: Any
    else_: Any
    def __init__(
        self,
        expr: Any,
        node: Any,
        elseifs: Any,
        else_: Any,
        *,
        lineno: int | None = None,
    ) -> None: ...

class ElseIf(_Node):
    expr: Any
    node: Any
    def __init__(self, expr: Any, node: Any, *, lineno: int | None = None) -> None: ...

class Else(_Node):
    node: Any
    def __init__(self, node: Any, *, lineno: int | None = None) -> None: ...

class While(_Node):
    expr: Any
    node: Any
    def __init__(self, expr: Any, node: Any, *, lineno: int | None = None) -> None: ...

class DoWhile(_Node):
    node: Any
    expr: Any
    def __init__(self, node: Any, expr: Any, *, lineno: int | None = None) -> None: ...

class For(_Node):
    start: Any
    test: Any
    count: Any
    node: Any
    def __init__(
        self, start: Any, test: Any, count: Any, node: Any, *, lineno: int | None = None
    ) -> None: ...

class Foreach(_Node):
    expr: Any
    keyvar: Any
    valvar: Any
    node: Any
    def __init__(
        self,
        expr: Any,
        keyvar: Any,
        valvar: Any,
        node: Any,
        *,
        lineno: int | None = None,
    ) -> None: ...

class ForeachVariable(_Node):
    name: Any
    is_ref: Any
    def __init__(
        self, name: Any, is_ref: Any, *, lineno: int | None = None
    ) -> None: ...

class Switch(_Node):
    expr: Any
    nodes: Any
    def __init__(self, expr: Any, nodes: Any, *, lineno: int | None = None) -> None: ...

class Case(_Node):
    expr: Any
    nodes: Any
    def __init__(self, expr: Any, nodes: Any, *, lineno: int | None = None) -> None: ...

class Default(_Node):
    nodes: Any
    def __init__(self, nodes: Any, *, lineno: int | None = None) -> None: ...

# ── namespace / use ──
class UseDeclarations(_Node):
    nodes: Any
    def __init__(self, nodes: Any, *, lineno: int | None = None) -> None: ...

class UseDeclaration(_Node):
    name: Any
    alias: Any
    def __init__(self, name: Any, alias: Any, *, lineno: int | None = None) -> None: ...

class ConstantDeclarations(_Node):
    nodes: Any
    def __init__(self, nodes: Any, *, lineno: int | None = None) -> None: ...

class ConstantDeclaration(_Node):
    name: Any
    initial: Any
    def __init__(
        self, name: Any, initial: Any, *, lineno: int | None = None
    ) -> None: ...

class TraitUse(_Node):
    name: Any
    renames: Any
    def __init__(
        self, name: Any, renames: Any, *, lineno: int | None = None
    ) -> None: ...

class TraitModifier(_Node):
    from_: Any
    to: Any
    visibility: Any
    def __init__(
        self, from_: Any, to: Any, visibility: Any, *, lineno: int | None = None
    ) -> None: ...

# ── PHP 8.0+ ──
class MatchExpr(_Node):
    condition: Any
    arms: Any
    def __init__(
        self, condition: Any, arms: Any, *, lineno: int | None = None
    ) -> None: ...

class MatchArm(_Node):
    pattern: Any
    body: Any
    def __init__(
        self, pattern: Any, body: Any, *, lineno: int | None = None
    ) -> None: ...

class NamedArgument(_Node):
    name: Any
    node: Any
    def __init__(self, name: Any, node: Any, *, lineno: int | None = None) -> None: ...

class NullsafePropertyAccess(_Node):
    node: Any
    name: Any
    def __init__(self, node: Any, name: Any, *, lineno: int | None = None) -> None: ...

class NullsafeCall(_Node):
    node: Any
    name: Any
    params: Any
    def __init__(
        self, node: Any, name: Any, params: Any, *, lineno: int | None = None
    ) -> None: ...

class ConstructorParameter(_Node):
    modifiers: Any
    name: Any
    type_: Any
    default: Any
    def __init__(
        self,
        modifiers: Any,
        name: Any,
        type_: Any,
        default: Any,
        *,
        lineno: int | None = None,
    ) -> None: ...
