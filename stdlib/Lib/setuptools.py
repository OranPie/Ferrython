"""
setuptools — Minimal compatibility shim for Ferrython.

Provides setup(), find_packages(), and Entry Point support for basic
package compatibility. Full build system functionality is handled by ferryip.
"""

import os

__version__ = "69.0.0"


def setup(**attrs):
    """Record package metadata. In Ferrython, actual building is done by ferryip."""
    return attrs


def find_packages(where='.', exclude=(), include=('*',)):
    """Find all Python packages in a directory tree."""
    packages = []
    base = os.path.abspath(where)
    for root, dirs, files in os.walk(base):
        if '__init__.py' in files:
            rel = os.path.relpath(root, base)
            package = rel.replace(os.sep, '.')
            if package == '.':
                continue
            # Check exclude patterns
            skip = False
            for pat in exclude:
                if _match_pattern(package, pat):
                    skip = True
                    break
            if skip:
                continue
            # Check include patterns
            if include != ('*',):
                matched = False
                for pat in include:
                    if _match_pattern(package, pat):
                        matched = True
                        break
                if not matched:
                    continue
            packages.append(package)
    return sorted(packages)


def find_namespace_packages(where='.', exclude=(), include=('*',)):
    """Find all namespace packages (directories without __init__.py)."""
    packages = []
    base = os.path.abspath(where)
    for root, dirs, files in os.walk(base):
        rel = os.path.relpath(root, base)
        if rel == '.':
            continue
        package = rel.replace(os.sep, '.')
        if not package.startswith('_') and not any(c == '__pycache__' for c in rel.split(os.sep)):
            packages.append(package)
    return sorted(packages)


def _match_pattern(name, pattern):
    """Simple glob-like pattern matching for package names."""
    if pattern == '*':
        return True
    if pattern.endswith('*'):
        return name.startswith(pattern[:-1])
    if pattern.startswith('*'):
        return name.endswith(pattern[1:])
    return name == pattern


class Distribution:
    """Minimal Distribution class for package metadata."""

    def __init__(self, attrs=None):
        self.metadata = PackageMetadata()
        if attrs:
            for key, value in attrs.items():
                setattr(self.metadata, key, value)
                setattr(self, key, value)


class PackageMetadata:
    """Package metadata container."""

    def __init__(self):
        self.name = None
        self.version = None
        self.description = None
        self.long_description = None
        self.author = None
        self.author_email = None
        self.url = None
        self.license = None
        self.classifiers = []
        self.install_requires = []
        self.python_requires = None


class Command:
    """Base class for distutils/setuptools commands."""

    def __init__(self, dist=None):
        self.distribution = dist

    def initialize_options(self):
        pass

    def finalize_options(self):
        pass

    def run(self):
        pass


class Extension:
    """Describes a C/C++ extension module."""

    def __init__(self, name, sources, **kwargs):
        self.name = name
        self.sources = sources
        self.include_dirs = kwargs.get('include_dirs', [])
        self.define_macros = kwargs.get('define_macros', [])
        self.undef_macros = kwargs.get('undef_macros', [])
        self.library_dirs = kwargs.get('library_dirs', [])
        self.libraries = kwargs.get('libraries', [])
        self.extra_compile_args = kwargs.get('extra_compile_args', [])
        self.extra_link_args = kwargs.get('extra_link_args', [])
        self.language = kwargs.get('language', None)
