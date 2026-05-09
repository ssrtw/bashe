def pytest_configure(config):
    from bashe.parser import php

    print(f"\n── PHP AST module: {php.__name__} ──")
