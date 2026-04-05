"""A collection of string constants and Template class."""

import re

# Some strings for ctype-style character classification
whitespace = ' \t\n\r\x0b\x0c'
ascii_lowercase = 'abcdefghijklmnopqrstuvwxyz'
ascii_uppercase = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ'
ascii_letters = ascii_lowercase + ascii_uppercase
digits = '0123456789'
hexdigits = digits + 'abcdef' + 'ABCDEF'
octdigits = '01234567'
punctuation = r"""!"#$%&'()*+,-./:;<=>?@[\]^_`{|}~"""
printable = digits + ascii_letters + punctuation + whitespace


class Formatter:
    """PEP 3101 string formatter."""

    def format(self, format_string, *args, **kwargs):
        return self.vformat(format_string, args, kwargs)

    def vformat(self, format_string, args, kwargs):
        result = []
        auto_arg_index = 0
        for literal_text, field_name, format_spec, conversion in self.parse(format_string):
            if literal_text:
                result.append(literal_text)
            if field_name is not None:
                if field_name == '':
                    field_name = str(auto_arg_index)
                    auto_arg_index += 1
                obj = self.get_field(field_name, args, kwargs)
                obj = self.convert_field(obj, conversion)
                format_spec = self.vformat(format_spec, args, kwargs) if format_spec else ''
                result.append(self.format_field(obj, format_spec))
        return ''.join(result)

    def parse(self, format_string):
        """Parse format_string into (literal, field_name, format_spec, conversion) tuples."""
        result = []
        i = 0
        length = len(format_string)
        while i < length:
            # Find next '{' or '}'
            literal_start = i
            while i < length and format_string[i] not in '{}':
                i += 1
            literal = format_string[literal_start:i]
            if i >= length:
                result.append((literal, None, None, None))
                break
            ch = format_string[i]
            if ch == '{':
                if i + 1 < length and format_string[i + 1] == '{':
                    result.append((literal + '{', None, None, None))
                    i += 2
                    continue
                # Find matching '}'
                i += 1
                field_start = i
                depth = 1
                while i < length and depth > 0:
                    if format_string[i] == '{':
                        depth += 1
                    elif format_string[i] == '}':
                        depth -= 1
                    i += 1
                field = format_string[field_start:i - 1]
                # Parse field: name!conversion:format_spec
                conversion = None
                format_spec = ''
                if '!' in field:
                    parts = field.split('!', 1)
                    field_name = parts[0]
                    rest = parts[1]
                    if ':' in rest:
                        conversion = rest[0]
                        format_spec = rest[2:]
                    else:
                        conversion = rest
                elif ':' in field:
                    parts = field.split(':', 1)
                    field_name = parts[0]
                    format_spec = parts[1]
                else:
                    field_name = field
                result.append((literal, field_name, format_spec, conversion))
            elif ch == '}':
                if i + 1 < length and format_string[i + 1] == '}':
                    result.append((literal + '}', None, None, None))
                    i += 2
                    continue
                raise ValueError("Single '}' encountered in format string")
        return result

    def get_field(self, field_name, args, kwargs):
        # Try integer index first
        try:
            idx = int(field_name)
            return args[idx]
        except (ValueError, TypeError):
            pass
        # Try keyword
        parts = field_name.split('.', 1)
        first = parts[0]
        try:
            idx = int(first)
            obj = args[idx]
        except (ValueError, TypeError):
            obj = kwargs[first]
        for attr in parts[1:]:
            try:
                idx = int(attr)
                obj = obj[idx]
            except (ValueError, TypeError):
                obj = getattr(obj, attr)
        return obj

    def convert_field(self, value, conversion):
        if conversion is None:
            return value
        elif conversion == 's':
            return str(value)
        elif conversion == 'r':
            return repr(value)
        elif conversion == 'a':
            return repr(value)  # ascii() not always available
        raise ValueError("Unknown conversion specifier " + str(conversion))

    def format_field(self, value, format_spec):
        return format(value, format_spec)


_template_pattern = re.compile(r"""
    \$(?:
        (\$) |                  # Escape - two dollar signs
        ([_a-z][_a-z0-9]*) |   # Simple identifier
        \{([^}]*)\}            # Braced identifier
    )
""", re.IGNORECASE | re.VERBOSE)


class Template:
    """A string class for supporting $-substitutions."""

    delimiter = '$'

    def __init__(self, template):
        self.template = template

    def substitute(self, mapping=None, **kws):
        if mapping is None:
            mapping = kws
        elif kws:
            d = dict(mapping)
            d.update(kws)
            mapping = d

        def convert(match):
            escaped = match.group(1)
            named = match.group(2)
            braced = match.group(3)
            if escaped is not None:
                return '$'
            if named is not None:
                return str(mapping[named])
            if braced is not None:
                return str(mapping[braced])
            raise ValueError('Unrecognized named group in pattern')
        return _template_pattern.sub(convert, self.template)

    def safe_substitute(self, mapping=None, **kws):
        if mapping is None:
            mapping = kws
        elif kws:
            d = dict(mapping)
            d.update(kws)
            mapping = d

        def convert(match):
            escaped = match.group(1)
            named = match.group(2)
            braced = match.group(3)
            if escaped is not None:
                return '$'
            if named is not None:
                try:
                    return str(mapping[named])
                except KeyError:
                    return match.group()
            if braced is not None:
                try:
                    return str(mapping[braced])
                except KeyError:
                    return match.group()
            return match.group()
        return _template_pattern.sub(convert, self.template)
