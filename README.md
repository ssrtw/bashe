# Bashe

<p align="center">
  <img src="assets/logo.png" alt="Bashe logo" width="256">
</p>

[![GitHub release](https://img.shields.io/github/release/ssrtw/bashe/all.svg)](https://github.com/ssrtw/bashe/releases)
[![license](https://img.shields.io/github/license/ssrtw/bashe.svg)](./LICENSE)
![python version badge](https://img.shields.io/badge/language-python3.10-orange.svg)

Bashe (巴蛇, pronounced like "parser") is a fast, tree-sitter based PHP
parser for Python, with optional phply-compatible AST output. The name
comes from the legendary giant snake in Chinese mythology.

## Features

- Parses PHP source into a structured AST using tree-sitter
- Supports PHP 7 features: scalar types, return type declarations,
  null coalescing (`??`), spaceship operator (`<=>`)
- Supports PHP 8 features: constructor property promotion, named
  arguments, nullsafe operator (`?->`), match expressions, union types
- Optional `phply.phpparse`-compatible output — just `pip install bashe[phply]`
- Minimal dependencies when phply is not needed

## Installation

Python 3.10+ is supported; 3.13+ (with [uv](https://docs.astral.sh/uv/)) is
recommended for the best experience.

```bash
# Core parser only (tree-sitter + tree-sitter-php)
uv pip install bashe

# With phply compatibility layer
uv pip install bashe[phply]
```

## Quick Start

```python
from bashe import Bashe

parser = Bashe()
ast = parser.parse('<?php echo "hello, world!"; ?>')
print(ast)
```

For phply-compatible AST nodes (`phpast.Variable`, `phpast.FunctionCall`, etc.):

```bash
uv pip install bashe[phply]
```

```python
from bashe import Bashe

parser = Bashe()
nodes = parser.parse(
    '<?php namespace Foo; class Bar { function baz() { echo __METHOD__; } } ?>',
    filename="/path/to/file.php",
)
# All magic constants (__FILE__, __DIR__, __NAMESPACE__, __CLASS__,
# __FUNCTION__, __METHOD__, __TRAIT__) are resolved at parse time —
# no need for resolve_magic_constants().
```

## Performance

Bashe is approximately **2× faster** than phply's lexer-based parser on
equivalent inputs.

| Parser | 3.10 (10k) | Ratio (3.10) | 3.13 (10k) | Ratio (3.13) |
| ------ | ---------- | ------------ | ---------- | ------------ |
| bashe  | 2.46s      | 1.00×        | 1.73s      | 1.00×        |
| phply  | 5.76s      | 2.34×        | 3.66s      | 2.12×        |

\_Benchmark: parsing `bench.php` (multi-line PHP with includes, evals,
function definitions) 10,000 times on CPython 3.10 and 3.13

## Development

```bash
# Install dev dependencies (pytest, ipykernel, phply)
uv pip install -e ".[dev]"

# Run tests
uv run pytest
```

## License

MIT — see [LICENSE](./LICENSE).
