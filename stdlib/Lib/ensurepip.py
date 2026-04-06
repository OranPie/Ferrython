"""
ensurepip — Bootstrap pip into a Ferrython environment.

In Ferrython, ferryip is the built-in package manager (pip-compatible).
This module provides compatibility stubs.
"""

import os
import sys

_PIP_VERSION = "24.0"

def version():
    """Return the bundled pip version string."""
    return _PIP_VERSION

def bootstrap(*, root=None, upgrade=False, user=False,
              altinstall=False, default_pip=False, verbosity=0):
    """Bootstrap pip (ferryip) into the current environment.

    In Ferrython, ferryip is always available, so this is a no-op.
    """
    print("ferryip is bundled with Ferrython — no bootstrapping needed.")
    return

def _main(args=None):
    """Entry point for `ferrython -m ensurepip`."""
    bootstrap()
