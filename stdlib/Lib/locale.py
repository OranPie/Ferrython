"""Pure Python implementation of the locale module.

Interface to the locale services.
"""

import os


# Locale categories
LC_CTYPE = 0
LC_NUMERIC = 1
LC_TIME = 2
LC_COLLATE = 3
LC_MONETARY = 4
LC_MESSAGES = 5
LC_ALL = 6

# Conversion characters
CHAR_MAX = 127


_current_locale = 'C'


def setlocale(category, locale=None):
    """Set the locale for the given category."""
    global _current_locale
    if locale is None:
        return _current_locale
    if locale == '' or locale == 'C' or locale == 'POSIX':
        _current_locale = 'C'
    else:
        _current_locale = locale
    return _current_locale


def getlocale(category=LC_CTYPE):
    """Return current locale for the given category as (language, encoding)."""
    if _current_locale in ('C', 'POSIX'):
        return (None, None)
    parts = _current_locale.split('.')
    lang = parts[0] if parts else None
    enc = parts[1] if len(parts) > 1 else None
    return (lang, enc)


def getdefaultlocale(envvars=('LC_ALL', 'LC_CTYPE', 'LANG', 'LANGUAGE')):
    """Return (language, encoding) for the default locale."""
    for var in envvars:
        val = os.environ.get(var)
        if val:
            parts = val.split('.')
            lang = parts[0]
            enc = parts[1] if len(parts) > 1 else 'UTF-8'
            return (lang, enc)
    return ('en_US', 'UTF-8')


def getpreferredencoding(do_setlocale=True):
    """Return the user's preferred encoding."""
    return 'UTF-8'


def localeconv():
    """Return a dictionary of locale conventions."""
    return {
        'decimal_point': '.',
        'grouping': [],
        'thousands_sep': '',
        'currency_symbol': '',
        'int_curr_symbol': '',
        'mon_decimal_point': '',
        'mon_thousands_sep': '',
        'mon_grouping': [],
        'positive_sign': '',
        'negative_sign': '-',
        'int_frac_digits': CHAR_MAX,
        'frac_digits': CHAR_MAX,
        'p_cs_precedes': CHAR_MAX,
        'p_sep_by_space': CHAR_MAX,
        'n_cs_precedes': CHAR_MAX,
        'n_sep_by_space': CHAR_MAX,
        'p_sign_posn': CHAR_MAX,
        'n_sign_posn': CHAR_MAX,
    }


def normalize(localename):
    """Normalize locale name."""
    if not localename:
        return localename
    return localename.replace('-', '_')


def resetlocale(category=LC_ALL):
    """Reset locale to default."""
    setlocale(category, '')


def format_string(fmt, val, grouping=False, monetary=False):
    """Format a string using locale-aware formatting."""
    if isinstance(val, tuple):
        return fmt % val
    return fmt % (val,)


def currency(val, symbol=True, grouping=False, international=False):
    """Format a number as currency."""
    return '%.2f' % val


def str(val):
    """Format a number with locale decimal point."""
    return '%g' % val


def atof(string, func=float):
    """Parse a string to a float using locale settings."""
    return func(string)


def atoi(string):
    """Parse a string to an integer using locale settings."""
    return int(string)


def getencoding():
    """Get the current locale encoding."""
    return 'UTF-8'
