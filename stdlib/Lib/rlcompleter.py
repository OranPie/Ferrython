"""Readline completion helper for Python interactive shell.

This module provides the Completer class for tab-completion in the
Python interactive shell when readline support is available.
"""

import re
import keyword
import sys

__all__ = ['Completer']


class Completer:
    """Completer class for readline-style tab completion.
    
    This class provides word completion for Python identifiers and attributes
    in an interactive shell environment.
    
    Example:
        import rlcompleter
        import readline
        completer = rlcompleter.Completer()
        readline.set_completer(completer.complete)
    """
    
    def __init__(self, namespace=None):
        """Initialize the Completer.
        
        Args:
            namespace: Optional namespace dictionary to use for completions.
                      Defaults to the main module's namespace or globals().
        """
        if namespace is None:
            try:
                import __main__
                namespace = __main__.__dict__
            except (ImportError, AttributeError):
                namespace = {}
        self.namespace = namespace
    
    def complete(self, text, state):
        """Complete a word with readline-style completion.
        
        Args:
            text: The text to complete.
            state: The completion index (0 for first, 1 for second, etc).
        
        Returns:
            The next completion, or None if no more completions exist.
        """
        if state == 0:
            # First call for this text
            if '.' in text:
                self.matches = self._attr_matches(text)
            else:
                self.matches = self._global_matches(text)
        
        # Return the state-th match, or None
        try:
            return self.matches[state]
        except IndexError:
            return None
    
    def _global_matches(self, text):
        """Complete a global name.
        
        Args:
            text: The text to complete.
        
        Returns:
            List of matching global names and keywords.
        """
        matches = []
        
        # Get all names in namespace
        for name in self.namespace:
            if name.startswith(text):
                matches.append(name)
        
        # Add Python keywords
        for word in keyword.kwlist:
            if word.startswith(text):
                matches.append(word)
        
        # Add builtins
        try:
            import builtins
            for name in dir(builtins):
                if name.startswith(text):
                    matches.append(name)
        except (ImportError, AttributeError):
            pass
        
        return sorted(set(matches))
    
    def _attr_matches(self, text):
        """Complete an attribute name.
        
        Args:
            text: The text to complete (should contain a dot).
        
        Returns:
            List of matching attribute names.
        """
        matches = []
        
        # Split on the last dot
        m = re.match(r'(\w+(\.\w+)*)\.(\w*)$', text)
        if not m:
            return matches
        
        expr = m.group(1)
        attr_start = m.group(3)
        
        try:
            # Evaluate the expression to get the object
            obj = eval(expr, self.namespace)
        except Exception:
            return matches
        
        # Get all attributes of the object
        try:
            words = dir(obj)
        except Exception:
            return matches
        
        # Filter by prefix
        prefix = expr + '.'
        for word in words:
            if word.startswith(attr_start):
                matches.append(prefix + word)
        
        return sorted(matches)


# Module-level function for backwards compatibility
_completer = None

def get_completer():
    """Get or create the global completer instance."""
    global _completer
    if _completer is None:
        _completer = Completer()
    return _completer


def parse_and_bind(line):
    """Parse and bind a readline configuration line.
    
    This is a stub implementation for compatibility.
    
    Args:
        line: A readline configuration line.
    
    Note:
        Actual readline binding requires readline module support.
    """
    # This would configure readline, but requires active readline support
    pass


def set_startup_hook(function):
    """Set a function to be called at startup.
    
    This is a stub implementation for compatibility.
    
    Args:
        function: Optional function to call, or None to disable.
    
    Note:
        Actual startup hook requires readline module support.
    """
    # This would set a readline startup hook if readline is available
    try:
        import readline
        readline.set_startup_hook(function)
    except (ImportError, AttributeError):
        pass


def clear_history():
    """Clear readline history.
    
    This is a stub implementation for compatibility.
    
    Note:
        Actual history clearing requires readline module support.
    """
    try:
        import readline
        readline.clear_history()
    except (ImportError, AttributeError):
        pass
