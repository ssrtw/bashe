"""Shared test utilities."""

from bashe.parser import Bashe

parser = Bashe()


def eq_ast(input, expected, filename=None, with_top_lineno=False):
    output = parser.parse(input, filename)

    diff = None
    try:
        for out, exp in zip(output, expected):
            if out != exp:
                diff = f"\n  got: {out!r}\n  exp: {exp!r}"
                break
        else:
            if len(output) != len(expected):
                diff = f"\n  length: {len(output)} vs {len(expected)}"
            else:
                if with_top_lineno:
                    for out, exp in zip(output, expected):
                        assert out.lineno == exp.lineno, (
                            f"got line: {out.lineno}, expected:{exp.lineno}"
                        )
                return
    except Exception as exc:
        diff = f"\n  error: {exc}"

    print("\n── output ──")
    for o in output:
        print(f"  {o!r}")
    print("── expected ──")
    for e in expected:
        print(f"  {e!r}")
    print(f"len: {len(output)} vs {len(expected)}")

    msg = "mismatch"
    if diff:
        msg += diff
    raise AssertionError(msg)
