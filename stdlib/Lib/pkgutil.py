"""Utilities for packages and modules.

This module provides utilities for working with packages and modules.
"""

import os
import sys

__all__ = ['iter_modules', 'get_data', 'extend_path', 'walk_packages', 'find_loader']


def _get_path(package_or_name):
    """Get the __path__ for a package."""
    if isinstance(package_or_name, str):
        try:
            __import__(package_or_name)
            module = sys.modules[package_or_name]
        except ImportError:
            raise ValueError(f"Cannot find package {package_or_name!r}")
    else:
        module = package_or_name
    
    if not hasattr(module, '__path__'):
        raise ValueError(f"{module!r} is not a package")
    
    return module.__path__


def iter_modules(path=None, prefix=''):
    """Iterate over all modules in a package.
    
    Args:
        path: List of filesystem paths to search. Defaults to sys.path.
        prefix: Optional string to prepend to module names.
    
    Yields:
        Tuples of (module_finder, name, is_pkg) for each module found.
    """
    if path is None:
        path = sys.path
    
    for item_path in path:
        if not os.path.isdir(item_path):
            continue
        
        try:
            entries = os.listdir(item_path)
        except (OSError, PermissionError):
            continue
        
        for entry in sorted(entries):
            if entry.startswith('_'):
                continue
            
            full_path = os.path.join(item_path, entry)
            
            # Check for package (directory with __init__.py)
            if os.path.isdir(full_path):
                init_path = os.path.join(full_path, '__init__.py')
                if os.path.isfile(init_path):
                    yield (None, prefix + entry, True)
            # Check for module (.py file)
            elif entry.endswith('.py') and entry != '__init__.py':
                module_name = entry[:-3]
                yield (None, prefix + module_name, False)


def get_data(package_or_name, resource):
    """Get data from a package resource.
    
    Args:
        package_or_name: Package name string or module object.
        resource: Relative path to resource within the package.
    
    Returns:
        The data as bytes.
    
    Raises:
        OSError: If the resource cannot be found.
    """
    if isinstance(package_or_name, str):
        try:
            __import__(package_or_name)
            module = sys.modules[package_or_name]
        except ImportError:
            raise ValueError(f"Cannot find package {package_or_name!r}")
    else:
        module = package_or_name
    
    if hasattr(module, '__path__'):
        # It's a package
        base_path = module.__path__[0]
    elif hasattr(module, '__file__'):
        # It's a module
        base_path = os.path.dirname(module.__file__)
    else:
        raise ValueError(f"Cannot determine path for {module!r}")
    
    resource_path = os.path.join(base_path, resource)
    
    try:
        with open(resource_path, 'rb') as f:
            return f.read()
    except FileNotFoundError:
        raise OSError(f"Resource {resource!r} not found in {package_or_name!r}")


def extend_path(path, name):
    """Extend a package's __path__ attribute.
    
    This is typically used in __init__.py to allow multiple namespace packages.
    
    Args:
        path: The current __path__ list.
        name: The package name.
    
    Returns:
        The extended path list.
    """
    if not isinstance(path, list):
        path = list(path)
    
    # Look for additional paths in sys.path
    pkg_name = name.split('.')[-1]
    
    for entry in sys.path:
        if not os.path.isdir(entry):
            continue
        
        candidate = os.path.join(entry, pkg_name)
        if os.path.isdir(candidate) and candidate not in path:
            path.append(candidate)
    
    return path


def walk_packages(path=None, prefix='', onerror=None):
    """Walk packages and modules recursively.
    
    Args:
        path: List of filesystem paths to search.
        prefix: Optional string to prepend to module names.
        onerror: Optional error handler function.
    
    Yields:
        Tuples of (module_finder, name, is_pkg) for each module found.
    """
    if path is None:
        path = sys.path
    
    for finder, name, is_pkg in iter_modules(path, prefix):
        yield (finder, name, is_pkg)
        
        if is_pkg:
            try:
                __import__(name)
                module = sys.modules[name]
                if hasattr(module, '__path__'):
                    for item in walk_packages(module.__path__, name + '.', onerror):
                        yield item
            except Exception as e:
                if onerror:
                    onerror(str(e))


def find_loader(fullname):
    """Find a module loader.
    
    Args:
        fullname: Full name of the module.
    
    Returns:
        A tuple of (loader, portions) or (None, portions).
    
    Note:
        This is a stub implementation that returns None as loaders are
        typically provided by the import system.
    """
    try:
        __import__(fullname)
        module = sys.modules.get(fullname)
        if module:
            return (None, [fullname])
    except ImportError:
        pass
    
    return (None, [])
