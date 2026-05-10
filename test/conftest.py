import warnings

warnings.filterwarnings("ignore", message=r".*phply as AST backend.*", category=DeprecationWarning)


def pytest_collection_finish(session):
    with warnings.catch_warnings():
        warnings.simplefilter("ignore", DeprecationWarning)
        from bashe.parser import php

        print(f"\n── PHP AST module: {php.__name__} ──")
