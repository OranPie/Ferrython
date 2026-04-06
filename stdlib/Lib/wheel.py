"""
wheel — Wheel file format support for Ferrython.

Provides utilities for reading, building, and installing Python wheel archives.
This is a compatibility stub — actual wheel installation is handled by ferryip.
"""

import os
import sys
import zipfile

__version__ = "0.43.0"

WHEEL_INFO_RE = r'(?P<namever>(?P<name>.+?)-(?P<ver>\d.*?))(-(?P<build>\d.*?))?-(?P<pyver>.+?)-(?P<abi>.+?)-(?P<plat>.+?)\.whl'


class WheelFile:
    """Represents a .whl (wheel) archive."""

    def __init__(self, path):
        self.path = path
        self.filename = os.path.basename(path)
        self._parse_filename()

    def _parse_filename(self):
        """Parse wheel filename into components."""
        name = self.filename
        if name.endswith('.whl'):
            name = name[:-4]

        parts = name.split('-')
        if len(parts) >= 5:
            self.name = parts[0]
            self.version = parts[1]
            self.python_tag = parts[2] if len(parts) > 2 else 'py3'
            self.abi_tag = parts[3] if len(parts) > 3 else 'none'
            self.platform_tag = parts[4] if len(parts) > 4 else 'any'
        elif len(parts) >= 2:
            self.name = parts[0]
            self.version = parts[1]
            self.python_tag = 'py3'
            self.abi_tag = 'none'
            self.platform_tag = 'any'
        else:
            self.name = name
            self.version = '0.0.0'
            self.python_tag = 'py3'
            self.abi_tag = 'none'
            self.platform_tag = 'any'

    def is_compatible(self):
        """Check if this wheel is compatible with the current platform."""
        # Pure-python wheels are always compatible
        if self.abi_tag == 'none' and self.platform_tag == 'any':
            return True
        # Check Python version tag
        if 'py3' in self.python_tag or 'py2.py3' in self.python_tag:
            return True
        return False

    @property
    def dist_info_name(self):
        return '{}-{}.dist-info'.format(
            self.name.replace('-', '_'),
            self.version
        )

    def namelist(self):
        """List files in the wheel archive."""
        with zipfile.ZipFile(self.path, 'r') as zf:
            return zf.namelist()

    def read(self, name):
        """Read a file from the wheel archive."""
        with zipfile.ZipFile(self.path, 'r') as zf:
            return zf.read(name)

    def extractall(self, path):
        """Extract all files to a directory."""
        with zipfile.ZipFile(self.path, 'r') as zf:
            zf.extractall(path)


def unpack(wheel_path, dest_dir):
    """Unpack a wheel file to a directory."""
    whl = WheelFile(wheel_path)
    whl.extractall(dest_dir)
    return whl
