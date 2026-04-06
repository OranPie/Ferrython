"""Internationalization and localization support.

Provides GNU gettext-compatible interface for message translation.
"""

import os

__all__ = [
    'NullTranslations', 'GNUTranslations', 'Catalog',
    'gettext', 'ngettext', 'pgettext', 'npgettext',
    'dgettext', 'dngettext', 'dpgettext', 'dnpgettext',
    'textdomain', 'bindtextdomain', 'bind_textdomain_codeset',
    'translation', 'install',
]

# Global state
_current_domain = 'messages'
_localedirs = {}
_translations = {}
_codeset = {}

ENOENT = 2


class NullTranslations:
    """Base translation class that returns messages unchanged."""

    def __init__(self, fp=None):
        self._info = {}
        self._catalog = {}
        self._charset = None
        self._fallback = None
        if fp is not None:
            self._parse(fp)

    def _parse(self, fp):
        pass

    def add_fallback(self, fallback):
        if self._fallback:
            self._fallback.add_fallback(fallback)
        else:
            self._fallback = fallback

    def gettext(self, message):
        if self._fallback:
            return self._fallback.gettext(message)
        return message

    def ngettext(self, singular, plural, n):
        if self._fallback:
            return self._fallback.ngettext(singular, plural, n)
        if n == 1:
            return singular
        return plural

    def pgettext(self, context, message):
        if self._fallback:
            return self._fallback.pgettext(context, message)
        return message

    def npgettext(self, context, singular, plural, n):
        if self._fallback:
            return self._fallback.npgettext(context, singular, plural, n)
        if n == 1:
            return singular
        return plural

    def lgettext(self, message):
        return self.gettext(message)

    def lngettext(self, singular, plural, n):
        return self.ngettext(singular, plural, n)

    def info(self):
        return self._info

    def charset(self):
        return self._charset

    def install(self, names=None):
        import builtins
        builtins.__dict__['_'] = self.gettext
        if names:
            for name in names:
                if name == 'gettext':
                    builtins.__dict__['gettext'] = builtins.__dict__['_']
                elif name == 'ngettext':
                    builtins.__dict__['ngettext'] = self.ngettext
                elif name == 'pgettext':
                    builtins.__dict__['pgettext'] = self.pgettext
                elif name == 'npgettext':
                    builtins.__dict__['npgettext'] = self.npgettext


class GNUTranslations(NullTranslations):
    """GNU .mo file reader with full plural support."""

    LE_MAGIC = 0x950412de
    BE_MAGIC = 0xde120495

    VERSIONS = {0, 1}

    def _parse(self, fp):
        """Parse a .mo file and populate _catalog."""
        import struct
        buf = fp.read()
        buflen = len(buf)

        # Read magic number
        magic = struct.unpack('<I', buf[:4])[0]
        if magic == self.LE_MAGIC:
            ii = '<II'
            endian = '<'
        elif magic == self.BE_MAGIC:
            ii = '>II'
            endian = '>'
        else:
            raise OSError(0, 'Bad magic number', getattr(fp, 'name', ''))

        version, nstrings = struct.unpack(ii, buf[4:12])
        major_version = version >> 16
        if major_version not in self.VERSIONS:
            raise OSError(0, 'Bad version number ' + str(major_version),
                         getattr(fp, 'name', ''))

        msg_off = struct.unpack(endian + 'I', buf[12:16])[0]
        trans_off = struct.unpack(endian + 'I', buf[16:20])[0]

        self._catalog = catalog = {}
        self.plural = self._get_default_plural

        for i in range(nstrings):
            # Original string
            mlen = struct.unpack(endian + 'I', buf[msg_off:msg_off + 4])[0]
            moff = struct.unpack(endian + 'I', buf[msg_off + 4:msg_off + 8])[0]
            msg = buf[moff:moff + mlen].decode('utf-8')
            msg_off += 8

            # Translated string
            tlen = struct.unpack(endian + 'I', buf[trans_off:trans_off + 4])[0]
            toff = struct.unpack(endian + 'I', buf[trans_off + 4:trans_off + 8])[0]
            tmsg = buf[toff:toff + tlen].decode('utf-8')
            trans_off += 8

            if '\x00' in msg:
                # Plural forms
                msg_singular, msg_plural = msg.split('\x00', 1)
                tmsg_parts = tmsg.split('\x00')
                catalog[msg_singular] = tmsg_parts
            else:
                catalog[msg] = tmsg

            # Parse metadata from empty key
            if msg == '':
                for item in tmsg.split('\n'):
                    item = item.strip()
                    if not item:
                        continue
                    if ':' in item:
                        key, val = item.split(':', 1)
                        key = key.strip().lower()
                        val = val.strip()
                        self._info[key] = val
                        if key == 'content-type':
                            for param in val.split(';'):
                                param = param.strip()
                                if param.startswith('charset='):
                                    self._charset = param[8:]

    @staticmethod
    def _get_default_plural(n):
        return int(n != 1)

    def gettext(self, message):
        try:
            tmsg = self._catalog[message]
            if isinstance(tmsg, list):
                return tmsg[0]
            return tmsg
        except KeyError:
            if self._fallback:
                return self._fallback.gettext(message)
            return message

    def ngettext(self, singular, plural, n):
        try:
            tmsg = self._catalog.get(singular)
            if tmsg is not None and isinstance(tmsg, list):
                idx = self.plural(n)
                if idx < len(tmsg):
                    return tmsg[idx]
        except (KeyError, IndexError):
            pass
        if self._fallback:
            return self._fallback.ngettext(singular, plural, n)
        if n == 1:
            return singular
        return plural

    def pgettext(self, context, message):
        ctxt_msg = context + '\x04' + message
        try:
            tmsg = self._catalog[ctxt_msg]
            if isinstance(tmsg, list):
                return tmsg[0]
            return tmsg
        except KeyError:
            if self._fallback:
                return self._fallback.pgettext(context, message)
            return message


def _expand_lang(loc):
    """Expand locale into list of candidate language codes."""
    langs = []
    if loc:
        # Normalize the locale code
        loc = loc.replace('-', '_')
        parts = loc.split('.')
        lang = parts[0]
        if lang:
            langs.append(lang)
            if '_' in lang:
                langs.append(lang.split('_')[0])
    return langs


def find(domain, localedir=None, languages=None, all=False):
    """Find .mo file for domain/language."""
    if localedir is None:
        localedir = _localedirs.get(domain, os.path.join('/', 'usr', 'share', 'locale'))

    if languages is None:
        languages = []
        for envvar in ('LANGUAGE', 'LC_ALL', 'LC_MESSAGES', 'LANG'):
            val = os.environ.get(envvar)
            if val:
                languages = val.split(':')
                break
        if not languages:
            languages = ['C']

    result = []
    for lang in languages:
        if lang == 'C':
            continue
        for candidate in _expand_lang(lang):
            mofile = os.path.join(localedir, candidate, 'LC_MESSAGES', domain + '.mo')
            if os.path.isfile(mofile):
                if all:
                    result.append(mofile)
                else:
                    return mofile
    if all:
        return result
    return None


def translation(domain, localedir=None, languages=None, class_=None, fallback=False, codeset=None):
    """Return a translation object for domain."""
    if class_ is None:
        class_ = GNUTranslations

    mofile = find(domain, localedir, languages)
    if mofile is None:
        if fallback:
            return NullTranslations()
        raise FileNotFoundError(ENOENT, 'No translation file found for domain', domain)

    key = (class_, os.path.abspath(mofile))
    if key in _translations:
        return _translations[key]

    with open(mofile, 'rb') as fp:
        t = class_(fp)
    _translations[key] = t
    return t


def install(domain, localedir=None, codeset=None, names=None):
    """Install gettext as the global _() function."""
    t = translation(domain, localedir, fallback=True, codeset=codeset)
    t.install(names)


def textdomain(domain=None):
    """Set or get the current global domain."""
    global _current_domain
    if domain is not None:
        _current_domain = domain
    return _current_domain


def bindtextdomain(domain, localedir=None):
    """Bind domain to a locale directory."""
    if localedir is not None:
        _localedirs[domain] = localedir
    return _localedirs.get(domain, os.path.join('/', 'usr', 'share', 'locale'))


def bind_textdomain_codeset(domain, codeset=None):
    """Set encoding for messages from domain."""
    if codeset is not None:
        _codeset[domain] = codeset
    return _codeset.get(domain)


def dgettext(domain, message):
    """Get translation from specific domain."""
    try:
        t = translation(domain, fallback=True)
        return t.gettext(message)
    except Exception:
        return message


def dngettext(domain, singular, plural, n):
    try:
        t = translation(domain, fallback=True)
        return t.ngettext(singular, plural, n)
    except Exception:
        if n == 1:
            return singular
        return plural


def dpgettext(domain, context, message):
    try:
        t = translation(domain, fallback=True)
        return t.pgettext(context, message)
    except Exception:
        return message


def dnpgettext(domain, context, singular, plural, n):
    try:
        t = translation(domain, fallback=True)
        return t.npgettext(context, singular, plural, n)
    except Exception:
        if n == 1:
            return singular
        return plural


# Module-level convenience functions that use the global domain
def gettext(message):
    return dgettext(_current_domain, message)


def ngettext(singular, plural, n):
    return dngettext(_current_domain, singular, plural, n)


def pgettext(context, message):
    return dpgettext(_current_domain, context, message)


def npgettext(context, singular, plural, n):
    return dnpgettext(_current_domain, context, singular, plural, n)


# Backward compatibility
Catalog = translation

# Convenience alias
_ = gettext
