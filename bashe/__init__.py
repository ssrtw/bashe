"""Bashe - A tree-sitter backed PHP parser with native AST support.

Provides a :class:`Bashe` parser and a ``php`` proxy object that gives
access to AST node types.  By default this uses native
:mod:`bashe.types`; pass ``Bashe(legacy=True)`` to route through
:mod:`phply.phpast` for backwards compatibility.
"""

from .parser import Bashe, php

__all__ = ["Bashe", "php"]
