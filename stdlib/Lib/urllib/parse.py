"""URL parsing utilities for Ferrython."""

__all__ = [
    'urlparse', 'urlunparse', 'urljoin', 'urlsplit', 'urlunsplit',
    'urlencode', 'quote', 'unquote', 'parse_qs', 'parse_qsl',
]

# Characters that are never percent-encoded
_ALWAYS_SAFE = frozenset(
    'ABCDEFGHIJKLMNOPQRSTUVWXYZ'
    'abcdefghijklmnopqrstuvwxyz'
    '0123456789'
    '_.-~'
)


class SplitResult:
    """Result of urlsplit()."""
    __slots__ = ('scheme', 'netloc', 'path', 'query', 'fragment')

    def __init__(self, scheme, netloc, path, query, fragment):
        self.scheme = scheme
        self.netloc = netloc
        self.path = path
        self.query = query
        self.fragment = fragment

    def geturl(self):
        return urlunsplit((self.scheme, self.netloc, self.path,
                           self.query, self.fragment))

    def __repr__(self):
        return ("SplitResult(scheme=%r, netloc=%r, path=%r, query=%r, "
                "fragment=%r)" % (self.scheme, self.netloc, self.path,
                                  self.query, self.fragment))

    def __iter__(self):
        yield self.scheme
        yield self.netloc
        yield self.path
        yield self.query
        yield self.fragment

    def __getitem__(self, index):
        return tuple(self)[index]

    def __len__(self):
        return 5

    def __eq__(self, other):
        return tuple(self) == tuple(other)


class ParseResult:
    """Result of urlparse()."""
    __slots__ = ('scheme', 'netloc', 'path', 'params', 'query', 'fragment')

    def __init__(self, scheme, netloc, path, params, query, fragment):
        self.scheme = scheme
        self.netloc = netloc
        self.path = path
        self.params = params
        self.query = query
        self.fragment = fragment

    def geturl(self):
        return urlunparse((self.scheme, self.netloc, self.path,
                           self.params, self.query, self.fragment))

    def __repr__(self):
        return ("ParseResult(scheme=%r, netloc=%r, path=%r, params=%r, "
                "query=%r, fragment=%r)" % (
                    self.scheme, self.netloc, self.path,
                    self.params, self.query, self.fragment))

    def __iter__(self):
        yield self.scheme
        yield self.netloc
        yield self.path
        yield self.params
        yield self.query
        yield self.fragment

    def __getitem__(self, index):
        return tuple(self)[index]

    def __len__(self):
        return 6

    def __eq__(self, other):
        return tuple(self) == tuple(other)


def urlsplit(url, scheme='', allow_fragments=True):
    """Split a URL into 5 components: (scheme, netloc, path, query, fragment)."""
    url = str(url)
    netloc = ''
    query = ''
    fragment = ''

    # Extract scheme
    i = url.find(':')
    if i > 0 and url[:i].isalpha():
        scheme = url[:i].lower()
        url = url[i + 1:]
    elif not scheme:
        scheme = ''

    # Extract fragment
    if allow_fragments and '#' in url:
        idx = url.find('#')
        fragment = url[idx + 1:]
        url = url[:idx]

    # Extract query
    if '?' in url:
        idx = url.find('?')
        query = url[idx + 1:]
        url = url[:idx]

    # Extract netloc
    if url.startswith('//'):
        url = url[2:]
        idx = url.find('/')
        if idx >= 0:
            netloc = url[:idx]
            url = url[idx:]
        else:
            netloc = url
            url = ''

    return SplitResult(scheme, netloc, url, query, fragment)


def urlunsplit(components):
    """Combine 5 URL components into a URL string."""
    scheme, netloc, path, query, fragment = components
    url = ''
    if scheme:
        url = scheme + ':'
    if netloc:
        url = url + '//' + netloc
    if path:
        url = url + path
    if query:
        url = url + '?' + query
    if fragment:
        url = url + '#' + fragment
    return url


def urlparse(url, scheme='', allow_fragments=True):
    """Parse a URL into 6 components: (scheme, netloc, path, params, query, fragment)."""
    sr = urlsplit(url, scheme, allow_fragments)
    path = sr.path
    params = ''
    if ';' in path:
        idx = path.rfind(';')
        params = path[idx + 1:]
        path = path[:idx]
    return ParseResult(sr.scheme, sr.netloc, path, params, sr.query, sr.fragment)


def urlunparse(components):
    """Combine 6 URL components into a URL string."""
    scheme, netloc, path, params, query, fragment = components
    if params:
        path = path + ';' + params
    return urlunsplit((scheme, netloc, path, query, fragment))


def quote(string, safe='/', encoding=None, errors=None):
    """Percent-encode a string."""
    safe_chars = set(safe) | _ALWAYS_SAFE
    result = []
    if isinstance(string, bytes):
        for byte in string:
            c = chr(byte)
            if c in safe_chars:
                result.append(c)
            else:
                result.append('%%%02X' % byte)
    else:
        for char in str(string):
            if char in safe_chars:
                result.append(char)
            else:
                for byte in char.encode('utf-8'):
                    result.append('%%%02X' % byte)
    return ''.join(result)


def unquote(string, encoding='utf-8', errors='replace'):
    """Decode a percent-encoded string."""
    if '%' not in string:
        return string
    result = []
    i = 0
    while i < len(string):
        c = string[i]
        if c == '%' and i + 2 < len(string):
            hex_str = string[i + 1:i + 3]
            try:
                byte_val = int(hex_str, 16)
                result.append(chr(byte_val))
                i += 3
                continue
            except ValueError:
                pass
        result.append(c)
        i += 1
    return ''.join(result)


def urlencode(query, doseq=False, safe='', encoding=None, errors=None,
              quote_via=None):
    """Encode a dict or sequence of two-element tuples into a URL query string."""
    if quote_via is None:
        quote_via = quote
    if hasattr(query, 'items'):
        query = list(query.items())
    parts = []
    for key, value in query:
        k = quote_via(str(key), safe=safe)
        if doseq and isinstance(value, (list, tuple)):
            for v in value:
                parts.append(k + '=' + quote_via(str(v), safe=safe))
        else:
            parts.append(k + '=' + quote_via(str(value), safe=safe))
    return '&'.join(parts)


def parse_qs(qs, keep_blank_values=False, strict_parsing=False):
    """Parse a query string into a dict of lists."""
    result = {}
    pairs = parse_qsl(qs, keep_blank_values, strict_parsing)
    for key, value in pairs:
        if key in result:
            result[key].append(value)
        else:
            result[key] = [value]
    return result


def parse_qsl(qs, keep_blank_values=False, strict_parsing=False):
    """Parse a query string into a list of (key, value) pairs."""
    pairs = []
    if not qs:
        return pairs
    for part in qs.split('&'):
        if not part:
            continue
        if '=' in part:
            key, value = part.split('=', 1)
        else:
            key = part
            value = ''
        key = unquote(key.replace('+', ' '))
        value = unquote(value.replace('+', ' '))
        if value or keep_blank_values:
            pairs.append((key, value))
        elif key:
            pairs.append((key, value))
    return pairs


def urljoin(base, url, allow_fragments=True):
    """Join a base URL and a possibly relative URL to form an absolute URL."""
    if not base:
        return url
    if not url:
        return base

    bscheme, bnetloc, bpath, bparams, bquery, bfragment = urlparse(base)
    scheme, netloc, path, params, query, fragment = urlparse(url)

    # url is absolute
    if scheme and scheme != bscheme:
        return url

    # Use base scheme
    scheme = bscheme

    # url has netloc
    if netloc:
        return urlunparse((scheme, netloc, path, params, query, fragment))

    netloc = bnetloc

    if not path and not params:
        path = bpath
        params = bparams
        if not query:
            query = bquery
        return urlunparse((scheme, netloc, path, params, query, fragment))

    # Resolve relative path
    if path.startswith('/'):
        return urlunparse((scheme, netloc, path, params, query, fragment))

    # Merge paths
    if bpath:
        idx = bpath.rfind('/')
        if idx >= 0:
            path = bpath[:idx + 1] + path
        else:
            path = path
    elif bnetloc:
        path = '/' + path

    # Normalize . and ..
    segments = path.split('/')
    resolved = []
    for seg in segments:
        if seg == '.':
            continue
        elif seg == '..':
            if resolved and resolved[-1] != '':
                resolved.pop()
        else:
            resolved.append(seg)
    path = '/'.join(resolved)
    if not path.startswith('/'):
        path = '/' + path

    return urlunparse((scheme, netloc, path, params, query, fragment))
