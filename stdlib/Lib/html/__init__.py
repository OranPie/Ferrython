"""HTML entity handling for Ferrython."""

__all__ = ['escape', 'unescape']


def escape(s, quote=True):
    """Replace special characters &, <, >, " and ' with HTML-safe sequences."""
    s = s.replace("&", "&amp;")
    s = s.replace("<", "&lt;")
    s = s.replace(">", "&gt;")
    if quote:
        s = s.replace('"', "&quot;")
        s = s.replace("'", "&#x27;")
    return s


def unescape(s):
    """Unescape HTML entities in a string."""
    # Named entities first
    s = s.replace("&lt;", "<")
    s = s.replace("&gt;", ">")
    s = s.replace("&quot;", '"')
    s = s.replace("&#39;", "'")
    s = s.replace("&#x27;", "'")
    s = s.replace("&apos;", "'")
    s = s.replace("&amp;", "&")

    # Numeric character references: &#NNN; and &#xHHH;
    result = []
    i = 0
    while i < len(s):
        if s[i] == '&' and i + 2 < len(s) and s[i + 1] == '#':
            j = i + 2
            is_hex = j < len(s) and s[j] in ('x', 'X')
            if is_hex:
                j += 1
            start = j
            while j < len(s) and s[j] != ';':
                j += 1
            if j < len(s) and s[j] == ';':
                num_str = s[start:j]
                try:
                    code = int(num_str, 16) if is_hex else int(num_str)
                    result.append(chr(code))
                    i = j + 1
                    continue
                except (ValueError, OverflowError):
                    pass
        result.append(s[i])
        i += 1
    return ''.join(result)
