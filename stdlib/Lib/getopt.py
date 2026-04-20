"""Parser for command line options (classic Unix-style).

This module helps scripts to parse the command line arguments in sys.argv.
It supports the same conventions as the Unix getopt() function.
"""

__all__ = ['GetoptError', 'error', 'getopt', 'gnu_getopt']


class GetoptError(Exception):
    """Exception raised on getopt errors."""
    def __init__(self, msg, opt=''):
        self.msg = msg
        self.opt = opt
        super().__init__(msg)

    def __str__(self):
        return self.msg


error = GetoptError


def getopt(args, shortopts, longopts=[]):
    """Parse command line options.

    args: argument list (usually sys.argv[1:])
    shortopts: option letters, with ':' for options requiring argument
    longopts: list of long option names (with '=' suffix if they take args)

    Returns (opts, args) where opts is list of (option, value) pairs.
    """
    opts = []
    if isinstance(longopts, str):
        longopts = [longopts]
    else:
        longopts = list(longopts)
    while args and args[0].startswith('-') and args[0] != '-':
        if args[0] == '--':
            args = args[1:]
            break
        if args[0].startswith('--'):
            opts, args = do_longs(opts, args[0][2:], longopts, args[1:])
        else:
            opts, args = do_shorts(opts, args[0][1:], shortopts, args[1:])
    return opts, args


def gnu_getopt(args, shortopts, longopts=[]):
    """Like getopt but allows interspersing options and non-option arguments."""
    opts = []
    prog_args = []
    if isinstance(longopts, str):
        longopts = [longopts]
    else:
        longopts = list(longopts)

    while args:
        if args[0] == '--':
            prog_args += args[1:]
            break
        if args[0].startswith('--'):
            opts, args = do_longs(opts, args[0][2:], longopts, args[1:])
        elif args[0].startswith('-') and args[0] != '-':
            opts, args = do_shorts(opts, args[0][1:], shortopts, args[1:])
        else:
            prog_args.append(args[0])
            args = args[1:]

    return opts, prog_args


def short_has_arg(opt, shortopts):
    """Check if a short option requires an argument."""
    for i, c in enumerate(shortopts):
        if c == opt:
            return i + 1 < len(shortopts) and shortopts[i + 1] == ':'
    raise GetoptError('option -%s not recognized' % opt, opt)

def long_has_args(opt, longopts):
    """Check if a long option requires an argument. Returns (has_arg, option)."""
    possibilities = [o for o in longopts if o == opt or o == opt + '=' or o.startswith(opt) or o.startswith(opt + '=')]
    if not possibilities:
        raise GetoptError('option --%s not recognized' % opt, opt)
    # Exact match takes priority
    if opt in possibilities:
        return False, opt
    if opt + '=' in possibilities:
        return True, opt
    # Prefix match — must be unique
    if len(possibilities) > 1:
        raise GetoptError('option --%s not a unique prefix' % opt, opt)
    match = possibilities[0]
    if match.endswith('='):
        return True, match[:-1]
    return False, match

def do_longs(opts, opt, longopts, args):
    """Process a long option."""
    if '=' in opt:
        opt, optarg = opt.split('=', 1)
    else:
        optarg = None

    has_arg = False
    match = None
    for lo in longopts:
        if lo == opt:
            match = lo
            break
        if lo == opt + '=':
            match = lo
            has_arg = True
            break
        if lo.startswith(opt):
            match = lo
            has_arg = lo.endswith('=')
            break
        if lo.startswith(opt + '='):
            match = lo
            has_arg = True
            break

    if match is None:
        raise GetoptError('option --%s not recognized' % opt, opt)

    if has_arg:
        if optarg is None:
            if not args:
                raise GetoptError('option --%s requires argument' % opt, opt)
            optarg, args = args[0], args[1:]
    elif optarg is not None:
        raise GetoptError('option --%s must not have an argument' % opt, opt)

    opt_name = match.rstrip('=')
    opts.append(('--' + opt_name, optarg or ''))
    return opts, args


def do_shorts(opts, optstring, shortopts, args):
    """Process short options."""
    i = 0
    while i < len(optstring):
        opt = optstring[i]
        i += 1
        if shortopts.find(opt) < 0:
            raise GetoptError('option -%s not recognized' % opt, opt)
        idx = shortopts.index(opt)
        if idx + 1 < len(shortopts) and shortopts[idx + 1] == ':':
            if i < len(optstring):
                optarg = optstring[i:]
                i = len(optstring)
            else:
                if not args:
                    raise GetoptError('option -%s requires argument' % opt, opt)
                optarg = args[0]
                args = args[1:]
            opts.append(('-' + opt, optarg))
        else:
            opts.append(('-' + opt, ''))
    return opts, args
