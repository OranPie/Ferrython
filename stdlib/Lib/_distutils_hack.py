"""
_distutils_hack — Compatibility shim for setuptools' distutils override.

This module is used by setuptools to redirect distutils imports.
In Ferrython, it's a no-op since we provide our own distutils.
"""

def enable():
    """Enable distutils hack (no-op in Ferrython)."""
    pass

def disable():
    """Disable distutils hack (no-op in Ferrython)."""
    pass

_enabled = False

class DistutilsMetaFinder:
    """Meta-path finder for distutils override (no-op)."""
    def find_module(self, fullname, path=None):
        return None
