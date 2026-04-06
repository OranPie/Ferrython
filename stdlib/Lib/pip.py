"""
pip — Package installer compatibility module for Ferrython.

This module delegates to ferryip, the built-in Ferrython package manager.
Supports: `ferrython -m pip install`, `ferrython -m pip list`, etc.
"""

import sys
import os


__version__ = "24.0"


def main(args=None):
    """Entry point for `ferrython -m pip`."""
    if args is None:
        args = sys.argv[1:]

    # Find ferryip binary
    exe_dir = os.path.dirname(sys.executable) if sys.executable else ''
    ferryip = os.path.join(exe_dir, 'ferryip') if exe_dir else 'ferryip'

    if not os.path.exists(ferryip):
        # Try alongside the python binary
        ferryip = 'ferryip'

    try:
        import subprocess
        result = subprocess.run([ferryip] + list(args))
        sys.exit(result.returncode)
    except Exception as e:
        print("Error: could not run ferryip:", e, file=sys.stderr)
        print("Install ferryip with: cargo build -p ferrython-pip", file=sys.stderr)
        sys.exit(1)


if __name__ == '__main__':
    main()
