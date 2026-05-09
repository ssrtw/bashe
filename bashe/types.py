"""Native phply-compatible AST node types.

All 79 node types matching phply.phpast definitions are provided as
native Python classes.  No dependency on phply is required — these can
be extended with new fields or entirely new node types for PHP syntax
that phply does not support.
"""

__all__ = [
    "InlineHTML",
    "Block",
    "Assignment",
    "ListAssignment",
    "New",
    "Clone",
    "Break",
    "Continue",
    "Return",
    "Yield",
    "Global",
    "Static",
    "Echo",
    "Print",
    "Unset",
    "Try",
    "Catch",
    "Finally",
    "Throw",
    "Declare",
    "Directive",
    "Function",
    "Method",
    "Closure",
    "Class",
    "Trait",
    "ClassConstants",
    "ClassConstant",
    "ClassVariables",
    "ClassVariable",
    "Interface",
    "AssignOp",
    "BinaryOp",
    "UnaryOp",
    "TernaryOp",
    "PreIncDecOp",
    "PostIncDecOp",
    "Cast",
    "IsSet",
    "Empty",
    "Eval",
    "Include",
    "Require",
    "Exit",
    "Silence",
    "MagicConstant",
    "Constant",
    "Variable",
    "StaticVariable",
    "LexicalVariable",
    "FormalParameter",
    "Parameter",
    "FunctionCall",
    "Array",
    "ArrayElement",
    "ArrayOffset",
    "StringOffset",
    "ObjectProperty",
    "StaticProperty",
    "MethodCall",
    "StaticMethodCall",
    "If",
    "ElseIf",
    "Else",
    "While",
    "DoWhile",
    "For",
    "Foreach",
    "ForeachVariable",
    "Switch",
    "Case",
    "Default",
    "Namespace",
    "UseDeclarations",
    "UseDeclaration",
    "ConstantDeclarations",
    "ConstantDeclaration",
    "TraitUse",
    "TraitModifier",
    # PHP 7+
    # Function (redefined with return_type)
    # PHP 8.0+
    "MatchExpr",
    "MatchArm",
    "NamedArgument",
    "NullsafePropertyAccess",
    "NullsafeCall",
    "ConstructorParameter",
]


class Node:
    """Base class for AST nodes.

    Implements the same ``fields`` / ``lineno`` protocol as
    :class:`phply.phpast.Node` so native and phply nodes are
    interchangeable.
    """

    fields = []

    def __init__(self, *args, **kwargs):
        assert len(self.fields) == len(args), (
            f"{self.__class__.__name__} takes {len(self.fields)} arguments"
        )
        self.lineno = kwargs.get("lineno")
        for i, field in enumerate(self.fields):
            setattr(self, field, args[i])

    def __repr__(self):
        vals = ", ".join(repr(getattr(self, f, None)) for f in self.fields)
        return f"{self.__class__.__name__}({vals})"

    def __eq__(self, other):
        if not isinstance(other, self.__class__):
            return False
        for field in self.fields:
            if not (getattr(self, field, None) == getattr(other, field, None)):
                return False
        return True

    def __ne__(self, other):
        return not self.__eq__(other)

    def accept(self, visitor):
        """Call *visitor* on this node and every child node depth-first."""
        visitor(self)
        for field in self.fields:
            value = getattr(self, field)
            if isinstance(value, Node):
                value.accept(visitor)
            elif isinstance(value, list):
                for item in value:
                    if isinstance(item, Node):
                        item.accept(visitor)

    def generic(self, with_lineno=False):
        """Return a ``(classname, dict)`` tuple for nested serialisation.

        Matches the phply ``generic()`` protocol.
        """
        values = {}
        if with_lineno:
            values["lineno"] = self.lineno
        for field in self.fields:
            value = getattr(self, field)
            if hasattr(value, "generic"):
                value = value.generic(with_lineno)
            elif isinstance(value, list):
                items = value
                value = []
                for item in items:
                    if hasattr(item, "generic"):
                        item = item.generic(with_lineno)
                    value.append(item)
            values[field] = value
        return (self.__class__.__name__, values)


def _node(name, fields):
    """Create a concrete Node subclass with the given *fields*."""
    return type(name, (Node,), {"fields": list(fields)})


InlineHTML = _node("InlineHTML", ["data"])
Block = _node("Block", ["nodes"])
Assignment = _node("Assignment", ["node", "expr", "is_ref"])
ListAssignment = _node("ListAssignment", ["nodes", "expr"])
New = _node("New", ["name", "params"])
Clone = _node("Clone", ["node"])
Break = _node("Break", ["node"])
Continue = _node("Continue", ["node"])
Return = _node("Return", ["node"])
Yield = _node("Yield", ["node"])
Global = _node("Global", ["nodes"])
Static = _node("Static", ["nodes"])
Echo = _node("Echo", ["nodes"])
Print = _node("Print", ["node"])
Unset = _node("Unset", ["nodes"])
Try = _node("Try", ["nodes", "catches", "finally"])
Catch = _node("Catch", ["class_", "var", "nodes"])
Finally = _node("Finally", ["nodes"])
Throw = _node("Throw", ["node"])
Declare = _node("Declare", ["directives", "node"])
Directive = _node("Directive", ["name", "node"])
Function = _node("Function", ["name", "params", "nodes", "is_ref"])
Method = _node("Method", ["name", "modifiers", "params", "nodes", "is_ref"])
Closure = _node("Closure", ["params", "vars", "nodes", "is_ref"])
Class = _node("Class", ["name", "type", "extends", "implements", "traits", "nodes"])
Trait = _node("Trait", ["name", "traits", "nodes"])
ClassConstants = _node("ClassConstants", ["nodes"])
ClassConstant = _node("ClassConstant", ["name", "initial"])
ClassVariables = _node("ClassVariables", ["modifiers", "nodes"])
ClassVariable = _node("ClassVariable", ["name", "initial"])
Interface = _node("Interface", ["name", "extends", "nodes"])
AssignOp = _node("AssignOp", ["op", "left", "right"])
BinaryOp = _node("BinaryOp", ["op", "left", "right"])
UnaryOp = _node("UnaryOp", ["op", "expr"])
TernaryOp = _node("TernaryOp", ["expr", "iftrue", "iffalse"])
PreIncDecOp = _node("PreIncDecOp", ["op", "expr"])
PostIncDecOp = _node("PostIncDecOp", ["op", "expr"])
Cast = _node("Cast", ["type", "expr"])
IsSet = _node("IsSet", ["nodes"])
Empty = _node("Empty", ["expr"])
Eval = _node("Eval", ["expr"])
Include = _node("Include", ["expr", "once"])
Require = _node("Require", ["expr", "once"])
Exit = _node("Exit", ["expr", "type"])
Silence = _node("Silence", ["expr"])
MagicConstant = _node("MagicConstant", ["name", "value"])
Constant = _node("Constant", ["name"])
Variable = _node("Variable", ["name"])
StaticVariable = _node("StaticVariable", ["name", "initial"])
LexicalVariable = _node("LexicalVariable", ["name", "is_ref"])
FormalParameter = _node("FormalParameter", ["name", "default", "is_ref", "type"])
Parameter = _node("Parameter", ["node", "is_ref"])
FunctionCall = _node("FunctionCall", ["name", "params"])
Array = _node("Array", ["nodes"])
ArrayElement = _node("ArrayElement", ["key", "value", "is_ref"])
ArrayOffset = _node("ArrayOffset", ["node", "expr"])
StringOffset = _node("StringOffset", ["node", "expr"])
ObjectProperty = _node("ObjectProperty", ["node", "name"])
StaticProperty = _node("StaticProperty", ["node", "name"])
MethodCall = _node("MethodCall", ["node", "name", "params"])
StaticMethodCall = _node("StaticMethodCall", ["class_", "name", "params"])
If = _node("If", ["expr", "node", "elseifs", "else_"])
ElseIf = _node("ElseIf", ["expr", "node"])
Else = _node("Else", ["node"])
While = _node("While", ["expr", "node"])
DoWhile = _node("DoWhile", ["node", "expr"])
For = _node("For", ["start", "test", "count", "node"])
Foreach = _node("Foreach", ["expr", "keyvar", "valvar", "node"])
ForeachVariable = _node("ForeachVariable", ["name", "is_ref"])
Switch = _node("Switch", ["expr", "nodes"])
Case = _node("Case", ["expr", "nodes"])
Default = _node("Default", ["nodes"])
Namespace = _node("Namespace", ["name", "nodes"])
UseDeclarations = _node("UseDeclarations", ["nodes"])
UseDeclaration = _node("UseDeclaration", ["name", "alias"])
ConstantDeclarations = _node("ConstantDeclarations", ["nodes"])
ConstantDeclaration = _node("ConstantDeclaration", ["name", "initial"])
TraitUse = _node("TraitUse", ["name", "renames"])
TraitModifier = _node("TraitModifier", ["from", "to", "visibility"])

# ── PHP 7+ types (not in phply) ────────────────────────────────────


Function = _node(  # overrides phply-compat: adds return_type
    "Function", ["name", "params", "nodes", "is_ref", "return_type"]
)


# ── PHP 8.0+ types ─────────────────────────────────────────────────

MatchExpr = _node("MatchExpr", ["condition", "arms"])
MatchArm = _node("MatchArm", ["pattern", "body"])
NamedArgument = _node("NamedArgument", ["name", "node"])
NullsafePropertyAccess = _node("NullsafePropertyAccess", ["node", "name"])
NullsafeCall = _node("NullsafeCall", ["node", "name", "params"])
ConstructorParameter = _node(
    "ConstructorParameter", ["modifiers", "name", "type", "default"]
)
