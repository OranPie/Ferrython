"""A powerful, extensible, and easy-to-use option parser.

Simplified implementation of the standard library optparse module,
compatible with CPython 3.8's optparse interface.
"""


class OptionError(Exception):
    """Raised for option-related errors."""
    pass


class BadOptionError(OptionError):
    """Raised when an invalid option is encountered."""
    pass


class OptionValueError(OptionError):
    """Raised when an invalid option value is encountered."""
    pass


class Values:
    """Container for option values, accessible as attributes."""

    def __init__(self, defaults=None):
        if defaults:
            for key, val in defaults.items():
                setattr(self, key, val)

    def __repr__(self):
        items = sorted(self.__dict__.items())
        parts = [f"{k}={v!r}" for k, v in items]
        return "Values({" + ", ".join(parts) + "})"

    def __eq__(self, other):
        if isinstance(other, Values):
            return self.__dict__ == other.__dict__
        return NotImplemented

    def ensure_value(self, attr, value):
        if not hasattr(self, attr) or getattr(self, attr) is None:
            setattr(self, attr, value)
        return getattr(self, attr)


class Option:
    """Represents a single command-line option."""

    ACTIONS = ('store', 'store_true', 'store_false', 'store_const',
               'append', 'count', 'help', 'version')
    STORE_ACTIONS = ('store', 'store_const', 'store_true', 'store_false',
                     'append', 'count')
    TYPES = ('string', 'int', 'float', 'choice')

    def __init__(self, *opts, **kwargs):
        self._short_opts = []
        self._long_opts = []
        for opt in opts:
            if opt.startswith('--'):
                self._long_opts.append(opt)
            elif opt.startswith('-'):
                self._short_opts.append(opt)
            else:
                raise OptionError(f"invalid option string {opt!r}")

        self.action = kwargs.get('action', 'store')
        self.type = kwargs.get('type', None)
        self.dest = kwargs.get('dest', None)
        self.default = kwargs.get('default', None)
        self.help = kwargs.get('help', '')
        self.const = kwargs.get('const', None)
        self.choices = kwargs.get('choices', None)
        self.metavar = kwargs.get('metavar', None)

        # Infer dest from option names
        if self.dest is None:
            if self._long_opts:
                self.dest = self._long_opts[0][2:].replace('-', '_')
            elif self._short_opts:
                self.dest = self._short_opts[0][1:].replace('-', '_')

        # Infer type for store action
        if self.type is None and self.action == 'store':
            self.type = 'string'

    def takes_value(self):
        return self.action == 'store' or self.action == 'append'

    def process(self, opt, value, values):
        if self.action == 'store':
            setattr(values, self.dest, self._convert_value(opt, value))
        elif self.action == 'store_true':
            setattr(values, self.dest, True)
        elif self.action == 'store_false':
            setattr(values, self.dest, False)
        elif self.action == 'store_const':
            setattr(values, self.dest, self.const)
        elif self.action == 'append':
            lst = getattr(values, self.dest, None)
            if lst is None:
                lst = []
                setattr(values, self.dest, lst)
            lst.append(self._convert_value(opt, value))
        elif self.action == 'count':
            cur = getattr(values, self.dest, 0)
            if cur is None:
                cur = 0
            setattr(values, self.dest, cur + 1)

    def _convert_value(self, opt, value):
        if value is None:
            return None
        if self.type == 'int':
            try:
                return int(value)
            except ValueError:
                raise OptionValueError(
                    f"option {opt}: invalid integer value: {value!r}")
        elif self.type == 'float':
            try:
                return float(value)
            except ValueError:
                raise OptionValueError(
                    f"option {opt}: invalid float value: {value!r}")
        elif self.type == 'choice':
            if self.choices and value not in self.choices:
                raise OptionValueError(
                    f"option {opt}: invalid choice: {value!r}")
            return value
        return value


class OptionGroup:
    """A group of related options."""

    def __init__(self, parser, title, description=None):
        self.parser = parser
        self.title = title
        self.description = description
        self.option_list = []

    def add_option(self, *args, **kwargs):
        opt = Option(*args, **kwargs)
        self.option_list.append(opt)
        self.parser._register_option(opt)
        return opt


class OptionParser:
    """Main option parser class."""

    def __init__(self, usage=None, description=None, version=None,
                 prog=None, add_help_option=True):
        self.usage = usage
        self.description = description
        self.version = version
        self.prog = prog
        self.option_list = []
        self.option_groups = []
        self._short_map = {}
        self._long_map = {}
        self._defaults = {}

        if add_help_option:
            self.add_option('-h', '--help', action='store_true',
                            dest='help', default=False,
                            help='show this help message and exit')
        if version:
            self.add_option('--version', action='store_true',
                            dest='version', default=False,
                            help='show version and exit')

    def add_option(self, *args, **kwargs):
        opt = Option(*args, **kwargs)
        self.option_list.append(opt)
        self._register_option(opt)
        return opt

    def _register_option(self, opt):
        for s in opt._short_opts:
            self._short_map[s] = opt
        for l in opt._long_opts:
            self._long_map[l] = opt
        if opt.dest and opt.default is not None:
            self._defaults[opt.dest] = opt.default

    def add_option_group(self, *args, **kwargs):
        if len(args) == 1 and isinstance(args[0], OptionGroup):
            group = args[0]
        else:
            group = OptionGroup(self, *args, **kwargs)
        self.option_groups.append(group)
        return group

    def set_defaults(self, **kwargs):
        self._defaults.update(kwargs)

    def get_default_values(self):
        defaults = {}
        for opt in self.option_list:
            if opt.dest:
                defaults.setdefault(opt.dest, opt.default)
        for group in self.option_groups:
            for opt in group.option_list:
                if opt.dest:
                    defaults.setdefault(opt.dest, opt.default)
        defaults.update(self._defaults)
        return Values(defaults)

    def parse_args(self, args=None):
        if args is None:
            import sys
            args = sys.argv[1:]

        values = self.get_default_values()
        remaining = []
        i = 0
        while i < len(args):
            arg = args[i]

            if arg == '--':
                remaining.extend(args[i + 1:])
                break

            if arg.startswith('--'):
                if '=' in arg:
                    key, val = arg.split('=', 1)
                    opt = self._long_map.get(key)
                    if opt is None:
                        raise BadOptionError(f"no such option: {key}")
                    opt.process(key, val, values)
                else:
                    opt = self._long_map.get(arg)
                    if opt is None:
                        raise BadOptionError(f"no such option: {arg}")
                    if opt.takes_value():
                        i += 1
                        if i >= len(args):
                            raise OptionError(
                                f"option {arg} requires an argument")
                        opt.process(arg, args[i], values)
                    else:
                        opt.process(arg, None, values)

            elif arg.startswith('-') and len(arg) > 1:
                opt = self._short_map.get(arg[:2])
                if opt is None:
                    raise BadOptionError(f"no such option: {arg[:2]}")
                if opt.takes_value():
                    if len(arg) > 2:
                        opt.process(arg[:2], arg[2:], values)
                    else:
                        i += 1
                        if i >= len(args):
                            raise OptionError(
                                f"option {arg} requires an argument")
                        opt.process(arg[:2], args[i], values)
                else:
                    opt.process(arg[:2], None, values)
                    if len(arg) > 2:
                        # Handle combined short options like -vvv
                        for ch in arg[2:]:
                            short = '-' + ch
                            opt2 = self._short_map.get(short)
                            if opt2 is None:
                                raise BadOptionError(
                                    f"no such option: {short}")
                            opt2.process(short, None, values)
            else:
                remaining.append(arg)

            i += 1

        return values, remaining

    def format_help(self):
        lines = []
        if self.usage:
            lines.append(f"Usage: {self.usage}")
            lines.append('')
        if self.description:
            lines.append(self.description)
            lines.append('')
        lines.append('Options:')
        for opt in self.option_list:
            names = ', '.join(opt._short_opts + opt._long_opts)
            if opt.metavar:
                names += ' ' + opt.metavar
            line = f"  {names:30s} {opt.help}"
            lines.append(line)
        for group in self.option_groups:
            lines.append('')
            lines.append(f"  {group.title}:")
            if group.description:
                lines.append(f"    {group.description}")
            for opt in group.option_list:
                names = ', '.join(opt._short_opts + opt._long_opts)
                line = f"    {names:28s} {opt.help}"
                lines.append(line)
        return '\n'.join(lines)

    def print_help(self):
        print(self.format_help())

    def error(self, msg):
        raise OptionError(msg)
