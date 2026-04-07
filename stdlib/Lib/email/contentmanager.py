"""email.contentmanager — Content manager for email messages."""


class ContentManager:
    """Manage content type handlers for email messages."""
    
    def __init__(self):
        self._handlers = {}
    
    def add_get_handler(self, key, handler):
        self._handlers[('get', key)] = handler
    
    def add_set_handler(self, typekey, handler):
        self._handlers[('set', typekey)] = handler
    
    def get_content(self, msg, *args, **kw):
        content_type = msg.get_content_type() if hasattr(msg, 'get_content_type') else 'text/plain'
        handler = self._handlers.get(('get', content_type))
        if handler is None:
            maintype = content_type.split('/')[0]
            handler = self._handlers.get(('get', maintype))
        if handler is None:
            handler = self._handlers.get(('get', ''))
        if handler is None:
            raise KeyError(content_type)
        return handler(msg, *args, **kw)
    
    def set_content(self, msg, obj, *args, **kw):
        handler = None
        typ = type(obj)
        for key in (typ.__name__, typ):
            handler = self._handlers.get(('set', key))
            if handler is not None:
                break
        if handler is None:
            handler = self._handlers.get(('set', ''))
        if handler is None:
            raise TypeError(f"no handler for type {typ!r}")
        handler(msg, obj, *args, **kw)


raw_data_manager = ContentManager()
