"""
distutils — Minimal compatibility shim.

The distutils module is deprecated in Python 3.10+ and removed in 3.12.
This provides the minimum interface needed for setuptools compatibility.
"""

class DistutilsError(Exception):
    pass

class DistutilsModuleError(DistutilsError):
    pass

class DistutilsFileError(DistutilsError):
    pass

class DistutilsOptionError(DistutilsError):
    pass

class DistutilsPlatformError(DistutilsError):
    pass

class DistutilsSetupError(DistutilsError):
    pass

class DistutilsArgError(DistutilsError):
    pass


def setup(**attrs):
    """Minimal setup() — records metadata but doesn't build anything."""
    return attrs


def find_packages(where='.', exclude=(), include=('*',)):
    """Find all packages in a directory."""
    import os
    packages = []
    base = os.path.abspath(where)
    for root, dirs, files in os.walk(base):
        if '__init__.py' in files:
            rel = os.path.relpath(root, base)
            package = rel.replace(os.sep, '.')
            if package == '.':
                continue
            packages.append(package)
    return packages
