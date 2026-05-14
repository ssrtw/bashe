"""Utility helpers for building AST nodes across native / phply backends."""

from typing import Any


def F(
    Function: type,
    name: str,
    params: list[Any],
    nodes: list[Any],
    is_ref: bool,
    return_type: str | None = None,
    **kwargs: Any,
) -> Any:
    """Create a Function node with the correct number of positional args.

    *phply*'s ``Function`` has 4 fields; native ``Function`` (from
    ``bashe.types``) has 5 fields (adds *return_type*).  This helper
    inspects ``Function.fields`` and pads or strips the ``return_type``
    argument so the call succeeds in either backend.

    Args:
        Function: The ``Function`` class from the active AST backend.
        name (str): The function name.
        params (list): List of formal parameter nodes.
        nodes (list): List of statement nodes in the function body.
        is_ref (bool): Whether the function returns by reference.
        return_type (str | None): Optional return type annotation.
        **kwargs: Additional keyword arguments (e.g. ``lineno``).

    Returns:
        A ``Function`` AST node with the correct number of positional
        arguments for the active backend.
    """
    if len(Function.fields) == 4:
        return Function(name, params, nodes, is_ref, **kwargs)
    return Function(name, params, nodes, is_ref, return_type, **kwargs)
