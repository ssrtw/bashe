# Bashe

<p align="center">
  <img src="assets/logo.png" alt="Bashe logo" width="256">
</p>

[![GitHub release](https://img.shields.io/github/release/ssrtw/bashe/all.svg)](https://github.com/ssrtw/bashe/releases)
[![license](https://img.shields.io/github/license/ssrtw/bashe.svg)](./LICENSE)
![python badge](https://img.shields.io/badge/python-3.10+-blue.svg)
![rust badge](https://img.shields.io/badge/engine-Rust_PyO3-maroon)

Bashe (巴蛇) is a fast, tree-sitter based PHP parser for Python, powered by
**Rust + PyO3**. It parses PHP source into a clean, structured AST with
full PHP 7/8 support — and runs **5× faster** than phply.

## Features

- Parses PHP into a structured AST via tree-sitter
- Full PHP 7 support: scalar types, return type declarations,
  null coalescing (`??`), spaceship operator (`<=>`)
- Full PHP 8 support: constructor property promotion, named
  arguments, nullsafe operator (`?->`), match expressions, union types
- Magic constants (`__FILE__`, `__DIR__`, `__NAMESPACE__`,
  `__FUNCTION__`, etc.) resolved during parsing
- Built on Rust/PyO3 — tree-sitter parsing and AST translation
  happen entirely in native code

## Installation

Python 3.10+ is supported.

```bash
pip install bashe
```

## Quick Start

```python
from bashe import Bashe

parser = Bashe()
ast = parser.parse('<?php echo "hello, world!"; ?>')
print(ast)

# Magic constants are resolved automatically
nodes = parser.parse(
    '<?php namespace Foo; class Bar { function baz() { echo __METHOD__; } } ?>',
    filename="/path/to/file.php",
)
```

## Performance

Bashe is approximately **5× faster** than phply's pure-Python parser.

| Parser | Time (10k) | Ratio     |
| ------ | ---------- | --------- |
| bashe  | 0.75s      | 1.00×     |
| phply  | 3.64s      | **4.85×** |

_Benchmark: parsing `bench.php` (multi-line PHP with includes, evals,
function definitions) 10,000 times on CPython 3.13._

## Migrating from phply

Bashe is a drop-in replacement for phply:

```python
# Before (phply)
from phply.phpparse import make_parser
parser = make_parser()
ast = parser.parse(code)

# After (bashe)
from bashe import Bashe
parser = Bashe()
ast = parser.parse(code)
```

Bashe uses its own fast native AST types (`Variable`, `FunctionCall`,
`Function`, `Class`, etc.) — no dependency on phply.

## Development

```bash
# Install dependencies and dev tools
uv sync --dev

# Run tests
uv run pytest
```

## License

MIT — see [LICENSE](./LICENSE).
