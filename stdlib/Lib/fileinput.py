"""fileinput — iterate over lines from multiple input streams."""

import sys
import os

_state = None


class FileInput:
    """Iterate over lines from a list of files."""

    def __init__(self, files=None, inplace=False, backup='', mode='r',
                 openhook=None):
        if files is None:
            files = sys.argv[1:]
        if isinstance(files, str):
            files = [files]
        if not files:
            files = ['-']
        self._files = list(files)
        self._inplace = inplace
        self._backup = backup
        self._mode = mode
        self._openhook = openhook
        self._filelineno = 0
        self._lineno = 0
        self._filename = None
        self._file = None
        self._isstdin = False
        self._fileindex = -1
        self._buffer = []
        self._bufindex = 0

    def __iter__(self):
        return self

    def __next__(self):
        while True:
            if self._buffer and self._bufindex < len(self._buffer):
                line = self._buffer[self._bufindex]
                self._bufindex += 1
                self._lineno += 1
                self._filelineno += 1
                return line
            # Need to open next file
            if self._file is not None:
                if not self._isstdin:
                    self._file.close()
                self._file = None
            self._fileindex += 1
            if self._fileindex >= len(self._files):
                raise StopIteration
            self._filename = self._files[self._fileindex]
            self._filelineno = 0
            if self._filename == '-':
                self._isstdin = True
                self._file = sys.stdin
            else:
                self._isstdin = False
                if self._openhook:
                    self._file = self._openhook(self._filename, self._mode)
                else:
                    self._file = open(self._filename, self._mode)
            self._buffer = self._file.readlines()
            self._bufindex = 0

    def __enter__(self):
        return self

    def __exit__(self, *args):
        self.close()
        return False

    def filename(self):
        return self._filename

    def fileno(self):
        if self._file:
            try:
                return self._file.fileno()
            except Exception:
                pass
        return -1

    def lineno(self):
        return self._lineno

    def filelineno(self):
        return self._filelineno

    def isfirstline(self):
        return self._filelineno == 1

    def isstdin(self):
        return self._isstdin

    def nextfile(self):
        if self._file and not self._isstdin:
            self._file.close()
        self._file = None
        self._buffer = []
        self._bufindex = 0

    def close(self):
        self.nextfile()
        self._files = []
        global _state
        if _state is self:
            _state = None


def input(files=None, inplace=False, backup='', mode='r', openhook=None):
    global _state
    if _state and _state._files:
        raise RuntimeError("input() already active")
    _state = FileInput(files, inplace, backup, mode, openhook)
    return _state


def close():
    global _state
    if _state:
        _state.close()
        _state = None


def filename():
    if not _state:
        raise RuntimeError("no active input()")
    return _state.filename()


def fileno():
    if not _state:
        raise RuntimeError("no active input()")
    return _state.fileno()


def lineno():
    if not _state:
        raise RuntimeError("no active input()")
    return _state.lineno()


def filelineno():
    if not _state:
        raise RuntimeError("no active input()")
    return _state.filelineno()


def isfirstline():
    if not _state:
        raise RuntimeError("no active input()")
    return _state.isfirstline()


def isstdin():
    if not _state:
        raise RuntimeError("no active input()")
    return _state.isstdin()


def nextfile():
    if not _state:
        raise RuntimeError("no active input()")
    return _state.nextfile()
