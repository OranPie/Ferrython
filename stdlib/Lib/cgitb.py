"""CGI Traceback Module.

This module provides tools for debugging CGI scripts by generating
formatted tracebacks and enabling traceback hooks.
"""

import sys
import os
import traceback as tb_module
import linecache
import inspect
import html as html_module

__all__ = ['enable', 'handler', 'text', 'html', 'Hook', 'EarlyExitWarning']


class EarlyExitWarning(UserWarning):
    """Warning for early exit in CGI debugging."""
    pass


def enable():
    """Enable automatic CGI traceback handling.
    
    This installs a hook in sys.excepthook to display formatted
    tracebacks when unhandled exceptions occur.
    """
    sys.excepthook = handler


def handler(etype, evalue, etb):
    """Handle exceptions by displaying formatted traceback.
    
    This function is suitable for use as sys.excepthook.
    
    Args:
        etype: The exception type.
        evalue: The exception value.
        etb: The exception traceback.
    """
    try:
        # Check if we're in a CGI environment
        if 'REQUEST_METHOD' in os.environ:
            # We're in CGI, output as HTML
            sys.stdout.write('Content-Type: text/html\n\n')
            sys.stdout.write(html(etype, evalue, etb))
        else:
            # Output as plain text
            sys.stdout.write(text(etype, evalue, etb))
    except Exception:
        # Fall back to default behavior if something fails
        sys.__excepthook__(etype, evalue, etb)


def text(etype, evalue, etb=None):
    """Format traceback as plain text.
    
    Args:
        etype: The exception type.
        evalue: The exception value.
        etb: The exception traceback.
    
    Returns:
        Formatted traceback as a string.
    """
    if etb is None and hasattr(evalue, '__traceback__'):
        etb = evalue.__traceback__
    
    lines = ['Traceback (most recent call last):\n']
    
    # Format traceback frames manually
    if etb:
        tb = etb
        while tb is not None:
            try:
                frame = tb.tb_frame
                lineno = tb.tb_lineno
                filename = frame.f_code.co_filename
                funcname = frame.f_code.co_name
                lines.append('  File "{}", line {}, in {}\n'.format(filename, lineno, funcname))
                
                # Try to get the source line
                line = linecache.getline(filename, lineno, frame.f_globals)
                if line:
                    lines.append('    {}\n'.format(line.strip()))
            except Exception:
                pass
            
            tb = tb.tb_next if tb else None
    
    # Format exception
    lines.append('{}: {}\n'.format(etype.__name__, evalue))
    
    return ''.join(lines)


def html(etype, evalue, etb=None):
    """Format traceback as HTML.
    
    Args:
        etype: The exception type.
        evalue: The exception value.
        etb: The exception traceback.
    
    Returns:
        Formatted traceback as HTML string.
    """
    if etb is None and hasattr(evalue, '__traceback__'):
        etb = evalue.__traceback__
    
    # HTML header
    html_lines = [
        '<!DOCTYPE html>',
        '<html>',
        '<head>',
        '<title>CGI Traceback</title>',
        '<style>',
        'body { font-family: monospace; margin: 20px; }',
        '.traceback { background-color: #f0f0f0; padding: 10px; border: 1px solid #ccc; }',
        '.frame { margin-bottom: 20px; }',
        '.filename { color: #0066cc; font-weight: bold; }',
        '.lineno { color: #666666; }',
        '.function { color: #cc0000; }',
        '.source { margin-left: 20px; background-color: #ffffff; padding: 5px; }',
        '.exception { color: #cc0000; font-weight: bold; margin-top: 20px; }',
        '</style>',
        '</head>',
        '<body>',
        '<h1>CGI Traceback</h1>',
        '<div class="traceback">',
    ]
    
    # Format traceback frames manually
    if etb:
        tb = etb
        while tb is not None:
            try:
                frame = tb.tb_frame
                lineno = tb.tb_lineno
                filename = frame.f_code.co_filename
                funcname = frame.f_code.co_name
                
                html_lines.append('<div class="frame">')
                html_lines.append('<span class="filename">File "{}"</span>, <span class="lineno">line {}</span>, <span class="function">in {}</span>'.format(
                    html_module.escape(filename), lineno, html_module.escape(funcname)))
                
                # Try to get the source line
                line = linecache.getline(filename, lineno, frame.f_globals)
                if line:
                    html_lines.append('<div class="source">{}</div>'.format(html_module.escape(line.rstrip())))
                
                html_lines.append('</div>')
            except Exception:
                pass
            
            tb = tb.tb_next if tb else None
    
    # Format exception
    exception_name = html_module.escape(etype.__name__)
    exception_value = html_module.escape(str(evalue))
    html_lines.append('<div class="exception">{}: {}</div>'.format(exception_name, exception_value))
    
    html_lines.extend([
        '</div>',
        '</body>',
        '</html>',
    ])
    
    return '\n'.join(html_lines)


class Hook:
    """Class-based hook for installing cgitb traceback handler.
    
    This allows more control over exception handling behavior.
    """
    
    def __init__(self, display=1, logdir=None, context=5, file=None, format='html'):
        """Initialize Hook.
        
        Args:
            display: Whether to display tracebacks (1=yes, 0=no).
            logdir: Optional directory to log tracebacks.
            context: Number of context lines to display.
            file: Optional file object for output.
            format: Format for traceback ('html' or 'text').
        """
        self.display = display
        self.logdir = logdir
        self.context = context
        self.file = file or sys.stdout
        self.format = format
    
    def handle(self, info=None):
        """Handle an exception.
        
        Args:
            info: Optional exception info tuple (type, value, traceback).
        """
        if info is None:
            info = sys.exc_info()
        
        etype, evalue, etb = info
        
        if self.format == 'html':
            output = html(etype, evalue, etb)
        else:
            output = text(etype, evalue, etb)
        
        if self.display:
            self.file.write(output)
            self.file.write('\n')
        
        if self.logdir:
            self._log_traceback(output)
    
    def __call__(self, etype, evalue, etb):
        """Make Hook callable for use as sys.excepthook."""
        self.handle((etype, evalue, etb))
    
    def _log_traceback(self, output):
        """Log traceback to file (stub implementation)."""
        if not os.path.isdir(self.logdir):
            try:
                os.makedirs(self.logdir)
            except OSError:
                return
        
        # Would write to a file in logdir, but requires more context
        # This is a stub for now
        pass
