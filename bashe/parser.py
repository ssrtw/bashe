"""Tree-sitter PHP parser - refactored with dispatch table."""

import logging
import os
import warnings

import tree_sitter_php as tsphp
from tree_sitter import Language, Parser

from .utils import F

logger = logging.getLogger(__name__)


class _PhpAstProxy:
    """Proxy to the PHP AST module.

    Defaults to native :mod:`bashe.types`.  Use ``Bashe(legacy=True)`` to
    switch to ``phply.phpast`` for backwards compatibility.
    """

    _module = None
    _legacy = False
    _warned_legacy = False

    @classmethod
    def configure(cls, legacy=False):
        cls._legacy = legacy
        if legacy:
            try:
                from phply import phpast as mod  # type: ignore
            except ImportError:
                raise ImportError(
                    "phply is required for legacy compatibility. "
                    "Install it with:  pip install phply"
                )
            cls._module = mod
        else:
            from . import types as mod  # noqa: F811

            cls._module = mod

    def __getattr__(self, name):
        if self._module is None:
            self.configure(legacy=False)

        if self._legacy and not self._warned_legacy:
            warnings.warn(
                "Using phply as AST backend is deprecated. "
                "Remove the phply dependency and use native bashe.types "
                "for full PHP 8.x syntax support and better performance.",
                DeprecationWarning,
                stacklevel=2,
            )
            _PhpAstProxy._warned_legacy = True

        try:
            return getattr(self._module, name)
        except AttributeError:
            if self._legacy:
                raise AttributeError(
                    f"Type {name!r} is not available in phply. "
                    f"Remove phply and use native bashe.types instead."
                )
            raise


php = _PhpAstProxy()

PHP_LANGUAGE = Language(tsphp.language_php())
_parser = Parser(PHP_LANGUAGE)

SKIP_TYPES = {
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
}

STRING_CONTENT_TYPES = {
    "string_content",
    "heredoc_content",
    "nowdoc_content",
    "nowdoc_string",
    "escape_sequence",
}

STRING_EXCLUDED = {
    "heredoc_start",
    "heredoc_end",
    "nowdoc_start",
    "nowdoc_end",
    "shell_command_start",
    "shell_command_end",
}


def parse_number(text):
    text = text.lower().replace("_", "")
    if text.startswith("0x"):
        return int(text, 16)
    if text.startswith("0b"):
        return int(text, 2)
    if text.startswith("0o"):
        return int(text, 8)
    if text.startswith("0") and len(text) > 1 and "." not in text and "e" not in text:
        try:
            return int(text, 8)
        except ValueError:
            pass
    try:
        return int(text)
    except ValueError:
        return float(text)


def unescape_string(text, is_double):
    if not is_double:
        res, i = [], 0
        while i < len(text):
            if text[i] == "\\" and i + 1 < len(text):
                nxt = text[i + 1]
                if nxt == "\\":
                    res.append("\\")
                elif nxt == "'":
                    res.append("'")
                else:
                    res.append(text[i])
                    res.append(nxt)
                i += 2
            else:
                res.append(text[i])
                i += 1
        return "".join(res)
    mapping = {
        "n": "\n",
        "r": "\r",
        "t": "\t",
        "v": "\v",
        "e": "\x1b",
        "f": "\x0c",
        "\\": "\\",
        '"': '"',
        "$": "$",
    }
    res, i = [], 0
    while i < len(text):
        if text[i] == "\\" and i + 1 < len(text):
            char = text[i + 1]
            if char in mapping:
                res.append(mapping[char])
                i += 2
            elif char == "x":
                try:
                    res.append(chr(int(text[i + 2 : i + 4], 16)))
                    i += 4
                except (ValueError, IndexError):
                    res.append("\\x")
                    i += 2
            elif char.isdigit():
                try:
                    res.append(chr(int(text[i + 1 : i + 4], 8)))
                    i += 4
                except (ValueError, IndexError):
                    res.append(text[i + 1])
                    i += 2
            else:
                res.append(text[i + 1])
                i += 2
        else:
            res.append(text[i])
            i += 1
    return "".join(res)


# ─── context ──────────────────────────────────────────────────────────


def _scope_text(scope_node, ctx):
    if scope_node is None:
        return ""
    if scope_node.type == "variable_name":
        return ctx.translate(scope_node)
    txt = ctx.text(scope_node)
    if txt.lower() in ("static", "self", "parent"):
        return txt.lower()
    return txt


class Ctx:
    __slots__ = (
        "source",
        "echo_mode",
        "string_mode",
        "lineno_dict",
        "filename",
        "ns",
        "fn",
        "cls",
        "method",
        "trait",
    )

    def __init__(self, source: bytes, filename: str = None):
        self.source = source
        self.echo_mode = False
        self.string_mode = False
        self.lineno_dict = {}
        self.filename = filename
        self.ns = None
        self.fn = None
        self.cls = None
        self.method = None
        self.trait = None

    def text(self, n):
        return self.source[n.start_byte : n.end_byte].decode("utf8") if n else ""

    def lineno(self, n):
        return {"lineno": n.start_point[0] + 1} if n else {}

    def translate(self, n):
        return translate(n, self)

    def add_text_gap(self, parts, child, prev_end, is_double):
        start = prev_end if prev_end is not None else child.start_byte
        txt = unescape_string(
            self.source[start : child.end_byte].decode("utf8"), is_double
        )
        if parts and isinstance(parts[-1], str):
            parts[-1] += txt
        else:
            parts.append(txt)
        return child.end_byte

    # string body helpers ───────────────────────────────────────
    def process_string_children(self, children, parts, is_double):
        i = 0
        prev_end = None
        while i < len(children):
            c = children[i]
            if c.type in STRING_CONTENT_TYPES:
                prev_end = self.add_text_gap(parts, c, prev_end, is_double)
            elif c.type == "ERROR":
                prev_end = self.add_text_gap(parts, c, prev_end, is_double)
            elif (
                c.type == "{" and i + 2 < len(children) and children[i + 2].type == "}"
            ):
                inner = children[i + 1]
                prev = children[i + 2]
                if inner.is_named:
                    res = self.translate(inner)
                    if res is not None:
                        if inner.type == "dynamic_variable_name":
                            res = php.Variable(res, **self.lineno(inner))
                        parts.append(res)
                i += 2
                prev_end = prev.end_byte
            elif c.type in ("heredoc_body", "nowdoc_body"):
                self.process_string_children(list(c.children), parts, is_double)
                if c.children:
                    prev_end = c.children[-1].end_byte
            elif c.is_named and c.type not in STRING_EXCLUDED:
                res = self.translate(c)
                if res is not None:
                    parts.append(res)
                prev_end = c.end_byte
            i += 1

    def build_string_result(self, parts, ts_node):
        if not parts:
            return ""
        if len(parts) == 1 and isinstance(parts[0], (str, int, float)):
            return parts[0]
        if all(isinstance(p, (str, int, float)) for p in parts):
            return "".join(str(p) for p in parts)
        res = parts[0]
        for p in parts[1:]:
            res = php.BinaryOp(".", res, p, **self.lineno(ts_node))
        return res

    # echo helper ───────────────────────────────────────────────
    def emit(self, nodes, child, result):
        if self.echo_mode:
            nodes.append(php.Echo([result], **self.lineno(child)))
            self.echo_mode = False
        elif isinstance(result, list):
            nodes.extend(result)
        else:
            nodes.append(result)


# ─── dispatch table ───────────────────────────────────────────────────

_HANDLERS = {}


def _handler(*types):
    def dec(fn):
        for t in types:
            _HANDLERS[t] = fn
        return fn

    return dec


# ─── 1. Top-level ─────────────────────────────────────────────────────


@_handler("program", "namespace_definition", "declaration_list")
def top_level(ts_node, ctx):
    old_ns = ctx.ns
    name = None
    if ts_node.type == "namespace_definition":
        name = ctx.text(ts_node.child_by_field_name("name")) or None
        ctx.ns = name

    nodes = []

    def process(children):
        ti_prev_end = None
        for c in children:
            if c.type == "text":
                txt = ctx.text(c)
                if txt:
                    if ctx.echo_mode:
                        nodes.append(php.Echo([txt], **ctx.lineno(c)))
                        ctx.echo_mode = False
                    else:
                        nodes.append(php.InlineHTML(txt, **ctx.lineno(c)))
            elif c.type == "text_interpolation":
                for child in c.children:
                    if child.type == "text":
                        start = (
                            ti_prev_end if ti_prev_end is not None else child.start_byte
                        )
                        txt = ctx.source[start : child.end_byte].decode("utf8")
                        if txt:
                            nodes.append(php.InlineHTML(txt, **ctx.lineno(child)))
                    elif child.type == "php_tag":
                        ctx.echo_mode = ctx.text(child).strip() == "<?="
                    elif child.type == "php_end_tag":
                        ti_prev_end = child.end_byte
                    elif child.type not in SKIP_TYPES:
                        res = ctx.translate(child)
                        if res is not None:
                            ctx.emit(nodes, child, res)
                ti_prev_end = None
            elif c.type == "php_tag":
                ctx.echo_mode = ctx.text(c).strip() == "<?="
            elif c.type == "php_end_tag":
                pass
            elif c.type == "compound_statement":
                if nodes:
                    prev = nodes[-1]
                    if isinstance(
                        prev,
                        (
                            php.MethodCall,
                            php.FunctionCall,
                            php.ArrayOffset,
                            php.StringOffset,
                            php.ObjectProperty,
                            php.StaticProperty,
                            php.StaticMethodCall,
                            php.Variable,
                            php.Class,
                            php.Constant,
                        ),
                    ):
                        nc = c.named_children
                        inner = ctx.translate(nc[0]) if nc else None
                        if inner is not None:
                            if isinstance(inner, php.Constant):
                                inner = inner.name
                            elif isinstance(inner, list) and inner:
                                inner = inner[0]
                                if isinstance(inner, php.Constant):
                                    inner = inner.name
                            nodes[-1] = php.StringOffset(prev, inner, **ctx.lineno(c))
                            continue
                res = ctx.translate(c)
                if res is not None:
                    ctx.emit(nodes, c, res)
            elif c.type not in SKIP_TYPES:
                res = ctx.translate(c)
                if res is not None:
                    ctx.emit(nodes, c, res)

    process(ts_node.children)

    if ts_node.type == "namespace_definition":
        if len(nodes) == 1 and isinstance(nodes[0], php.Block):
            nodes = nodes[0].nodes
        result = php.Namespace(name, nodes, **ctx.lineno(ts_node))
        has_body = any(
            c.type == "declaration_list" for c in ts_node.children
        )
        if has_body:
            ctx.ns = old_ns
        return result
    return nodes


# ─── 1.5. Closures ─────────────────────────────────────────────────────


@_handler("anonymous_function")
def anonymous_function(ts_node, ctx):
    params = []
    uses = []
    is_ref = False
    body_stmts = []

    for c in ts_node.children:
        if c.type == "reference_modifier" and not c.is_named:
            pass
        elif c.type == "reference_modifier":
            is_ref = True
        elif c.type == "formal_parameters":
            params = [ctx.translate(p) for p in c.named_children]
        elif c.type == "anonymous_function_use_clause":
            for uc in c.children:
                if uc.type == "variable_name":
                    uses.append(php.LexicalVariable(ctx.text(uc), False))
                elif uc.type == "by_ref":
                    var_n = uc.named_children[0] if uc.named_children else None
                    uses.append(
                        php.LexicalVariable(ctx.text(var_n) if var_n else "", True)
                    )
        elif c.type == "compound_statement":
            bres = ctx.translate(c)
            body_stmts = (
                bres.nodes if isinstance(bres, php.Block) else ([bres] if bres else [])
            )

    return php.Closure(params, uses, body_stmts, is_ref, **ctx.lineno(ts_node))


# ─── 2. Literals ──────────────────────────────────────────────────────


@_handler("variable_name")
def variable_name(ts_node, ctx):
    return php.Variable(ctx.text(ts_node), **ctx.lineno(ts_node))


@_handler("variable_variable")
def variable_variable(ts_node, ctx):
    inner = ctx.translate(ts_node.named_children[0])
    return php.Variable(inner, **ctx.lineno(ts_node))


@_handler("integer")
def integer(ts_node, ctx):
    return parse_number(ctx.text(ts_node))


@_handler("float")
def float_(ts_node, ctx):
    return float(ctx.text(ts_node))


MAGIC_CONSTANTS = {
    "__LINE__",
    "__FILE__",
    "__DIR__",
    "__FUNCTION__",
    "__CLASS__",
    "__TRAIT__",
    "__METHOD__",
    "__NAMESPACE__",
}


@_handler("name", "qualified_name", "fully_qualified_name", "relative_name")
def name(ts_node, ctx):
    txt = ctx.text(ts_node)
    if txt.lower() in ("static", "self", "parent"):
        return txt.lower()
    if txt in MAGIC_CONSTANTS:
        value = None
        if txt == "__LINE__":
            value = ts_node.start_point[0] + 1
        elif txt == "__FILE__":
            value = ctx.filename
        elif txt == "__DIR__":
            value = os.path.dirname(ctx.filename) if ctx.filename else None
        elif txt == "__NAMESPACE__":
            value = ctx.ns
        elif txt == "__CLASS__":
            value = ctx.cls
        elif txt == "__FUNCTION__":
            value = ctx.fn
        elif txt == "__METHOD__":
            value = ctx.method
        elif txt == "__TRAIT__":
            value = ctx.trait
        return php.MagicConstant(txt, value, **ctx.lineno(ts_node))
    return php.Constant(txt, **ctx.lineno(ts_node))


@_handler("array_creation_expression")
def array_creation(ts_node, ctx):
    elements = []
    for c in ts_node.children:
        if c.type == "array_element_initializer":
            key_node = c.child_by_field_name("key")
            val_node = c.child_by_field_name("value")
            is_ref = any(
                not sub.is_named and ctx.text(sub).strip() == "&" for sub in c.children
            )
            key = ctx.translate(key_node) if key_node else None
            if val_node:
                val = ctx.translate(val_node)
            else:
                val = ctx.translate(c.named_children[-1]) if c.named_children else None
            elements.append(php.ArrayElement(key, val, is_ref, **ctx.lineno(c)))
    return php.Array(elements, **ctx.lineno(ts_node))


# ─── 3. Strings ───────────────────────────────────────────────────────


@_handler("list_literal")
def list_literal(ts_node, ctx):
    return [ctx.translate(c) for c in ts_node.named_children]


@_handler("string", "encapsed_string", "heredoc", "nowdoc", "shell_command_expression")
def string(ts_node, ctx):
    is_double = ts_node.type != "nowdoc" and (
        ts_node.type != "string" or '"' in ctx.text(ts_node).splitlines()[0]
    )
    parts = []
    prev = ctx.string_mode
    ctx.string_mode = True
    ctx.process_string_children(list(ts_node.children), parts, is_double)
    ctx.string_mode = prev
    res = ctx.build_string_result(parts, ts_node)
    if ts_node.type == "shell_command_expression":
        return php.FunctionCall(
            "shell_exec", [php.Parameter(res, False)], **ctx.lineno(ts_node)
        )
    return res


@_handler("text_interpolation")
def text_interpolation(ts_node, ctx):
    txt = ctx.text(ts_node)
    idx = txt.find("?>")
    if idx >= 0:
        txt = txt[idx + 2 :]
    idx = txt.find("<?")
    if idx >= 0:
        txt = txt[:idx] + txt[idx + 2 :]
    if txt:
        return php.InlineHTML(txt, **ctx.lineno(ts_node))
    return None


# ─── 4. Statements ────────────────────────────────────────────────────


@_handler("expression_statement")
def expression_statement(ts_node, ctx):
    if not ts_node.named_children:
        return None
    res = ctx.translate(ts_node.named_children[0])
    if isinstance(res, php.Constant) and res.name.lower() in ("exit", "die"):
        return php.Exit(None, res.name.lower(), **ctx.lineno(ts_node))
    return res


@_handler("clone_expression")
def clone_expr(ts_node, ctx):
    nc = ts_node.named_children
    return php.Clone(ctx.translate(nc[0]) if nc else None, **ctx.lineno(ts_node))


@_handler("print_intrinsic")
def print_intrinsic(ts_node, ctx):
    nc = ts_node.named_children
    return php.Print(ctx.translate(nc[0]) if nc else None, **ctx.lineno(ts_node))


@_handler("break_statement")
def break_stmt(ts_node, ctx):
    nc = ts_node.named_children
    depth = ctx.translate(nc[0]) if nc else None
    return php.Break(depth, **ctx.lineno(ts_node))


@_handler("continue_statement")
def continue_stmt(ts_node, ctx):
    nc = ts_node.named_children
    depth = ctx.translate(nc[0]) if nc else None
    return php.Continue(depth, **ctx.lineno(ts_node))


@_handler("unset_statement")
def unset_stmt(ts_node, ctx):
    vars_ = [ctx.translate(c) for c in ts_node.named_children]
    return php.Unset(vars_, **ctx.lineno(ts_node))


@_handler("parenthesized_expression")
def parenthesized(ts_node, ctx):
    return ctx.translate(ts_node.named_children[0])


@_handler("error_suppression_expression")
def error_suppression(ts_node, ctx):
    nc = ts_node.named_children
    return php.Silence(ctx.translate(nc[0]) if nc else None, **ctx.lineno(ts_node))


@_handler("while_statement")
def while_stmt(ts_node, ctx):
    nc = ts_node.named_children
    cond = ctx.translate(nc[0]) if nc else None
    body = ctx.translate(nc[1]) if len(nc) > 1 else php.Block([], **ctx.lineno(ts_node))
    body_stmts = body.nodes if isinstance(body, php.Block) else [body]
    return php.While(cond, php.Block(body_stmts), **ctx.lineno(ts_node))


@_handler("do_statement")
def do_stmt(ts_node, ctx):
    nc = ts_node.named_children
    body = ctx.translate(nc[0]) if nc else php.Block([], **ctx.lineno(ts_node))
    body_stmts = body.nodes if isinstance(body, php.Block) else [body]
    cond = ctx.translate(nc[1]) if len(nc) > 1 else None
    return php.DoWhile(php.Block(body_stmts), cond, **ctx.lineno(ts_node))


@_handler("for_statement")
def for_stmt(ts_node, ctx):
    nc = ts_node.named_children
    start = ctx.translate(nc[0]) if len(nc) > 0 else None
    test = ctx.translate(nc[1]) if len(nc) > 1 else None
    count = ctx.translate(nc[2]) if len(nc) > 2 else None
    body_node = nc[3] if len(nc) > 3 else None
    body = ctx.translate(body_node) if body_node else php.Block([], **ctx.lineno(ts_node))
    body_stmts = body.nodes if isinstance(body, php.Block) else [body]
    return php.For(start, test, count, php.Block(body_stmts), **ctx.lineno(ts_node))


@_handler("switch_statement")
def switch_stmt(ts_node, ctx):
    nc = ts_node.named_children
    expr = ctx.translate(nc[0]) if nc else None
    switch_block = nc[1] if len(nc) > 1 else None
    cases = ctx.translate(switch_block) if switch_block else []
    return php.Switch(expr, cases, **ctx.lineno(ts_node))


@_handler("switch_block", "case_statement", "default_statement")
def switch_block(ts_node, ctx):
    if ts_node.type == "switch_block":
        result = []
        for c in ts_node.named_children:
            res = ctx.translate(c)
            if res is not None:
                result.append(res)
        return result
    nodes = []
    for i, c in enumerate(ts_node.named_children):
        if ts_node.type == "case_statement" and i == 0:
            continue
        res = ctx.translate(c)
        if res is not None:
            if isinstance(res, list):
                nodes.extend(res)
            else:
                nodes.append(res)
    if ts_node.type == "case_statement":
        expr_n = ts_node.child_by_field_name("value")
        expr = ctx.translate(expr_n) if expr_n else None
        return php.Case(expr, nodes, **ctx.lineno(ts_node))
    return php.Default(nodes, **ctx.lineno(ts_node))


@_handler("match_expression")
def match_expr(ts_node, ctx):
    cond = ctx.translate(ts_node.child_by_field_name("condition"))
    body = ctx.translate(ts_node.child_by_field_name("body"))
    return php.MatchExpr(cond, body or [], **ctx.lineno(ts_node))


@_handler("match_block")
def match_block_handler(ts_node, ctx):
    return [ctx.translate(c) for c in ts_node.named_children]


@_handler("match_conditional_expression")
def match_conditional(ts_node, ctx):
    cond_n = ts_node.child_by_field_name("conditional_expressions")
    if cond_n is not None:
        patterns = [ctx.translate(c) for c in cond_n.named_children]
        if len(patterns) == 1:
            pattern = patterns[0]
        else:
            pattern = patterns
    else:
        pattern = None
    body = ctx.translate(ts_node.child_by_field_name("return_expression"))
    return php.MatchArm(pattern, body, **ctx.lineno(ts_node))


@_handler("match_default_expression")
def match_default_handler(ts_node, ctx):
    body = ctx.translate(ts_node.child_by_field_name("return_expression"))
    return php.MatchArm(None, body, **ctx.lineno(ts_node))


@_handler("interface_declaration")
def interface_decl(ts_node, ctx):
    name = ctx.text(ts_node.child_by_field_name("name"))
    extends = None
    for c in ts_node.named_children:
        if c.type == "base_clause":
            ext_names = [ctx.text(nc) for nc in c.named_children]
            extends = ext_names[0] if len(ext_names) == 1 else ext_names
            break
    body_node = ts_node.child_by_field_name("body")
    body = []
    if body_node:
        for c in body_node.children:
            if c.type in ("{", "}", ";", None):
                continue
            res = ctx.translate(c)
            if res is not None:
                if isinstance(res, list):
                    body.extend(res)
                else:
                    body.append(res)
    return php.Interface(name, extends, body, **ctx.lineno(ts_node))


@_handler("function_static_declaration")
def function_static_decl(ts_node, ctx):
    vars_ = []
    for c in ts_node.named_children:
        name_node = c.child_by_field_name("name")
        value_node = c.child_by_field_name("value")
        name = ctx.text(name_node) if name_node else None
        value = ctx.translate(value_node) if value_node else None
        v = php.StaticVariable(name, value, **ctx.lineno(c))
        vars_.append(v)
    return php.Static(vars_, **ctx.lineno(ts_node))


@_handler("return_statement")
def return_stmt(ts_node, ctx):
    expr = ctx.translate(ts_node.named_children[0]) if ts_node.named_children else None
    return php.Return(expr, **ctx.lineno(ts_node))


@_handler("exit_statement")
def exit_stmt(ts_node, ctx):
    arg = None
    for c in ts_node.named_children:
        r = ctx.translate(c)
        if r is not None:
            arg = r
            break
    t = "exit" if "exit" in ctx.text(ts_node).lower() else "die"
    return php.Exit(arg, t, **ctx.lineno(ts_node))


@_handler(
    "include_expression",
    "include_once_expression",
    "require_expression",
    "require_once_expression",
)
def include_expr(ts_node, ctx):
    nc = ts_node.named_children
    expr = ctx.translate(nc[0]) if nc else None
    t = ts_node.type
    once = "once" in t
    if t.startswith("include"):
        return php.Include(expr, once, **ctx.lineno(ts_node))
    else:
        return php.Require(expr, once, **ctx.lineno(ts_node))


@_handler("global_statement", "global_declaration")
def global_stmt(ts_node, ctx):
    vars_ = [
        ctx.translate(c)
        for c in ts_node.named_children
        if ctx.text(c).lower() not in ("global",)
    ]
    return php.Global(vars_, **ctx.lineno(ts_node))


@_handler("const_declaration")
def const_decl(ts_node, ctx):
    consts = []
    for c in ts_node.named_children:
        if c.type == "const_element":
            nc = c.named_children
            name = ctx.text(nc[0]) if nc else ""
            value = ctx.translate(nc[1]) if len(nc) > 1 else None
            consts.append(php.ConstantDeclaration(name, value))
    return php.ConstantDeclarations(consts, **ctx.lineno(ts_node))


@_handler("declare_statement")
def declare_stmt(ts_node, ctx):
    directives = []
    body_nodes = []
    for c in ts_node.named_children:
        if c.type == "declare_directive":
            text = ctx.text(c).strip()
            parts = text.split("=", 1)
            name = parts[0].strip()
            value_n = c.named_children[0] if c.named_children else None
            value = ctx.translate(value_n) if value_n else None
            if value is None and len(parts) > 1:
                value = int(parts[1]) if parts[1].isdigit() else parts[1]
            directives.append(php.Directive(name, value, **ctx.lineno(c)))
        else:
            body_nodes.append(c)
    body = None
    if body_nodes:
        stmts = []
        for bn in body_nodes:
            res = ctx.translate(bn)
            if res is not None:
                if isinstance(res, list):
                    stmts.extend(res)
                else:
                    stmts.append(res)
        if len(stmts) == 1 and isinstance(stmts[0], php.Block):
            body = stmts[0]
        elif stmts:
            body = php.Block(stmts, **ctx.lineno(ts_node))
    return php.Declare(directives, body, **ctx.lineno(ts_node))


@_handler("namespace_use_declaration")
def namespace_use_decl(ts_node, ctx):
    decls = []
    for c in ts_node.children:
        if c.type == "namespace_use_clause":
            decls.append(ctx.translate(c))
    return php.UseDeclarations(decls, **ctx.lineno(ts_node)) if decls else None


@_handler("use_declaration")
def trait_use_decl(ts_node, ctx):
    name = ""
    modifiers = []
    for c in ts_node.named_children:
        if c.type == "name":
            name = ctx.text(c)
        elif c.type == "use_list":
            for sub in c.named_children:
                if sub.type == "use_as_clause":
                    modifiers.append(ctx.translate(sub))
    return php.TraitUse(name, modifiers, **ctx.lineno(ts_node))


@_handler("use_as_clause")
def use_as_clause(ts_node, ctx):
    named = ts_node.named_children
    original = ctx.translate(named[0]) if len(named) > 0 else None
    alias = None
    visibility = None
    for c in named[1:]:
        if c.type == "visibility_modifier":
            visibility = ctx.text(c).strip().lower()
        elif c.type in ("name", "qualified_name"):
            alias = ctx.text(c)
    if isinstance(original, php.Constant):
        original = original.name
    return php.TraitModifier(original, alias, visibility, **ctx.lineno(ts_node))


@_handler("use_list")
def use_list(ts_node, ctx):
    return [
        ctx.translate(c) for c in ts_node.named_children if c.type == "use_as_clause"
    ]


@_handler("namespace_use_clause")
def namespace_use_clause(ts_node, ctx):
    named = ts_node.named_children
    name = ctx.text(named[0]) if named else ""
    alias_node = ts_node.child_by_field_name("alias")
    alias = ctx.text(alias_node) if alias_node else None
    return php.UseDeclaration(name, alias)


# ─── 5. Operators ─────────────────────────────────────────────────────


@_handler("assignment_expression", "augmented_assignment_expression")
def assignment(ts_node, ctx):
    left_node = ts_node.child_by_field_name("left")
    left = ctx.translate(left_node)
    right = ctx.translate(ts_node.child_by_field_name("right"))
    op_node = next(
        (c for c in ts_node.children if not c.is_named and "=" in ctx.text(c)), None
    )
    op = ctx.text(op_node).strip() if op_node else "="
    if op == "=":
        if left_node and left_node.type == "list_literal":
            return php.ListAssignment(
                left if isinstance(left, list) else [left],
                right,
                **ctx.lineno(ts_node),
            )
        return php.Assignment(left, right, False, **ctx.lineno(ts_node))
    if op == "=&":
        return php.Assignment(left, right, True, **ctx.lineno(ts_node))
    return php.AssignOp(op, left, right, **ctx.lineno(ts_node))


@_handler("reference_assignment_expression")
def reference_assignment(ts_node, ctx):
    named = ts_node.named_children
    left = ctx.translate(named[0]) if named else None
    right = ctx.translate(named[1]) if len(named) > 1 else None
    return php.Assignment(left, right, True, **ctx.lineno(ts_node))


@_handler("update_expression")
def update_expr(ts_node, ctx):
    op_text = ""
    for c in ts_node.children:
        if not c.is_named:
            op_text += ctx.text(c).strip()
    nc = ts_node.named_children
    var = ctx.translate(nc[0]) if nc else None
    if ts_node.children[0].is_named:
        return php.PostIncDecOp(op_text, var, **ctx.lineno(ts_node))
    return php.PreIncDecOp(op_text, var, **ctx.lineno(ts_node))


@_handler("unary_op_expression")
def unary(ts_node, ctx):
    op = ctx.text(ts_node.children[0]).strip()
    return php.UnaryOp(
        op, ctx.translate(ts_node.named_children[0]), **ctx.lineno(ts_node)
    )


@_handler("binary_expression")
def binary(ts_node, ctx):
    left = ctx.translate(ts_node.child_by_field_name("left"))
    right = ctx.translate(ts_node.child_by_field_name("right"))
    op = "".join(
        ctx.text(c) for c in ts_node.children if not c.is_named and ctx.text(c).strip()
    ).strip()
    return php.BinaryOp(
        op.lower() if op.lower() == "instanceof" else op,
        left,
        right,
        **ctx.lineno(ts_node),
    )


@_handler("conditional_expression")
def conditional(ts_node, ctx):
    cond = ctx.translate(ts_node.child_by_field_name("condition"))
    body = ctx.translate(ts_node.child_by_field_name("body"))
    alt = ctx.translate(ts_node.child_by_field_name("alternative"))
    return php.TernaryOp(
        cond, body if body is not None else cond, alt, **ctx.lineno(ts_node)
    )


@_handler("cast_expression")
def cast_expr(ts_node, ctx):
    type_name = ""
    for c in ts_node.children:
        if c.type == "cast_type":
            type_name = ctx.text(c).lower()
            break
    mapping = {"boolean": "bool", "real": "double", "float": "double", "integer": "int"}
    type_name = mapping.get(type_name, type_name)
    expr_node = ts_node.named_children[-1] if ts_node.named_children else None
    expr = ctx.translate(expr_node) if expr_node else None
    return php.Cast(type_name, expr, **ctx.lineno(ts_node))


# ─── 5.5. Dynamic Variable Names ──────────────────────────────────────


def _dv_translate(node, ctx):
    """Translate inside dynamic_variable_name context."""
    if node.type == "name":
        return php.Variable("$" + ctx.text(node), **ctx.lineno(node))
    if node.type == "subscript_expression":
        obj = _dv_translate(node.named_children[0], ctx)
        idx = (
            ctx.translate(node.named_children[1])
            if len(node.named_children) > 1
            else None
        )
        if isinstance(idx, php.Constant):
            idx = idx.name
        return php.ArrayOffset(obj, idx, **ctx.lineno(node))
    if node.type == "member_access_expression":
        obj = _dv_translate(node.child_by_field_name("object"), ctx)
        name_n = node.child_by_field_name("name")
        nm = ctx.text(name_n) if name_n else ""
        return php.ObjectProperty(obj, nm, **ctx.lineno(node))
    if node.type == "scoped_property_access_expression":
        scope_n = node.child_by_field_name("scope")
        scope = ctx.text(scope_n) if scope_n else ""
        name_n = node.child_by_field_name("name")
        if name_n and name_n.type == "variable_name":
            nm = ctx.translate(name_n)
        else:
            nm = ctx.text(name_n) if name_n else ""
        return php.StaticProperty(scope, nm, **ctx.lineno(node))
    if node.type == "class_constant_access_expression":
        nc = node.named_children
        scope = ctx.text(nc[0]) if len(nc) > 0 else ""
        nm = ctx.text(nc[1]) if len(nc) > 1 else ""
        return php.StaticProperty(scope, nm, **ctx.lineno(node))
    if node.type == "member_call_expression":
        obj = _dv_translate(node.child_by_field_name("object"), ctx)
        name_n = node.child_by_field_name("name")
        nm = ctx.text(name_n) if name_n else ""
        args_node = node.child_by_field_name("arguments")
        args = ctx.translate(args_node) if args_node else None
        if args is None:
            args = []
        return php.MethodCall(obj, nm, args, **ctx.lineno(node))
    if node.type == "function_call_expression":
        fn_node = node.child_by_field_name("function")
        name = ctx.text(fn_node) if fn_node else ""
        args_node = node.child_by_field_name("arguments")
        args = ctx.translate(args_node) if args_node else None
        if args is None:
            args = []
        return php.FunctionCall(name, args, **ctx.lineno(node))
    return ctx.translate(node)


@_handler("dynamic_variable_name")
def dynamic_variable_name(ts_node, ctx):
    inner = ts_node.named_children[0] if ts_node.named_children else None
    if inner is None:
        return None
    if inner.type == "name":
        return php.Variable("$" + ctx.text(inner), **ctx.lineno(inner))
    elif inner.type == "dynamic_variable_name":
        return php.Variable(ctx.translate(inner), **ctx.lineno(inner))
    elif inner.type == "variable_name":
        inner_res = ctx.translate(inner)
        if ctx.string_mode:
            return inner_res
        return php.Variable(inner_res, **ctx.lineno(inner))
    else:
        inner_res = _dv_translate(inner, ctx)
        if ctx.string_mode:
            return inner_res
        return php.Variable(inner_res, **ctx.lineno(inner))


# ─── 6. Access & Calls ────────────────────────────────────────────────


@_handler("member_access_expression")
def member_access(ts_node, ctx):
    obj = ctx.translate(ts_node.child_by_field_name("object"))
    name_n = ts_node.child_by_field_name("name")
    name = (
        ctx.translate(name_n)
        if name_n and name_n.type in ("variable_name", "variable_variable")
        else ctx.text(name_n)
        if name_n
        else ""
    )
    return php.ObjectProperty(obj, name, **ctx.lineno(ts_node))


@_handler("subscript_expression")
def subscript(ts_node, ctx):
    obj_node = ts_node.named_children[0]
    obj = ctx.translate(obj_node)
    idx = (
        ctx.translate(ts_node.named_children[1])
        if len(ts_node.named_children) > 1
        else None
    )
    if isinstance(idx, php.Constant):
        idx = idx.name
    if obj_node.type == "member_access_expression":
        name_n = obj_node.child_by_field_name("name")
        if name_n and name_n.type == "variable_name":
            obj_part = ctx.translate(obj_node.child_by_field_name("object"))
            name_part = ctx.translate(name_n)
            new_name = php.ArrayOffset(name_part, idx, **ctx.lineno(ts_node))
            return php.ObjectProperty(obj_part, new_name, **ctx.lineno(ts_node))
    if "{" in ctx.text(ts_node):
        return php.StringOffset(obj, idx, **ctx.lineno(ts_node))
    return php.ArrayOffset(obj, idx, **ctx.lineno(ts_node))


@_handler("scoped_property_access_expression")
def scoped_property(ts_node, ctx):
    scope_n = ts_node.child_by_field_name("scope")
    scope = _scope_text(scope_n, ctx)
    name_n = ts_node.child_by_field_name("name")
    if name_n and name_n.type in ("variable_name", "variable_variable"):
        name = ctx.translate(name_n)
    else:
        name = ctx.text(name_n) if name_n else ""
    return php.StaticProperty(scope, name, **ctx.lineno(ts_node))


@_handler("class_constant_access_expression")
def class_constant_access(ts_node, ctx):
    named = ts_node.named_children
    scope_n = named[0] if len(named) > 0 else None
    scope = _scope_text(scope_n, ctx)
    name_n = named[1] if len(named) > 1 else None
    if name_n and name_n.type == "name" and name_n.named_children:
        inner_name = name_n.named_children[0]
        if inner_name.type == "variable_name":
            name = ctx.translate(inner_name)
        else:
            name = ctx.text(name_n) if name_n else ""
    else:
        name = ctx.text(name_n) if name_n else ""
    if isinstance(name, str) and name.lower() == "class":
        return scope
    return php.StaticProperty(scope, name, **ctx.lineno(ts_node))


@_handler("function_call_expression")
def function_call(ts_node, ctx):
    fn_node = ts_node.child_by_field_name("function")
    fn_val = ctx.translate(fn_node)
    args_node = ts_node.child_by_field_name("arguments")
    args = [ctx.translate(a) for a in (args_node.named_children if args_node else [])]
    name = fn_val.name if isinstance(fn_val, php.Constant) else fn_val
    if isinstance(name, str):
        nl = name.lower()
        if nl in ("exit", "die"):
            arg = args[0].node if args else None
            return php.Exit(arg, nl, **ctx.lineno(ts_node))
        if nl == "isset":
            return php.IsSet(
                [a.node if isinstance(a, php.Parameter) else a for a in args],
                **ctx.lineno(ts_node),
            )
        if nl == "empty":
            arg = args[0].node if args else None
            return php.Empty(arg, **ctx.lineno(ts_node))
        if nl == "eval":
            arg = args[0].node if args else None
            return php.Eval(arg, **ctx.lineno(ts_node))
    return php.FunctionCall(name, args, **ctx.lineno(ts_node))


@_handler("scoped_call_expression")
def scoped_call(ts_node, ctx):
    scope_n = ts_node.child_by_field_name("scope")
    scope = _scope_text(scope_n, ctx)
    name_n = ts_node.child_by_field_name("name")
    if name_n and name_n.type in ("variable_name", "variable_variable"):
        method_name = ctx.translate(name_n)
    else:
        method_name = ctx.text(name_n) if name_n else ""
    args_node = ts_node.child_by_field_name("arguments")
    args = [ctx.translate(a) for a in (args_node.named_children if args_node else [])]
    return php.StaticMethodCall(scope, method_name, args, **ctx.lineno(ts_node))


@_handler("member_call_expression")
def member_call(ts_node, ctx):
    obj = ctx.translate(ts_node.child_by_field_name("object"))
    name_n = ts_node.child_by_field_name("name")
    name = (
        ctx.translate(name_n)
        if name_n and name_n.type in ("variable_name", "variable_variable")
        else ctx.text(name_n)
        if name_n
        else ""
    )
    args = [
        ctx.translate(a)
        for a in ts_node.child_by_field_name("arguments").named_children
    ]
    return php.MethodCall(obj, name, args, **ctx.lineno(ts_node))


@_handler("nullsafe_member_access_expression")
def nullsafe_access(ts_node, ctx):
    obj = ctx.translate(ts_node.child_by_field_name("object"))
    name_n = ts_node.child_by_field_name("name")
    name = (
        ctx.translate(name_n)
        if name_n and name_n.type == "variable_name"
        else ctx.text(name_n)
        if name_n
        else ""
    )
    return php.NullsafePropertyAccess(obj, name, **ctx.lineno(ts_node))


@_handler("nullsafe_member_call_expression")
def nullsafe_call(ts_node, ctx):
    obj = ctx.translate(ts_node.child_by_field_name("object"))
    name_n = ts_node.child_by_field_name("name")
    name = ctx.text(name_n) if name_n else ""
    args = [
        ctx.translate(a)
        for a in ts_node.child_by_field_name("arguments").named_children
    ]
    return php.NullsafeCall(obj, name, args, **ctx.lineno(ts_node))


@_handler("object_creation_expression")
def new(ts_node, ctx):
    cls_node = None
    args_node = None
    for c in ts_node.children:
        if c.is_named and c.type != "arguments":
            cls_node = c
        elif c.type == "arguments":
            args_node = c
    cls_name = ctx.text(cls_node) if cls_node else ""
    args = [ctx.translate(a) for a in (args_node.named_children if args_node else [])]
    return php.New(cls_name, args, **ctx.lineno(ts_node))


@_handler("argument")
def argument(ts_node, ctx):
    name_node = ts_node.child_by_field_name("name")
    if name_node:
        value = ctx.translate(ts_node.named_children[-1])
        return php.NamedArgument(
            ctx.text(name_node), value, **ctx.lineno(ts_node)
        )
    return php.Parameter(
        ctx.translate(ts_node.named_children[-1]),
        "&" in ctx.text(ts_node),
        **ctx.lineno(ts_node),
    )


# ─── 7. Flow Control ──────────────────────────────────────────────────


@_handler("yield_expression")
def yield_expr(ts_node, ctx):
    expr = None
    for c in ts_node.named_children:
        if c.type == "yield":
            continue
        r = ctx.translate(c)
        if r is not None:
            expr = r
            break
    return php.Yield(expr, **ctx.lineno(ts_node))


@_handler("array_element_initializer")
def array_element_init(ts_node, ctx):
    return ctx.translate(ts_node.named_children[-1])


@_handler("throw_expression")
def throw_expr(ts_node, ctx):
    expr = ctx.translate(ts_node.named_children[0]) if ts_node.named_children else None
    return php.Throw(expr, **ctx.lineno(ts_node))


@_handler("echo_statement", "echo_stdout")
def echo(ts_node, ctx):
    items = [ctx.translate(c) for c in ts_node.named_children if c.type != "echo"]
    return php.Echo(items, **ctx.lineno(ts_node))


@_handler("if_statement")
def if_stmt(ts_node, ctx):
    cond = ctx.translate(ts_node.child_by_field_name("condition"))
    body = ctx.translate(ts_node.child_by_field_name("body"))
    elseifs = []
    else_node = None
    extra_body = []
    for c in ts_node.named_children:
        if c.type == "else_if_clause":
            e_cond = ctx.translate(c.child_by_field_name("condition"))
            e_body = ctx.translate(c.child_by_field_name("body"))
            elseifs.append(php.ElseIf(e_cond, e_body, **ctx.lineno(c)))
        elif c.type == "else_clause":
            e_body = ctx.translate(c.child_by_field_name("body"))
            else_node = php.Else(e_body, **ctx.lineno(c))
        elif c.type == "text_interpolation":
            res = ctx.translate(c)
            if res is not None:
                extra_body.append(res)
            else:
                extra_body.append(None)
    if extra_body:
        if isinstance(body, php.Block):
            body.nodes.extend(extra_body)
        elif body is not None:
            body = php.Block(
                [body] + extra_body,
                **ctx.lineno(body if not isinstance(body, list) else ts_node),
            )
        else:
            body = php.Block(extra_body, **ctx.lineno(ts_node))
    return php.If(cond, body, elseifs, else_node, **ctx.lineno(ts_node))


@_handler("foreach_statement")
def foreach_stmt(ts_node, ctx):
    named = ts_node.named_children
    arr = ctx.translate(named[0])
    key = None
    is_ref = False
    second = named[1]
    if second.type == "pair":
        key = ctx.translate(second.named_children[0])
        val_inner = second.named_children[1]
        if val_inner.type == "by_ref":
            is_ref = True
            val_node = val_inner.named_children[0]
        else:
            val_node = val_inner
    elif second.type == "by_ref":
        is_ref = True
        val_node = second.named_children[0]
    else:
        val_node = second
    val = php.ForeachVariable(ctx.translate(val_node), is_ref, **ctx.lineno(val_node))
    body_node = named[2] if len(named) > 2 else None
    if body_node is None:
        body = php.Block([], **ctx.lineno(ts_node))
    else:
        body = ctx.translate(body_node)
        if isinstance(body, list):
            body = php.Block(body, **ctx.lineno(body_node))
    return php.Foreach(arr, key, val, body, **ctx.lineno(ts_node))


@_handler("compound_statement")
def compound(ts_node, ctx):
    nodes = []
    for c in ts_node.children:
        if c.type not in ("{", "}"):
            res = ctx.translate(c)
            if res is not None:
                if isinstance(res, list):
                    nodes.extend(res)
                else:
                    nodes.append(res)
    return php.Block(nodes, **ctx.lineno(ts_node))


@_handler("colon_block")
def colon_block(ts_node, ctx):
    nodes = []
    for c in ts_node.named_children:
        res = ctx.translate(c)
        if res is not None:
            if isinstance(res, list):
                nodes.extend(res)
            else:
                nodes.append(res)
    return php.Block(nodes, **ctx.lineno(ts_node))


# ─── 8. Class & Function ──────────────────────────────────────────────


@_handler("property_declaration")
def property_decl(ts_node, ctx):
    modifiers = []
    elements = []
    for c in ts_node.children:
        if c.type == "visibility_modifier":
            modifiers.append(ctx.text(c).strip().lower())
        elif c.type == "static_modifier":
            modifiers.append("static")
        elif c.type == "readonly_modifier":
            modifiers.append("readonly")
        elif c.type == "property_element":
            name_n = c.child_by_field_name("name")
            val_n = c.child_by_field_name("default_value")
            nm = ctx.text(name_n) if name_n else None
            val = ctx.translate(val_n) if val_n else None
            elements.append(php.ClassVariable(nm, val))
    return php.ClassVariables(modifiers, elements, **ctx.lineno(ts_node))


@_handler("method_declaration")
def method_decl(ts_node, ctx):
    old_method = ctx.method
    old_fn = ctx.fn
    modifiers = []
    for c in ts_node.children:
        if c.type == "visibility_modifier":
            modifiers.append(ctx.text(c).strip().lower())
        elif c.type == "static_modifier":
            modifiers.append("static")
        elif c.type == "final_modifier":
            modifiers.append("final")
        elif c.type == "abstract_modifier":
            modifiers.append("abstract")
    name = ctx.text(ts_node.child_by_field_name("name"))
    qualified = f"{ctx.cls}::{name}" if ctx.cls else name
    ctx.method = qualified
    ctx.fn = qualified
    params_node = ts_node.child_by_field_name("parameters")
    params = [
        ctx.translate(p) for p in (params_node.named_children if params_node else [])
    ]
    body = ctx.translate(ts_node.child_by_field_name("body"))
    body_stmts = body.nodes if isinstance(body, php.Block) else ([body] if body else [])
    result = php.Method(name, modifiers, params, body_stmts, False, **ctx.lineno(ts_node))
    ctx.method = old_method
    ctx.fn = old_fn
    return result


@_handler("class_declaration")
def class_decl(ts_node, ctx):
    old_cls = ctx.cls
    name = ctx.text(ts_node.child_by_field_name("name"))
    ctx.cls = f"{ctx.ns}\\{name}" if ctx.ns else name
    modifiers = []
    base_name = None
    interfaces = []

    for c in ts_node.children:
        if c.type in ("final_modifier", "abstract_modifier", "readonly_modifier"):
            modifiers.append(ctx.text(c).lower().strip())
        elif c.type == "base_clause":
            for bc in c.named_children:
                base_name = ctx.text(bc)
                break
        elif c.type == "class_interface_clause":
            for ic in c.named_children:
                interfaces.append(ctx.text(ic))

    body_node = ts_node.child_by_field_name("body")
    body = []
    uses = []
    if body_node:
        for c in body_node.children:
            if c.type in ("{", "}", ";", None):
                continue
            if c.type == "const_declaration":
                consts = []
                for elem_node in c.named_children:
                    if elem_node.type == "const_element":
                        nc = elem_node.named_children
                        nm = ctx.text(nc[0]) if nc else ""
                        val = ctx.translate(nc[1]) if len(nc) > 1 else None
                        consts.append(php.ClassConstant(nm, val))
                if consts:
                    body.append(php.ClassConstants(consts, **ctx.lineno(c)))
            elif c.type == "use_declaration":
                res = ctx.translate(c)
                if res is not None:
                    uses.append(res)
            else:
                res = ctx.translate(c)
                if res is not None:
                    if isinstance(res, list):
                        body.extend(res)
                    else:
                        body.append(res)

    result = php.Class(
        name,
        modifiers[0] if modifiers else None,
        base_name,
        interfaces,
        uses,
        body,
        **ctx.lineno(ts_node),
    )
    ctx.cls = old_cls
    return result


@_handler("trait_declaration")
def trait_decl(ts_node, ctx):
    old_trait = ctx.trait
    old_cls = ctx.cls
    name = ctx.text(ts_node.child_by_field_name("name"))
    full_name = f"{ctx.ns}\\{name}" if ctx.ns else name
    ctx.trait = full_name
    ctx.cls = full_name
    body_node = ts_node.child_by_field_name("body")
    body = []
    uses = []
    if body_node:
        for c in body_node.children:
            if c.type in ("{", "}", ";", None):
                continue
            if c.type == "use_declaration":
                res = ctx.translate(c)
                if res is not None:
                    uses.append(res)
            else:
                res = ctx.translate(c)
                if res is not None:
                    if isinstance(res, list):
                        body.extend(res)
                    else:
                        body.append(res)
    result = php.Trait(name, uses, body, **ctx.lineno(ts_node))
    ctx.trait = old_trait
    ctx.cls = old_cls
    return result


@_handler("function_definition")
def function_def(ts_node, ctx):
    old_fn = ctx.fn
    name = ctx.text(ts_node.child_by_field_name("name"))
    ctx.fn = f"{ctx.ns}\\{name}" if ctx.ns else name
    params_node = ts_node.child_by_field_name("parameters")
    params = [
        ctx.translate(p) for p in (params_node.named_children if params_node else [])
    ]
    body = ctx.translate(ts_node.child_by_field_name("body"))
    body_stmts = body.nodes if isinstance(body, php.Block) else ([body] if body else [])
    return_type_node = ts_node.child_by_field_name("return_type")
    return_type = ctx.text(return_type_node) if return_type_node else None
    result = F(php.Function, name, params, body_stmts, False, return_type, **ctx.lineno(ts_node))
    ctx.fn = old_fn
    return result


@_handler("simple_parameter")
def simple_param(ts_node, ctx):
    type_node = ts_node.child_by_field_name("type")
    name_node = ts_node.child_by_field_name("name")
    default_node = ts_node.child_by_field_name("default_value")
    type_name = ctx.text(type_node) if type_node else None
    name = ctx.text(name_node) if name_node else ""
    default_val = ctx.translate(default_node) if default_node else None
    is_ref = any(
        (not c.is_named and ctx.text(c).strip() == "&")
        or c.type == "reference_modifier"
        for c in ts_node.children
    )
    return php.FormalParameter(
        name, default_val, is_ref, type_name, **ctx.lineno(ts_node)
    )


@_handler("property_promotion_parameter")
def property_promotion_param(ts_node, ctx):
    modifiers = []
    type_node = ts_node.child_by_field_name("type")
    name_node = ts_node.child_by_field_name("name")
    default_node = ts_node.child_by_field_name("default_value")
    for c in ts_node.children:
        if c.type == "visibility_modifier":
            modifiers.append(ctx.text(c).strip().lower())
    type_name = ctx.text(type_node) if type_node else None
    name = ctx.text(name_node) if name_node else ""
    default_val = ctx.translate(default_node) if default_node else None
    return php.ConstructorParameter(
        modifiers, name, type_name, default_val, **ctx.lineno(ts_node)
    )


@_handler("try_statement")
def try_stmt(ts_node, ctx):
    body = ctx.translate(ts_node.child_by_field_name("body")) or []
    if isinstance(body, php.Block):
        body = body.nodes
    elif not isinstance(body, list):
        body = [body] if body else []
    catches, finally_block = [], None
    for c in ts_node.children:
        if c.type == "catch_clause":
            catches.append(ctx.translate(c))
        elif c.type == "finally_clause":
            finally_block = ctx.translate(c)
    return php.Try(body, catches, finally_block, **ctx.lineno(ts_node))


@_handler("catch_clause")
def catch_clause(ts_node, ctx):
    type_name = ctx.text(ts_node.child_by_field_name("type"))
    var = ctx.translate(ts_node.child_by_field_name("name"))
    body = ctx.translate(ts_node.child_by_field_name("body"))
    body_stmts = body.nodes if isinstance(body, php.Block) else ([body] if body else [])
    return php.Catch(type_name, var, body_stmts, **ctx.lineno(ts_node))


@_handler("finally_clause")
def finally_clause(ts_node, ctx):
    body = ctx.translate(ts_node.child_by_field_name("body"))
    body_stmts = body.nodes if isinstance(body, php.Block) else ([body] if body else [])
    return php.Finally(body_stmts, **ctx.lineno(ts_node))


# ─── main translate ───────────────────────────────────────────────────


def translate(ts_node, ctx):
    if ts_node is None:
        return None

    handler = _HANDLERS.get(ts_node.type)
    if handler:
        return handler(ts_node, ctx)

    # Fallback: translate named children
    logger.warning(
        "No handler for node type %r at line %d. Falling back to named children.",
        ts_node.type,
        ts_node.start_point[0] + 1,
    )
    if ts_node.named_children:
        res = []
        for c in ts_node.named_children:
            r = translate(c, ctx)
            if r is not None:
                if isinstance(r, list):
                    res.extend(r)
                else:
                    res.append(r)
        if res:
            return res
    return None


class Bashe:
    def __init__(self, legacy=False):
        _PhpAstProxy.configure(legacy=legacy)

    def parse(self, code: str, filename: str = None):
        src = bytes(code, "utf8")
        tree = _parser.parse(src)
        ctx = Ctx(src, filename)
        res = translate(tree.root_node, ctx)
        return res if isinstance(res, list) else [res] if res else []
