"""
pkg_resources — Package resource management for Ferrython.

Provides basic resource access and package metadata APIs
compatible with setuptools' pkg_resources.
"""

import os
import sys

__version__ = "69.0.0"

_working_set = None


class DistInfoDistribution:
    """Represents an installed distribution from .dist-info."""

    def __init__(self, location, metadata_path):
        self.location = location
        self._metadata_path = metadata_path
        self._metadata = {}
        self._load_metadata()

    def _load_metadata(self):
        metadata_file = os.path.join(self._metadata_path, 'METADATA')
        if not os.path.exists(metadata_file):
            return
        try:
            with open(metadata_file, 'r') as f:
                for line in f:
                    line = line.strip()
                    if ':' in line:
                        key, _, value = line.partition(':')
                        self._metadata[key.strip()] = value.strip()
        except (IOError, OSError):
            pass

    @property
    def project_name(self):
        return self._metadata.get('Name', '')

    @property
    def version(self):
        return self._metadata.get('Version', '0.0.0')

    @property
    def key(self):
        return self.project_name.lower().replace('-', '_')

    def __repr__(self):
        return '{}({!r}, {!r})'.format(
            type(self).__name__, self.project_name, self.version
        )


class WorkingSet:
    """A collection of installed distributions."""

    def __init__(self, entries=None):
        self.entries = entries or sys.path[:]
        self._dists = {}
        self._scan()

    def _scan(self):
        """Scan all entries for .dist-info directories."""
        for entry in self.entries:
            if not os.path.isdir(entry):
                continue
            try:
                for name in os.listdir(entry):
                    if name.endswith('.dist-info'):
                        dist_path = os.path.join(entry, name)
                        dist = DistInfoDistribution(entry, dist_path)
                        if dist.key:
                            self._dists[dist.key] = dist
            except OSError:
                pass

    def __iter__(self):
        return iter(self._dists.values())

    def __contains__(self, dist):
        if isinstance(dist, str):
            return dist.lower().replace('-', '_') in self._dists
        return dist in self._dists.values()

    def find(self, req):
        """Find a distribution matching a requirement."""
        if isinstance(req, str):
            name = req.split('>=')[0].split('==')[0].split('<')[0].strip()
            return self._dists.get(name.lower().replace('-', '_'))
        return None


def working_set():
    """Get the global working set."""
    global _working_set
    if _working_set is None:
        _working_set = WorkingSet()
    return _working_set


def require(*requirements):
    """Ensure packages are available (best-effort)."""
    ws = working_set()
    missing = []
    for req in requirements:
        name = req.split('>=')[0].split('==')[0].split('<')[0].strip()
        if name.lower().replace('-', '_') not in ws._dists:
            missing.append(name)
    if missing:
        raise DistributionNotFound(
            "Missing packages: {}".format(', '.join(missing))
        )


def get_distribution(dist_name):
    """Get a specific installed distribution."""
    ws = working_set()
    key = dist_name.lower().replace('-', '_')
    dist = ws._dists.get(key)
    if dist is None:
        raise DistributionNotFound(dist_name)
    return dist


def resource_filename(package_or_requirement, resource_name):
    """Return the filename for a resource."""
    # Simple implementation: resolve relative to the package
    if hasattr(package_or_requirement, '__file__'):
        base = os.path.dirname(package_or_requirement.__file__)
    elif isinstance(package_or_requirement, str):
        base = package_or_requirement.replace('.', os.sep)
    else:
        base = '.'
    return os.path.join(base, resource_name)


def resource_string(package_or_requirement, resource_name):
    """Return the contents of a resource as bytes."""
    filename = resource_filename(package_or_requirement, resource_name)
    with open(filename, 'rb') as f:
        return f.read()


def resource_stream(package_or_requirement, resource_name):
    """Return a file-like object for a resource."""
    filename = resource_filename(package_or_requirement, resource_name)
    return open(filename, 'rb')


def resource_isdir(package_or_requirement, resource_name):
    """Check if a resource is a directory."""
    return os.path.isdir(resource_filename(package_or_requirement, resource_name))


def resource_listdir(package_or_requirement, resource_name):
    """List entries in a resource directory."""
    dirname = resource_filename(package_or_requirement, resource_name)
    if os.path.isdir(dirname):
        return os.listdir(dirname)
    return []


class DistributionNotFound(Exception):
    """Raised when a required distribution is not found."""
    pass


class VersionConflict(Exception):
    """Raised when a version conflict is detected."""
    pass


def iter_entry_points(group, name=None):
    """Iterate over entry points (stub)."""
    return iter([])


def parse_requirements(strs):
    """Parse requirement strings."""
    if isinstance(strs, str):
        strs = strs.splitlines()
    for s in strs:
        s = s.strip()
        if s and not s.startswith('#'):
            yield Requirement(s)


class Requirement:
    """A parsed requirement string."""

    def __init__(self, s):
        self._raw = s
        # Simple parse: name[extras]>=version
        s = s.strip()
        self.extras = ()
        if '[' in s:
            bracket = s.index('[')
            end_bracket = s.index(']')
            self.extras = tuple(
                e.strip() for e in s[bracket+1:end_bracket].split(',')
            )
            s = s[:bracket] + s[end_bracket+1:]

        for op in ('>=', '<=', '!=', '~=', '==', '>', '<'):
            if op in s:
                pos = s.index(op)
                self.project_name = s[:pos].strip()
                self.specs = [(op, s[pos+len(op):].strip())]
                return

        self.project_name = s.strip()
        self.specs = []

    @property
    def key(self):
        return self.project_name.lower().replace('-', '_')

    def __repr__(self):
        return 'Requirement({!r})'.format(self._raw)

    def __str__(self):
        return self._raw
