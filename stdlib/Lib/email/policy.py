"""email.policy — Policy objects for email handling."""


class Policy:
    """Base policy for email handling."""
    
    def __init__(self, **kw):
        self.max_line_length = kw.get('max_line_length', 78)
        self.utf8 = kw.get('utf8', False)
        self.raise_on_defect = kw.get('raise_on_defect', False)
        self.cte_type = kw.get('cte_type', '8bit')
        for key, value in kw.items():
            if not hasattr(self, key):
                setattr(self, key, value)
    
    def clone(self, **kw):
        newpolicy = self.__class__(**{**self.__dict__, **kw})
        return newpolicy
    
    def handle_defect(self, obj, defect):
        if self.raise_on_defect:
            raise defect
    
    def register_defect(self, obj, defect):
        if hasattr(obj, 'defects'):
            obj.defects.append(defect)
    
    def header_max_count(self, name):
        return None
    
    def header_source_parse(self, sourcelines):
        name, value = sourcelines[0].split(':', 1)
        return (name, value.strip())
    
    def header_store_parse(self, name, value):
        return (name, value)
    
    def header_fetch_parse(self, name, value):
        return value
    
    def fold(self, name, value):
        return f"{name}: {value}\n"
    
    def fold_binary(self, name, value):
        return self.fold(name, value).encode('ascii', 'surrogateescape')


class EmailPolicy(Policy):
    """Policy for modern email messages (RFC 6531+)."""
    
    def __init__(self, **kw):
        super().__init__(**kw)
        self.utf8 = kw.get('utf8', False)
    
    def header_source_parse(self, sourcelines):
        name, value = sourcelines[0].split(':', 1)
        return (name, value.lstrip())
    
    def header_store_parse(self, name, value):
        return (name, value)


class Compat32(Policy):
    """Policy compatible with Python 3.2 email package behavior."""
    pass


# Default policy instances
compat32 = Compat32()
default = EmailPolicy()
SMTP = EmailPolicy(cte_type='7bit')
SMTPUTF8 = EmailPolicy(cte_type='7bit', utf8=True)
HTTP = EmailPolicy(max_line_length=0)
strict = EmailPolicy(raise_on_defect=True)
