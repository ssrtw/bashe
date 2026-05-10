"""Tests for bashe.types.Node - accept() and generic()."""

from bashe.types import (
    Array,
    ArrayElement,
    Assignment,
    BinaryOp,
    FormalParameter,
    Function,
    New,
    Return,
    Variable,
)


def test_accept_flat():
    collector = []
    node = Variable("$x")
    node.accept(lambda n: collector.append(n))
    assert len(collector) == 1
    assert collector[0] is node


def test_accept_nested():
    collector = []
    node = Assignment(Variable("$a"), BinaryOp("+", Variable("$b"), 1), False)
    node.accept(lambda n: collector.append(n.__class__.__name__))
    assert collector == ["Assignment", "Variable", "BinaryOp", "Variable"]


def test_accept_with_lists():
    collector = []
    node = New("Foo", [Variable("$x"), Variable("$y")])
    node.accept(lambda n: collector.append(n))
    assert len(collector) == 3
    assert collector[0] is node


def test_generic_leaf():
    v = Variable("$x")
    g = v.generic()
    assert g == ("Variable", {"name": "$x"})


def test_generic_with_lineno():
    v = Variable("$x", lineno=42)
    g = v.generic(with_lineno=True)
    assert g == ("Variable", {"name": "$x", "lineno": 42})


def test_generic_without_lineno():
    v = Variable("$x", lineno=42)
    g = v.generic(with_lineno=False)
    assert "lineno" not in g[1]


def test_generic_nested():
    node = BinaryOp("+", Variable("$x"), 1)
    g = node.generic()
    assert g == (
        "BinaryOp",
        {
            "op": "+",
            "left": ("Variable", {"name": "$x"}),
            "right": 1,
        },
    )


def test_generic_array():
    arr = Array(
        [
            ArrayElement(None, "a", False),
            ArrayElement("k", Variable("$v"), True),
        ]
    )
    g = arr.generic()
    assert g == (
        "Array",
        {
            "nodes": [
                ("ArrayElement", {"key": None, "value": "a", "is_ref": False}),
                (
                    "ArrayElement",
                    {"key": "k", "value": ("Variable", {"name": "$v"}), "is_ref": True},
                ),
            ]
        },
    )


def test_generic_function():
    f = Function(
        "f",
        [FormalParameter("$x", None, False, "int")],
        [Return(BinaryOp("*", Variable("$x"), 2))],
        False,
        None,
    )
    g = f.generic()
    assert g == (
        "Function",
        {
            "name": "f",
            "params": [
                (
                    "FormalParameter",
                    {"name": "$x", "default": None, "is_ref": False, "type": "int"},
                )
            ],
            "nodes": [
                (
                    "Return",
                    {
                        "node": (
                            "BinaryOp",
                            {
                                "op": "*",
                                "left": ("Variable", {"name": "$x"}),
                                "right": 2,
                            },
                        )
                    },
                )
            ],
            "is_ref": False,
            "return_type": None,
        },
    )
