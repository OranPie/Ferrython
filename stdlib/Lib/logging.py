"""Pure Python implementation of the logging module.

Provides a flexible event logging system.
"""

import os
import sys
import time
import traceback
import io

# Log levels
CRITICAL = 50
FATAL = CRITICAL
ERROR = 40
WARNING = 30
WARN = WARNING
INFO = 20
DEBUG = 10
NOTSET = 0

_levelToName = {
    CRITICAL: 'CRITICAL',
    ERROR: 'ERROR',
    WARNING: 'WARNING',
    INFO: 'INFO',
    DEBUG: 'DEBUG',
    NOTSET: 'NOTSET',
}
_nameToLevel = {v: k for k, v in _levelToName.items()}

_lock = None
_handlers = {}
_handlerList = []


def getLevelName(level):
    """Return the textual representation of a logging level."""
    return _levelToName.get(level, "Level %s" % level)


class LogRecord:
    """A LogRecord instance represents an event being logged."""
    
    def __init__(self, name, level, pathname, lineno, msg, args, exc_info,
                 func=None, sinfo=None):
        self.name = name
        self.levelno = level
        self.levelname = getLevelName(level)
        self.pathname = pathname
        self.lineno = lineno
        self.msg = msg
        self.args = args
        self.exc_info = exc_info
        self.exc_text = None
        self.funcName = func
        self.sinfo = sinfo
        self.created = time.time()
        self.msecs = (self.created - int(self.created)) * 1000
        self.relativeCreated = 0
        self.process = os.getpid() if hasattr(os, 'getpid') else 0
        self.processName = 'MainProcess'
        self.thread = 0
        self.threadName = 'MainThread'
    
    def getMessage(self):
        msg = str(self.msg)
        if self.args:
            try:
                msg = msg % self.args
            except (TypeError, ValueError):
                pass
        return msg
    
    def __repr__(self):
        return '<LogRecord: %s, %s, %s, %s, "%s">' % (
            self.name, self.levelno, self.pathname, self.lineno, self.msg)


class Formatter:
    """Format a LogRecord into text."""
    
    default_format = '%(levelname)s:%(name)s:%(message)s'
    default_time_format = '%Y-%m-%d %H:%M:%S'
    default_msec_format = '%s,%03d'
    
    def __init__(self, fmt=None, datefmt=None, style='%', validate=True):
        self._fmt = fmt or self.default_format
        self.datefmt = datefmt
        self._style = style
    
    def formatTime(self, record, datefmt=None):
        ct = time.localtime(record.created) if hasattr(time, 'localtime') else None
        if ct and datefmt:
            s = time.strftime(datefmt, ct)
        elif ct:
            s = time.strftime(self.default_time_format, ct)
        else:
            s = str(record.created)
        return s
    
    def formatException(self, ei):
        if ei and len(ei) > 1 and ei[1]:
            return str(ei[1])
        return ''
    
    def formatMessage(self, record):
        return self._fmt % record.__dict__
    
    def format(self, record):
        record.message = record.getMessage()
        record.asctime = self.formatTime(record, self.datefmt)
        s = self.formatMessage(record)
        if record.exc_info and record.exc_info[1]:
            if not record.exc_text:
                record.exc_text = self.formatException(record.exc_info)
        if record.exc_text:
            if s[-1:] != '\n':
                s = s + '\n'
            s = s + record.exc_text
        return s


_defaultFormatter = Formatter()


class Filter:
    """Filter instances are used to perform arbitrary filtering of LogRecords."""
    
    def __init__(self, name=''):
        self.name = name
        self.nlen = len(name)
    
    def filter(self, record):
        if self.nlen == 0:
            return True
        if self.name == record.name:
            return True
        if record.name.startswith(self.name + '.'):
            return True
        return False


class Filterer:
    """A base class for loggers and handlers, allowing them to share
    a common method to add and remove filters."""
    
    def __init__(self):
        self.filters = []
    
    def addFilter(self, filter):
        if filter not in self.filters:
            self.filters.append(filter)
    
    def removeFilter(self, filter):
        if filter in self.filters:
            self.filters.remove(filter)
    
    def filter(self, record):
        for f in self.filters:
            if hasattr(f, 'filter'):
                result = f.filter(record)
            else:
                result = f(record)
            if not result:
                return False
        return True


class Handler(Filterer):
    """Handler instances dispatch logging events to specific destinations."""
    
    def __init__(self, level=NOTSET):
        Filterer.__init__(self)
        self.level = level
        self.formatter = None
    
    def setLevel(self, level):
        if isinstance(level, str):
            level = _nameToLevel.get(level.upper(), NOTSET)
        self.level = level
    
    def setFormatter(self, fmt):
        self.formatter = fmt
    
    def format(self, record):
        if self.formatter:
            return self.formatter.format(record)
        return _defaultFormatter.format(record)
    
    def emit(self, record):
        raise NotImplementedError("emit must be implemented by Handler subclasses")
    
    def handle(self, record):
        rv = self.filter(record)
        if rv:
            self.emit(record)
        return rv
    
    def flush(self):
        pass
    
    def close(self):
        pass


class StreamHandler(Handler):
    """A handler class which writes logging records to a stream."""
    
    terminator = '\n'
    
    def __init__(self, stream=None):
        Handler.__init__(self)
        if stream is None:
            stream = sys.stderr
        self.stream = stream
    
    def flush(self):
        if self.stream and hasattr(self.stream, 'flush'):
            self.stream.flush()
    
    def emit(self, record):
        try:
            msg = self.format(record)
            stream = self.stream
            stream.write(msg + self.terminator)
            self.flush()
        except Exception:
            pass


class FileHandler(StreamHandler):
    """A handler class which writes to a disk file."""
    
    def __init__(self, filename, mode='a', encoding=None, delay=False):
        self.baseFilename = os.path.abspath(filename)
        self.mode = mode
        self.encoding = encoding
        self.delay = delay
        if delay:
            Handler.__init__(self)
            self.stream = None
        else:
            StreamHandler.__init__(self, self._open())
    
    def _open(self):
        return open(self.baseFilename, self.mode)
    
    def close(self):
        if self.stream:
            try:
                self.flush()
            finally:
                stream = self.stream
                self.stream = None
                stream.close()
    
    def emit(self, record):
        if self.stream is None:
            self.stream = self._open()
        StreamHandler.emit(self, record)


class NullHandler(Handler):
    """A handler that does nothing. Used to prevent 'No handler' warnings."""
    def handle(self, record):
        pass
    def emit(self, record):
        pass


class PlaceHolder:
    def __init__(self, alogger):
        self.loggerMap = {alogger: None}
    def append(self, alogger):
        if alogger not in self.loggerMap:
            self.loggerMap[alogger] = None


class Manager:
    """There is one Manager instance, which holds the hierarchy of loggers."""
    
    def __init__(self, rootnode):
        self.root = rootnode
        self.disable = 0
        self.loggerDict = {}
        self.loggerClass = None
    
    def getLogger(self, name):
        rv = None
        if name in self.loggerDict:
            rv = self.loggerDict[name]
            if isinstance(rv, PlaceHolder):
                ph = rv
                rv = Logger(name)
                rv.manager = self
                self.loggerDict[name] = rv
                self._fixupParents(rv)
        else:
            rv = Logger(name)
            rv.manager = self
            self.loggerDict[name] = rv
            self._fixupParents(rv)
        return rv
    
    def _fixupParents(self, alogger):
        name = alogger.name
        i = name.rfind(".")
        rv = None
        while (i > 0) and not rv:
            substr = name[:i]
            if substr not in self.loggerDict:
                self.loggerDict[substr] = PlaceHolder(alogger)
            else:
                obj = self.loggerDict[substr]
                if isinstance(obj, Logger):
                    rv = obj
                else:
                    obj.append(alogger)
            i = name.rfind(".", 0, i - 1)
        if not rv:
            rv = self.root
        alogger.parent = rv


class Logger(Filterer):
    """Instances of Logger represent a single logging channel."""
    
    def __init__(self, name, level=NOTSET):
        Filterer.__init__(self)
        self.name = name
        self.level = level
        self.parent = None
        self.propagate = True
        self.handlers = []
        self.disabled = False
        self.manager = None
    
    def setLevel(self, level):
        if isinstance(level, str):
            level = _nameToLevel.get(level.upper(), NOTSET)
        self.level = level
    
    def getEffectiveLevel(self):
        logger = self
        while logger:
            if logger.level:
                return logger.level
            logger = logger.parent
        return NOTSET
    
    def isEnabledFor(self, level):
        return level >= self.getEffectiveLevel()
    
    def addHandler(self, hdlr):
        if hdlr not in self.handlers:
            self.handlers.append(hdlr)
    
    def removeHandler(self, hdlr):
        if hdlr in self.handlers:
            self.handlers.remove(hdlr)
    
    def hasHandlers(self):
        c = self
        rv = False
        while c:
            if c.handlers:
                rv = True
                break
            if not c.propagate:
                break
            c = c.parent
        return rv
    
    def callHandlers(self, record):
        c = self
        found = 0
        while c:
            for hdlr in c.handlers:
                found = found + 1
                if record.levelno >= hdlr.level:
                    hdlr.handle(record)
            if not c.propagate:
                c = None
            else:
                c = c.parent
        if found == 0:
            if record.levelno >= WARNING:
                sys.stderr.write('No handlers could be found for logger "%s"\n' % self.name)
    
    def handle(self, record):
        if not self.disabled and self.filter(record):
            self.callHandlers(record)
    
    def makeRecord(self, name, level, fn, lno, msg, args, exc_info,
                   func=None, extra=None, sinfo=None):
        rv = LogRecord(name, level, fn, lno, msg, args, exc_info, func, sinfo)
        if extra:
            for key in extra:
                rv.__dict__[key] = extra[key]
        return rv
    
    def _log(self, level, msg, args, exc_info=None, extra=None,
             stack_info=False, stacklevel=1):
        record = self.makeRecord(
            self.name, level, "(unknown file)", 0,
            msg, args, exc_info, extra=extra)
        self.handle(record)
    
    def debug(self, msg, *args, **kwargs):
        if self.isEnabledFor(DEBUG):
            self._log(DEBUG, msg, args, **kwargs)
    
    def info(self, msg, *args, **kwargs):
        if self.isEnabledFor(INFO):
            self._log(INFO, msg, args, **kwargs)
    
    def warning(self, msg, *args, **kwargs):
        if self.isEnabledFor(WARNING):
            self._log(WARNING, msg, args, **kwargs)
    
    warn = warning
    
    def error(self, msg, *args, **kwargs):
        if self.isEnabledFor(ERROR):
            self._log(ERROR, msg, args, **kwargs)
    
    def critical(self, msg, *args, **kwargs):
        if self.isEnabledFor(CRITICAL):
            self._log(CRITICAL, msg, args, **kwargs)
    
    fatal = critical
    
    def exception(self, msg, *args, exc_info=True, **kwargs):
        self.error(msg, *args, exc_info=exc_info, **kwargs)
    
    def log(self, level, msg, *args, **kwargs):
        if self.isEnabledFor(level):
            self._log(level, msg, args, **kwargs)


class RootLogger(Logger):
    """The root logger; not to be instantiated directly."""
    
    def __init__(self, level):
        Logger.__init__(self, "root", level)


root = RootLogger(WARNING)
Logger.root = root
Logger.manager = Manager(root)


def getLogger(name=None):
    """Return a logger with the specified name."""
    if not name or name == root.name:
        return root
    return Logger.manager.getLogger(name)


def basicConfig(**kwargs):
    """Do basic configuration for the logging system."""
    handlers_list = kwargs.get('handlers')
    if handlers_list is None:
        handlers_list = []
        filename = kwargs.get('filename')
        if filename:
            mode = kwargs.get('filemode', 'a')
            h = FileHandler(filename, mode)
        else:
            stream = kwargs.get('stream')
            h = StreamHandler(stream)
        handlers_list.append(h)
    
    fmt = kwargs.get('format')
    datefmt = kwargs.get('datefmt')
    style = kwargs.get('style', '%')
    formatter = Formatter(fmt, datefmt, style)
    
    for h in handlers_list:
        if h.formatter is None:
            h.setFormatter(formatter)
        root.addHandler(h)
    
    level = kwargs.get('level')
    if level is not None:
        if isinstance(level, str):
            level = _nameToLevel.get(level.upper(), NOTSET)
        root.setLevel(level)


def critical(msg, *args, **kwargs):
    root.critical(msg, *args, **kwargs)

def fatal(msg, *args, **kwargs):
    root.critical(msg, *args, **kwargs)

def error(msg, *args, **kwargs):
    root.error(msg, *args, **kwargs)

def exception(msg, *args, **kwargs):
    root.exception(msg, *args, **kwargs)

def warning(msg, *args, **kwargs):
    root.warning(msg, *args, **kwargs)

def warn(msg, *args, **kwargs):
    root.warning(msg, *args, **kwargs)

def info(msg, *args, **kwargs):
    root.info(msg, *args, **kwargs)

def debug(msg, *args, **kwargs):
    root.debug(msg, *args, **kwargs)

def log(level, msg, *args, **kwargs):
    root.log(level, msg, *args, **kwargs)

def disable(level=CRITICAL):
    root.manager.disable = level

def shutdown():
    pass

# Make the levels available at module level
CRITICAL = CRITICAL
FATAL = FATAL
ERROR = ERROR
WARNING = WARNING
WARN = WARN
INFO = INFO
DEBUG = DEBUG
NOTSET = NOTSET
