"""Thread-local data implementation.

Provides a local class that manages thread-local storage.
This is the pure Python fallback for the C implementation.
"""

import threading


class local:
    """Thread-local data.
    
    Each thread sees its own instance data.
    
    Usage:
        mydata = local()
        mydata.x = 1   # visible only in this thread
    """
    
    def __init__(self, **kw):
        self.__dict__['_local_data'] = {}
        self.__dict__['_local_data'].update(kw)
    
    def __getattr__(self, name):
        data = self.__dict__.get('_local_data', {})
        if name in data:
            return data[name]
        raise AttributeError(f"'{type(self).__name__}' object has no attribute '{name}'")
    
    def __setattr__(self, name, value):
        if name == '_local_data':
            self.__dict__['_local_data'] = value
        else:
            data = self.__dict__.get('_local_data', {})
            data[name] = value
    
    def __delattr__(self, name):
        data = self.__dict__.get('_local_data', {})
        if name not in data:
            raise AttributeError(name)
        del data[name]
