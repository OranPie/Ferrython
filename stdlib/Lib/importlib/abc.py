"""importlib.abc — Abstract base classes for import."""

from abc import ABC, abstractmethod


class Finder(ABC):
    """Legacy abstract base class for import finders."""
    @abstractmethod
    def find_module(self, fullname, path=None):
        return None


class MetaPathFinder(Finder):
    """Abstract base class for import finders on sys.meta_path."""
    def find_module(self, fullname, path=None):
        return None
    
    def find_spec(self, fullname, path, target=None):
        return None
    
    def invalidate_caches(self):
        pass


class PathEntryFinder(Finder):
    """Abstract base class for path entry finders."""
    def find_module(self, fullname):
        return None
    
    def find_spec(self, fullname, target=None):
        return None
    
    def invalidate_caches(self):
        pass


class Loader(ABC):
    """Abstract base class for import loaders."""
    def create_module(self, spec):
        return None
    
    @abstractmethod
    def exec_module(self, module):
        raise ImportError

    def load_module(self, fullname):
        raise ImportError


class ResourceLoader(Loader):
    """Abstract base class for loaders that can return data."""
    @abstractmethod
    def get_data(self, path):
        raise OSError


class InspectLoader(Loader):
    """Abstract base class for loaders that inspect modules."""
    def is_package(self, fullname):
        raise ImportError
    
    def get_code(self, fullname):
        return None
    
    def get_source(self, fullname):
        raise ImportError
    
    def exec_module(self, module):
        pass


class ExecutionLoader(InspectLoader):
    """Abstract base class for loaders that can provide module execution."""
    @abstractmethod
    def get_filename(self, fullname):
        raise ImportError


class FileLoader(ResourceLoader, ExecutionLoader):
    """Abstract base class for file-based module loaders."""
    def __init__(self, fullname, path):
        self.name = fullname
        self.path = path
    
    def get_filename(self, fullname):
        return self.path
    
    def get_data(self, path):
        with open(path, 'rb') as f:
            return f.read()
    
    def exec_module(self, module):
        pass


class SourceLoader(ResourceLoader, ExecutionLoader):
    """Abstract base class for loading source code."""
    def path_mtime(self, path):
        raise OSError
    
    def path_stats(self, path):
        return {'mtime': self.path_mtime(path)}
    
    def set_data(self, path, data):
        pass
    
    def get_filename(self, fullname):
        raise ImportError
    
    def get_data(self, path):
        raise OSError
    
    def exec_module(self, module):
        pass
