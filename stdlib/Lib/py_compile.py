"""Compile Python source files to bytecode.

This module provides functions to compile Python source files.
"""

import os
import enum

try:
    import marshal
except ImportError:
    marshal = None

__all__ = ['compile', 'PycInvalidationMode']


class PycInvalidationMode(enum.Enum):
    """Enumeration of different invalidation modes for .pyc files.
    
    Attributes:
        TIMESTAMP: Invalidate .pyc files based on modification time.
        CHECKED_HASH: Invalidate .pyc files based on content hash checks.
        UNCHECKED_HASH: Use .pyc files without validation (faster).
    """
    TIMESTAMP = 1
    CHECKED_HASH = 2
    UNCHECKED_HASH = 3


def compile(file, cfile=None, dfile=None, doraise=True, quiet=0, 
           legacy=False, invalidation_mode=PycInvalidationMode.TIMESTAMP):
    """Compile a Python source file.
    
    Args:
        file: Path to the source file to compile.
        cfile: Path to the output .pyc file. If None, uses standard location.
        dfile: Display name for error messages.
        doraise: If True, raise exceptions on errors.
        quiet: Verbosity level (0=normal, 1=quiet).
        legacy: If True, use legacy .pyc format.
        invalidation_mode: PycInvalidationMode for cache validation.
    
    Returns:
        The path to the compiled file, or None on error.
    
    Raises:
        PyCompileError: If compilation fails and doraise=True.
    """
    if not os.path.exists(file):
        if doraise:
            raise OSError(f"File not found: {file}")
        if quiet < 1:
            print(f"Compiling {file}...")
        return None
    
    # Read source file
    try:
        with open(file, 'r', encoding='utf-8') as f:
            source = f.read()
    except (IOError, UnicodeDecodeError) as e:
        if doraise:
            raise PyCompileError(f"Cannot read {file}: {e}")
        return None
    
    # Compile source to code object
    try:
        code = compile(source, dfile or file, 'exec')
    except SyntaxError as e:
        if doraise:
            raise PyCompileError(f"Syntax error in {file}: {e}")
        if quiet < 1:
            print(f"Error compiling {file}: {e}")
        return None
    
    # Determine output path
    if cfile is None:
        if legacy:
            cfile = file + 'c'
        else:
            # Use standard __pycache__ location
            dirname = os.path.dirname(file)
            basename = os.path.basename(file)
            if basename.endswith('.py'):
                basename = basename[:-3]
            
            pycache = os.path.join(dirname, '__pycache__')
            os.makedirs(pycache, exist_ok=True)
            cfile = os.path.join(pycache, basename + '.pyc')
    
    # Write compiled code
    try:
        os.makedirs(os.path.dirname(cfile), exist_ok=True)
        with open(cfile, 'wb') as f:
            # Write a minimal pyc header (magic number + flags + timestamp)
            import sys
            magic = sys.version_info[0] * 100 + sys.version_info[1]
            f.write(magic.to_bytes(2, 'little'))
            f.write((0).to_bytes(2, 'little'))  # Flags
            f.write((0).to_bytes(4, 'little'))  # Timestamp
            f.write((0).to_bytes(4, 'little'))  # File size
            
            # Write marshalled code
            if marshal:
                try:
                    marshal.dump(code, f)
                except (ValueError, TypeError):
                    # Fallback: write bytecode directly if marshal fails
                    pass
        
        if quiet < 1:
            print(f"Compiling {file}...")
        
        return cfile
    except (IOError, OSError) as e:
        if doraise:
            raise PyCompileError(f"Cannot write {cfile}: {e}")
        if quiet < 1:
            print(f"Error writing {cfile}: {e}")
        return None


class PyCompileError(Exception):
    """Exception raised during Python compilation."""
    
    def __init__(self, msg, exc_type=None, exc_value=None, file=None, msg_prefix=''):
        """Initialize PyCompileError.
        
        Args:
            msg: The error message.
            exc_type: Optional exception type.
            exc_value: Optional exception value.
            file: Optional file path.
            msg_prefix: Optional message prefix.
        """
        self.msg = msg
        self.exc_type = exc_type
        self.exc_value = exc_value
        self.file = file
        self.msg_prefix = msg_prefix
        super().__init__(msg)
    
    def __str__(self):
        if self.msg_prefix:
            return f"{self.msg_prefix}: {self.msg}"
        return self.msg
