"""Interfaces for launching and remotely controlling web browsers.

Simplified implementation of the standard library webbrowser module.
"""


class Error(Exception):
    pass


class BaseBrowser:
    """Base class for browser controllers."""

    def __init__(self, name=''):
        self.name = name
        self.basename = name

    def open(self, url, new=0, autoraise=True):
        raise NotImplementedError

    def open_new(self, url):
        return self.open(url, 1)

    def open_new_tab(self, url):
        return self.open(url, 2)


class GenericBrowser(BaseBrowser):
    """Launch a browser via a command line."""

    def __init__(self, name):
        if isinstance(name, str):
            self.name = name
            self.args = [name]
        else:
            self.name = name[0]
            self.args = list(name)
        self.basename = self.name

    def open(self, url, new=0, autoraise=True):
        import os
        cmd = ' '.join(self.args) + ' ' + _escape_url(url)
        try:
            os.system(cmd + ' 2>/dev/null &')
            return True
        except Exception:
            return False


_browsers = {}
_tryorder = []


def _escape_url(url):
    """Escape special characters in URL for shell usage."""
    result = ''
    for ch in url:
        if ch in ('"', "'", '\\', ' ', '(', ')', '&', '|', ';', '$', '`'):
            result += '\\' + ch
        else:
            result += ch
    return result


def register(name, klass=None, instance=None, preferred=False):
    """Register a browser connector."""
    if klass is not None:
        _browsers[name] = klass
    if instance is not None:
        _browsers[name] = instance
    if preferred:
        _tryorder.insert(0, name)
    elif name not in _tryorder:
        _tryorder.append(name)


def get(using=None):
    """Return a browser launcher instance."""
    if using is not None:
        if using in _browsers:
            b = _browsers[using]
            if isinstance(b, type):
                return b(using)
            return b
        raise Error("could not locate runnable browser: {}".format(using))
    for name in _tryorder:
        if name in _browsers:
            b = _browsers[name]
            if isinstance(b, type):
                return b(name)
            return b
    return GenericBrowser('xdg-open')


def open(url, new=0, autoraise=True):
    """Display url using the default browser."""
    import os
    import sys
    platform = sys.platform if hasattr(sys, 'platform') else 'linux'
    try:
        if platform == 'darwin':
            os.system('open ' + _escape_url(url) + ' 2>/dev/null &')
        elif platform == 'win32':
            os.system('start ' + _escape_url(url))
        else:
            os.system('xdg-open ' + _escape_url(url) + ' 2>/dev/null &')
        return True
    except Exception:
        return False


def open_new(url):
    """Open url in a new browser window."""
    return open(url, new=1)


def open_new_tab(url):
    """Open url in a new browser tab."""
    return open(url, new=2)
