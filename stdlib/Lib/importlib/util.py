"""importlib.util — Utility code for importers."""

import sys


class _LazyModule:
    """Module proxy that loads the actual module on first attribute access."""
    def __init__(self, name):
        self.__name__ = name
        self.__loader__ = None
        self.__spec__ = None
        self._loaded = False

    def __getattr__(self, attr):
        if not self._loaded:
            self._loaded = True
            __import__(self.__name__)
            mod = sys.modules[self.__name__]
            self.__dict__.update(mod.__dict__)
            return getattr(mod, attr)
        raise AttributeError(f"module {self.__name__!r} has no attribute {attr!r}")


class ModuleSpec:
    """The specification for a module."""
    def __init__(self, name, loader, *, origin=None, loader_state=None,
                 is_package=None):
        self.name = name
        self.loader = loader
        self.origin = origin
        self.loader_state = loader_state
        self.submodule_search_locations = [] if is_package else None
        self._set_fileattr = False
        self._cached = None
        self.parent = name.rpartition('.')[0] if '.' in name else ''
    
    @property
    def cached(self):
        return self._cached
    
    @cached.setter
    def cached(self, value):
        self._cached = value
    
    def __repr__(self):
        args = [f"name={self.name!r}", f"loader={self.loader!r}"]
        if self.origin:
            args.append(f"origin={self.origin!r}")
        return f"ModuleSpec({', '.join(args)})"


def module_from_spec(spec):
    """Create a new module based on the provided spec."""
    import types
    # For built-in modules or when loader is None, import and return the module directly
    if spec.origin == 'built-in' or spec.loader is None:
        try:
            __import__(spec.name)
            mod = sys.modules.get(spec.name)
            if mod is not None:
                mod.__spec__ = spec
                # Provide a no-op loader so spec.loader.exec_module() works
                class _NoopLoader:
                    def exec_module(self, module):
                        pass
                spec.loader = _NoopLoader()
                return mod
        except ImportError:
            pass
    module = types.ModuleType(spec.name)
    module.__spec__ = spec
    module.__loader__ = spec.loader
    if spec.origin:
        module.__file__ = spec.origin
    if spec.submodule_search_locations is not None:
        module.__path__ = spec.submodule_search_locations
    module.__package__ = spec.parent
    return module


def spec_from_file_location(name, location=None, *, loader=None,
                             submodule_search_locations=None):
    """Return a module spec based on a file location."""
    spec = ModuleSpec(name, loader, origin=location)
    if submodule_search_locations is not None:
        spec.submodule_search_locations = submodule_search_locations
    spec._set_fileattr = True
    return spec


def spec_from_loader(name, loader, *, origin=None, is_package=None):
    """Return a module spec based on a loader.

    If is_package is not set, the loader's is_package() method is used
    (when available).
    """
    if is_package is None:
        if hasattr(loader, 'is_package'):
            try:
                is_package = loader.is_package(name)
            except ImportError:
                is_package = None
    spec = ModuleSpec(name, loader, origin=origin, is_package=is_package)
    return spec


def find_spec(name, package=None):
    """Find the spec for a module, optionally relative to a package."""
    if name.startswith('.'):
        if package is None:
            raise ValueError("relative import requires package")
        dots = 0
        for ch in name:
            if ch == '.':
                dots += 1
            else:
                break
        name = name[dots:]
        if name:
            name = package + '.' + name
        else:
            name = package
    
    if name in sys.modules:
        mod = sys.modules[name]
        if hasattr(mod, '__spec__'):
            return mod.__spec__
        origin = getattr(mod, '__file__', None)
        return ModuleSpec(name, None, origin=origin)
    
    # Try to find the module on the filesystem or as a builtin
    import os
    rel_path = name.replace('.', os.sep)
    search_paths = list(getattr(sys, 'path', []))
    if '.' not in search_paths:
        search_paths.insert(0, '.')
    for base in search_paths:
        file_path = os.path.join(base, rel_path + '.py')
        if os.path.exists(file_path):
            return ModuleSpec(name, None, origin=file_path)
        init_path = os.path.join(base, rel_path, '__init__.py')
        if os.path.exists(init_path):
            return ModuleSpec(name, None, origin=init_path, is_package=True)
    
    # Try importing it (catches builtins and other non-file modules)
    try:
        __import__(name)
        if name in sys.modules:
            mod = sys.modules[name]
            origin = getattr(mod, '__file__', 'built-in')
            return ModuleSpec(name, None, origin=origin)
    except ImportError:
        pass
    
    return None


def resolve_name(name, package):
    """Resolve a relative module name to an absolute one."""
    if not name.startswith('.'):
        return name
    dots = 0
    for ch in name:
        if ch == '.':
            dots += 1
        else:
            break
    if not package:
        raise ImportError("attempted relative import with no known parent package")
    bits = package.rsplit('.', dots - 1)
    if len(bits) < dots:
        raise ImportError("attempted relative import beyond top-level package")
    base = bits[0]
    rest = name[dots:]
    if rest:
        return base + '.' + rest
    return base


def source_hash(source_bytes):
    """Return the hash of source_bytes as bytes."""
    import hashlib
    return hashlib.sha256(source_bytes).digest()[:8]


def decode_source(source_bytes):
    """Decode bytes representing source code."""
    if isinstance(source_bytes, str):
        return source_bytes
    return source_bytes.decode('utf-8')


class LazyLoader:
    """A loader that defers module loading until first attribute access."""
    def __init__(self, loader):
        self.loader = loader
    
    @classmethod
    def factory(cls, loader):
        return lambda: cls(loader)
    
    def create_module(self, spec):
        return self.loader.create_module(spec)
    
    def exec_module(self, module):
        pass
