import timeit
from pathlib import Path

from phply import phplex  # noqa: F401
from phply.phpparse import make_parser  # noqa: F401

from bashe import Bashe

in_text = Path("bench.php").read_text()

bashe = Bashe()
phply = make_parser()

bashe_time = timeit.timeit(
    "bashe.parse(in_text)",
    number=10000,
    globals=locals(),
)
phply_time = timeit.timeit(
    "lexer = phplex.lexer.clone(); phply.parse(in_text, lexer=lexer)",
    number=10000,
    globals=locals(),
)

print(f"bashe  : {bashe_time:.4f}s")
print(f"phply  : {phply_time:.4f}s")
print(f"phply / bashe: {phply_time / bashe_time:.2f}x")
