"""A collection of string constants, Template, and Formatter."""

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


def capwords(s, sep=None):
    """Split a string into words and capitalize each word."""
    return (sep or ' ').join(word.capitalize() for word in s.split(sep))


def _formatter_field_name_split(field_name):
    first_end = len(field_name)
    for i, char in enumerate(field_name):
        if char == '.' or char == '[':
            first_end = i
            break

    first = field_name[:first_end]
    if first.isdigit():
        first = int(first)

    rest = []
    i = first_end
    while i < len(field_name):
        char = field_name[i]
        if char == '.':
            i += 1
            start = i
            while i < len(field_name) and field_name[i] not in '.[':
                i += 1
            rest.append((True, field_name[start:i]))
        elif char == '[':
            i += 1
            start = i
            while i < len(field_name) and field_name[i] != ']':
                i += 1
            if i >= len(field_name):
                raise ValueError("Missing ']' in format string")
            item = field_name[start:i]
            if item.isdigit():
                item = int(item)
            rest.append((False, item))
            i += 1
        else:
            raise ValueError("Only '.' or '[' may follow ']' in format field specifier")
    return first, rest


def _parse_formatter_field(field):
    conversion = None
    format_spec = ''
    bang = field.find('!')
    colon = field.find(':')

    if bang != -1 and (colon == -1 or bang < colon):
        field_name = field[:bang]
        rest = field[bang + 1:]
        if not rest:
            conversion = ''
        elif len(rest) >= 2 and rest[1] == ':':
            conversion = rest[0]
            format_spec = rest[2:]
        else:
            conversion = rest
    elif colon != -1:
        field_name = field[:colon]
        format_spec = field[colon + 1:]
    else:
        field_name = field

    return field_name, conversion, format_spec


class Formatter:
    """PEP 3101 string formatter."""

    def __init__(self):
        pass

    def format(*args, **kwargs):
        if len(args) < 2:
            if 'format_string' in kwargs:
                raise TypeError("format_string must be passed as a positional argument")
            raise TypeError("format() requires a format string")
        self = args[0]
        format_string = args[1]
        return self.vformat(format_string, args[2:], kwargs)

    def vformat(self, format_string, args, kwargs):
        used_args = set()
        result, _ = self._vformat(format_string, args, kwargs, used_args, 2)
        self.check_unused_args(used_args, args, kwargs)
        return result

    def _vformat(self, format_string, args, kwargs, used_args, recursion_depth,
                 auto_arg_index=0):
        if recursion_depth < 0:
            raise ValueError("Max string recursion exceeded")

        result = []
        for literal_text, field_name, format_spec, conversion in self.parse(format_string):
            if literal_text:
                result.append(literal_text)
            if field_name is not None:
                if field_name == '':
                    if auto_arg_index is False:
                        raise ValueError("cannot switch from manual field specification to automatic field numbering")
                    field_name = str(auto_arg_index)
                    auto_arg_index += 1
                elif field_name.isdigit():
                    if auto_arg_index:
                        raise ValueError("cannot switch from automatic field numbering to manual field specification")
                    auto_arg_index = False

                obj, arg_used = self.get_field(field_name, args, kwargs)
                used_args.add(arg_used)
                obj = self.convert_field(obj, conversion)
                if format_spec is None:
                    format_spec = ''
                format_spec, auto_arg_index = self._vformat(
                    format_spec, args, kwargs, used_args, recursion_depth - 1,
                    auto_arg_index=auto_arg_index)
                result.append(self.format_field(obj, format_spec))
        return ''.join(result), auto_arg_index

    def parse(self, format_string):
        """Parse format_string into (literal, field_name, format_spec, conversion) tuples."""
        result = []
        i = 0
        length = len(format_string)
        literal = []
        while i < length:
            ch = format_string[i]
            if ch == '{':
                if i + 1 < length and format_string[i + 1] == '{':
                    literal.append('{')
                    i += 2
                    continue
                i += 1
                start = i
                depth = 1
                while i < length and depth:
                    if format_string[i] == '{':
                        depth += 1
                    elif format_string[i] == '}':
                        depth -= 1
                    if depth:
                        i += 1
                if depth:
                    raise ValueError("expected '}' before end of string")
                field = format_string[start:i]
                i += 1
                field_name, conversion, format_spec = _parse_formatter_field(field)
                result.append((''.join(literal), field_name, format_spec, conversion))
                literal = []
            elif ch == '}':
                if i + 1 < length and format_string[i + 1] == '}':
                    literal.append('}')
                    i += 2
                    continue
                raise ValueError("Single '}' encountered in format string")
            else:
                literal.append(ch)
                i += 1
        if literal:
            result.append((''.join(literal), None, None, None))
        return result

    def get_field(self, field_name, args, kwargs):
        first, rest = _formatter_field_name_split(field_name)
        obj = self.get_value(first, args, kwargs)
        for is_attr, attr in rest:
            if is_attr:
                obj = getattr(obj, attr)
            else:
                obj = obj[attr]
        return obj, first

    def get_value(self, key, args, kwargs):
        if isinstance(key, int):
            return args[key]
        return kwargs[key]

    def check_unused_args(self, used_args, args, kwargs):
        pass

    def convert_field(self, value, conversion):
        if conversion is None:
            return value
        elif conversion == 's':
            return str(value)
        elif conversion == 'r':
            return repr(value)
        elif conversion == 'a':
            return ascii(value)
        raise ValueError("Unknown conversion specifier " + str(conversion))

    def format_field(self, value, format_spec):
        return format(value, format_spec)


class _TemplateChainMap:
    def __init__(self, mapping, kws):
        self.mapping = mapping
        self.kws = kws

    def __getitem__(self, key):
        try:
            return self.kws[key]
        except KeyError:
            return self.mapping[key]


def _compile_template_pattern(cls):
    flags = getattr(cls, 'flags', re.IGNORECASE) | re.VERBOSE
    pattern = cls.__dict__.get('pattern')
    if pattern is not None:
        if isinstance(pattern, str):
            pattern = _normalize_template_pattern(pattern)
            cls.pattern = re.compile(pattern, flags)
        return

    delim = re.escape(getattr(cls, 'delimiter', '$'))
    idpattern = getattr(cls, 'idpattern', r'(?a:[_a-z][_a-z0-9]*)')
    braceidpattern = getattr(cls, 'braceidpattern', None) or idpattern
    cls.pattern = re.compile(
        delim + r'(?:(?P<escaped>' + delim + r')|(?P<named>' + idpattern +
        r')|\{(?P<braced>' + braceidpattern + r')\}|(?P<invalid>))',
        flags)


def _normalize_template_pattern(pattern):
    result = []
    escaped = False
    in_char_class = False
    i = 0
    while i < len(pattern):
        char = pattern[i]
        if escaped:
            result.append(char)
            escaped = False
        elif char == '\\':
            result.append(char)
            escaped = True
        elif char == '[':
            result.append(char)
            in_char_class = True
        elif char == ']':
            result.append(char)
            in_char_class = False
        elif not in_char_class and char == '{':
            j = i + 1
            while j < len(pattern) and (pattern[j].isdigit() or pattern[j] == ','):
                j += 1
            if j > i + 1 and j < len(pattern) and pattern[j] == '}':
                result.append(pattern[i:j + 1])
                i = j
            else:
                result.append('\\{')
        elif not in_char_class and char == '}':
            result.append('\\}')
        else:
            result.append(char)
        i += 1
    return ''.join(result)


def _prepare_template_mapping(args, kws, method_name):
    if not args:
        raise TypeError(method_name + "() missing template argument")
    if len(args) > 2:
        raise TypeError(method_name + "() takes at most 2 positional arguments")
    template = args[0]
    if len(args) == 1:
        mapping = kws
    elif kws:
        mapping = _TemplateChainMap(args[1], kws)
    else:
        mapping = args[1]
    return template, mapping


class Template:
    """A string class for supporting $-substitutions."""

    delimiter = '$'
    idpattern = r'(?a:[_a-z][_a-z0-9]*)'
    braceidpattern = None
    flags = re.IGNORECASE

    @classmethod
    def __init_subclass__(cls):
        _compile_template_pattern(cls)

    def __init__(self, template):
        self.template = template

    def _invalid(self, mo):
        i = mo.start('invalid')
        before = self.template[:i]
        lineno = before.count('\n') + 1
        colno = len(before) - before.rfind('\n')
        raise ValueError("Invalid placeholder in string: line %d, col %d" %
                         (lineno, colno))

    def substitute(*args, **kws):
        self, mapping = _prepare_template_mapping(args, kws, "substitute")
        def convert(match):
            escaped = match.group('escaped')
            named = match.group('named')
            braced = match.group('braced')
            invalid = match.group('invalid')
            if escaped is not None:
                return self.delimiter
            if named is not None:
                return str(mapping[named])
            if braced is not None:
                return str(mapping[braced])
            if invalid is not None:
                self._invalid(match)
            raise ValueError("Unrecognized named group in pattern")
        return self.pattern.sub(convert, self.template)

    def safe_substitute(*args, **kws):
        self, mapping = _prepare_template_mapping(args, kws, "safe_substitute")
        def convert(match):
            escaped = match.group('escaped')
            named = match.group('named')
            braced = match.group('braced')
            invalid = match.group('invalid')
            if escaped is not None:
                return self.delimiter
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
            if invalid is not None:
                return match.group()
            raise ValueError("Unrecognized named group in pattern")
        return self.pattern.sub(convert, self.template)


_compile_template_pattern(Template)
