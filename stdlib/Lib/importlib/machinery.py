"""importlib.machinery — Importers and path hooks."""

import sys


# File suffixes for different module types
SOURCE_SUFFIXES = ['.py']
BYTECODE_SUFFIXES = ['.pyc']
EXTENSION_SUFFIXES = ['.so', '.pyd']
DEBUG_BYTECODE_SUFFIXES = ['.pyc']
OPTIMIZED_BYTECODE_SUFFIXES = ['.pyc']

all_suffixes = SOURCE_SUFFIXES + BYTECODE_SUFFIXES + EXTENSION_SUFFIXES


class ModuleSpec:
    """The specification for a module, used for module loading."""
    def __init__(self, name, loader, *, origin=None, loader_state=None,
                 is_package=None):
        self.name = name
        self.loader = loader
        self.origin = origin
        self.loader_state = loader_state
        self.submodule_search_locations = [] if is_package else None
        self._cached = None
        self.parent = name.rpartition('.')[0] if '.' in name else ''
    
    def __repr__(self):
        return f"ModuleSpec(name={self.name!r}, loader={self.loader!r})"


class BuiltinImporter:
    """Meta path importer for builtin modules."""
    @classmethod
    def find_module(cls, fullname, path=None):
        if fullname in sys.builtin_module_names:
            return cls
        return None
    
    @classmethod
    def find_spec(cls, fullname, path=None, target=None):
        if fullname in getattr(sys, 'builtin_module_names', ()):
            return ModuleSpec(fullname, cls, origin='built-in')
        return None
    
    @classmethod
    def create_module(cls, spec):
        return None
    
    @classmethod
    def exec_module(cls, module):
        pass
    
    @classmethod
    def load_module(cls, fullname):
        return sys.modules.get(fullname)
    
    @classmethod
    def is_package(cls, fullname):
        return False


class FrozenImporter:
    """Meta path importer for frozen modules."""
    @classmethod
    def find_module(cls, fullname, path=None):
        return None
    
    @classmethod
    def find_spec(cls, fullname, path=None, target=None):
        return None


class PathFinder:
    """Meta path finder for sys.path and package __path__ attributes."""
    @classmethod
    def find_spec(cls, fullname, path=None, target=None):
        return None
    
    @classmethod
    def find_module(cls, fullname, path=None):
        return None
    
    @classmethod
    def invalidate_caches(cls):
        pass


class FileFinder:
    """File-based finder."""
    def __init__(self, path, *loader_details):
        self.path = path
        self._loaders = list(loader_details)
    
    @classmethod
    def path_hook(cls, *loader_details):
        def path_hook_for_FileFinder(path):
            return cls(path, *loader_details)
        return path_hook_for_FileFinder
    
    def find_spec(self, fullname, target=None):
        return None
    
    def find_module(self, fullname):
        return None
    
    def invalidate_caches(self):
        pass


class SourceFileLoader:
    """Loader for source (.py) files."""
    def __init__(self, fullname, path):
        self.name = fullname
        self.path = path
    
    def create_module(self, spec):
        return None
    
    def exec_module(self, module):
        pass
    
    def get_filename(self, fullname=None):
        return self.path
    
    def get_data(self, path):
        with open(path, 'rb') as f:
            return f.read()


class SourcelessFileLoader:
    """Loader for bytecode (.pyc) files without source."""
    def __init__(self, fullname, path):
        self.name = fullname
        self.path = path
    
    def create_module(self, spec):
        return None
    
    def exec_module(self, module):
        pass


class ExtensionFileLoader:
    """Loader for extension modules (.so/.pyd)."""
    def __init__(self, fullname, path):
        self.name = fullname
        self.path = path
    
    def create_module(self, spec):
        return None
    
    def exec_module(self, module):
        pass
