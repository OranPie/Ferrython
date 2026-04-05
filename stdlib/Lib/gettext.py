"""Internationalization stub."""

def gettext(message):
    return message

def ngettext(singular, plural, n):
    if n == 1:
        return singular
    return plural

def install(domain=None, localedir=None, names=None):
    import builtins
    builtins.__dict__['_'] = gettext

class NullTranslations:
    def __init__(self, fp=None):
        self._catalog = {}
    
    def gettext(self, message):
        return message
    
    def ngettext(self, singular, plural, n):
        if n == 1:
            return singular
        return plural
    
    def install(self, names=None):
        import builtins
        builtins.__dict__['_'] = self.gettext

class GNUTranslations(NullTranslations):
    pass

def translation(domain, localedir=None, languages=None, fallback=False):
    if fallback:
        return NullTranslations()
    raise FileNotFoundError(f"No translation file for domain '{domain}'")

_ = gettext
