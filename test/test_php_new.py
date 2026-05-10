"""Tests for PHP 5.4+ / 7.x / 8.x syntax that phply does not support.

Always uses native ``bashe.types`` regardless of whether phply is installed.
"""

from bashe.types import (
    Array,
    ArrayElement,
    ArrayOffset,
    Assignment,
    BinaryOp,
    Class,
    ClassVariable,
    ClassVariables,
    ConstructorParameter,
    Echo,
    FormalParameter,
    Function,
    MatchArm,
    MatchExpr,
    Method,
    MethodCall,
    NamedArgument,
    New,
    NullsafeCall,
    NullsafePropertyAccess,
    ObjectProperty,
    Return,
    Trait,
    TraitUse,
    Variable,
)
from test.util import eq_ast

# ═══════════════════════════════════════════════════════════════════════
# PHP 5.4+
# ═══════════════════════════════════════════════════════════════════════


def test_php5():
    """Traits, short array syntax, and trait use in class bodies."""
    input = """<?php
trait SayHello {
    public function hello() { echo "Hello "; }
}
class World {
    use SayHello;
}
$data = ["PHP", "5.x"];
$obj = new World();
$obj->hello();
"""
    expected = [
        Trait(
            "SayHello",
            [],
            [Method("hello", ["public"], [], [Echo(["Hello "])], False)],
        ),
        Class("World", None, None, [], [TraitUse("SayHello", [])], []),
        Assignment(
            Variable("$data"),
            Array(
                [
                    ArrayElement(None, "PHP", False),
                    ArrayElement(None, "5.x", False),
                ]
            ),
            False,
        ),
        Assignment(Variable("$obj"), New("World", []), False),
        MethodCall(Variable("$obj"), "hello", []),
    ]
    eq_ast(input, expected, legacy=False)


# ═══════════════════════════════════════════════════════════════════════
# PHP 7.x
# ═══════════════════════════════════════════════════════════════════════


def test_php7_scalar_types_and_return_type():
    """Scalar type declarations and return type hints."""
    input = """<?php
function add(int $a, int $b): int {
    return $a + $b;
}
"""
    expected = [
        Function(
            "add",
            [
                FormalParameter("$a", None, False, "int"),
                FormalParameter("$b", None, False, "int"),
            ],
            [Return(BinaryOp("+", Variable("$a"), Variable("$b")))],
            False,
            "int",
        ),
    ]
    eq_ast(input, expected, legacy=False)


def test_php7_null_coalesce():
    """Null coalescing operator ??."""
    input = """<?php
$name = $_GET["user"] ?? 'Guest';
"""
    expected = [
        Assignment(
            Variable("$name"),
            BinaryOp("??", ArrayOffset(Variable("$_GET"), "user"), "Guest"),
            False,
        ),
    ]
    eq_ast(input, expected, legacy=False)


def test_php7_spaceship():
    """Spaceship operator <=>."""
    input = """<?php
$result = 1 <=> 2;
"""
    expected = [
        Assignment(Variable("$result"), BinaryOp("<=>", 1, 2), False),
    ]
    eq_ast(input, expected, legacy=False)


# ═══════════════════════════════════════════════════════════════════════
# PHP 8.0+
# ═══════════════════════════════════════════════════════════════════════


def test_php8_constructor_promotion():
    """Constructor property promotion — parameters with visibility."""
    input = """<?php
class User {
    public function __construct(
        public string $name,
        public ?int $age = null
    ) {}
}
"""
    expected = [
        Class(
            "User",
            None,
            None,
            [],
            [],
            [
                Method(
                    "__construct",
                    ["public"],
                    [
                        ConstructorParameter(["public"], "$name", "string", None),
                        ConstructorParameter(["public"], "$age", "?int", None),
                    ],
                    [],
                    False,
                ),
            ],
        ),
    ]
    eq_ast(input, expected, legacy=False)


def test_php8_named_arguments():
    """Named arguments — name: value syntax in calls."""
    input = """<?php
$user = new User(age: 25, name: "Alice");
"""
    expected = [
        Assignment(
            Variable("$user"),
            New("User", [NamedArgument("age", 25), NamedArgument("name", "Alice")]),
            False,
        ),
    ]
    eq_ast(input, expected, legacy=False)


def test_php8_nullsafe():
    """Nullsafe method/property access via ?->."""
    input = """<?php
$city = $user?->address?->getCity();
"""
    expected = [
        Assignment(
            Variable("$city"),
            NullsafeCall(
                NullsafePropertyAccess(Variable("$user"), "address"),
                "getCity",
                [],
            ),
            False,
        ),
    ]
    eq_ast(input, expected, legacy=False)


def test_php8_match_expression():
    """Match expression (PHP 8.0) — expression-based pattern matching."""
    input = """<?php
echo match($user->name) {
    'Alice' => '管理者',
    default => '一般用戶',
};
"""
    expected = [
        Echo(
            [
                MatchExpr(
                    ObjectProperty(Variable("$user"), "name"),
                    [
                        MatchArm("Alice", "管理者"),
                        MatchArm(None, "一般用戶"),
                    ],
                )
            ]
        ),
    ]
    eq_ast(input, expected, legacy=False)


# ═══════════════════════════════════════════════════════════════════════
# PHP 8.1
# ═══════════════════════════════════════════════════════════════════════


def test_php81_readonly_property():
    """Readonly property modifier (PHP 8.1)."""
    input = """<?php
class User {
    public readonly string $name;
}
"""
    expected = [
        Class(
            "User",
            None,
            None,
            [],
            [],
            [
                ClassVariables(
                    ["public", "readonly"],
                    [ClassVariable("$name", None)],
                ),
            ],
        ),
    ]
    eq_ast(input, expected, legacy=False)


def test_php81_intersection_type():
    """Intersection type hints — Countable&Iterator (PHP 8.1)."""
    input = """<?php
function process(Countable&Iterator $input): void {}
"""
    expected = [
        Function(
            "process",
            [
                FormalParameter("$input", None, False, "Countable&Iterator"),
            ],
            [],
            False,
            "void",
        ),
    ]
    eq_ast(input, expected, legacy=False)


# ═══════════════════════════════════════════════════════════════════════
# PHP 8.2
# ═══════════════════════════════════════════════════════════════════════


def test_php82_readonly_class():
    """Readonly class — all properties implicitly readonly (PHP 8.2)."""
    input = """<?php
readonly class Config {
    public string $host;
}
"""
    expected = [
        Class(
            "Config",
            "readonly",
            None,
            [],
            [],
            [
                ClassVariables(
                    ["public"],
                    [ClassVariable("$host", None)],
                ),
            ],
        ),
    ]
    eq_ast(input, expected, legacy=False)


def test_php82_dnf_type():
    """DNF (Disjunctive Normal Form) type — (A&B)|C (PHP 8.2)."""
    input = """<?php
function handle((Countable&Iterator)|null $data): void {}
"""
    expected = [
        Function(
            "handle",
            [
                FormalParameter(
                    "$data", None, False, "(Countable&Iterator)|null"
                ),
            ],
            [],
            False,
            "void",
        ),
    ]
    eq_ast(input, expected, legacy=False)
