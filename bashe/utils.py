"""Utility helpers for building AST nodes across native / phply backends."""


def F(Function, name, params, nodes, is_ref, return_type=None, **kwargs):
    """Create a Function node with the correct number of positional args.

    *phply*'s ``Function`` has 4 fields; native ``Function`` (from
    ``bashe.types``) has 5 fields (adds *return_type*).  This helper
    inspects ``Function.fields`` and pads or strips the ``return_type``
    argument so the call succeeds in either backend.
    """
    if len(Function.fields) == 4:
        return Function(name, params, nodes, is_ref, **kwargs)
    return Function(name, params, nodes, is_ref, return_type, **kwargs)
