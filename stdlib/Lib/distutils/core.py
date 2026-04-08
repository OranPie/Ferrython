"""
distutils.core — Minimal setup() interface for package distribution.

Provides setup() for backward compatibility with legacy packages.
In modern Python, setuptools.setup() is preferred.
"""

import sys
import os

class Distribution:
    """A Distribution describes how to build/install a Python project."""
    def __init__(self, attrs=None):
        self.attrs = attrs or {}
        self.metadata = DistributionMetadata(attrs)
        for key, val in self.attrs.items():
            if not hasattr(self, key):
                setattr(self, key, val)
        self.packages = self.attrs.get('packages', [])
        self.py_modules = self.attrs.get('py_modules', [])
        self.ext_modules = self.attrs.get('ext_modules', [])
        self.scripts = self.attrs.get('scripts', [])
        self.data_files = self.attrs.get('data_files', [])
        self.package_dir = self.attrs.get('package_dir', {})
        self.package_data = self.attrs.get('package_data', {})
        self.install_requires = self.attrs.get('install_requires', [])
        self.extras_require = self.attrs.get('extras_require', {})
        self.entry_points = self.attrs.get('entry_points', {})

    def get_name(self):
        return self.metadata.name or 'UNKNOWN'

    def get_version(self):
        return self.metadata.version or '0.0.0'

class DistributionMetadata:
    """Metadata for a Distribution."""
    def __init__(self, attrs=None):
        attrs = attrs or {}
        self.name = attrs.get('name', 'UNKNOWN')
        self.version = attrs.get('version', '0.0.0')
        self.author = attrs.get('author', '')
        self.author_email = attrs.get('author_email', '')
        self.url = attrs.get('url', '')
        self.license = attrs.get('license', '')
        self.description = attrs.get('description', '')
        self.long_description = attrs.get('long_description', '')
        self.classifiers = attrs.get('classifiers', [])
        self.keywords = attrs.get('keywords', [])
        self.platforms = attrs.get('platforms', [])
        self.python_requires = attrs.get('python_requires', '')

class Extension:
    """Describes a C/C++ extension module."""
    def __init__(self, name, sources, **kwargs):
        self.name = name
        self.sources = sources
        self.include_dirs = kwargs.get('include_dirs', [])
        self.library_dirs = kwargs.get('library_dirs', [])
        self.libraries = kwargs.get('libraries', [])
        self.define_macros = kwargs.get('define_macros', [])
        self.extra_compile_args = kwargs.get('extra_compile_args', [])
        self.extra_link_args = kwargs.get('extra_link_args', [])

_setup_distribution = None

def setup(**attrs):
    """The main setup function for configuring and installing packages."""
    global _setup_distribution
    dist = Distribution(attrs)
    _setup_distribution = dist
    return dist

def run_setup(script_name, script_args=None, stop_after='run'):
    """Run a setup script in a controlled environment."""
    global _setup_distribution
    _setup_distribution = None
    save_argv = sys.argv[:]
    try:
        sys.argv = [script_name] + (script_args or [])
        with open(script_name) as f:
            exec(compile(f.read(), script_name, 'exec'))
    finally:
        sys.argv = save_argv
    return _setup_distribution
