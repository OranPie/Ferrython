"""
importlib_metadata — Backport compatibility module.

In Python 3.8+, importlib.metadata is built-in. This module provides
the same interface for packages that import importlib_metadata directly.
"""

# Re-export everything from importlib.metadata
from importlib.metadata import *
from importlib.metadata import version, metadata, requires, packages_distributions
