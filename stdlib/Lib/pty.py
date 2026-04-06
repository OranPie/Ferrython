"""pty — Pseudo-terminal utilities.

This is a stub implementation providing the standard API.
Functions delegate to os.openpty() / os.forkpty() when available,
and raise NotImplementedError otherwise.
"""

import os
import sys

STDIN_FILENO = 0
STDOUT_FILENO = 1
STDERR_FILENO = 2

CHILD = 0


def openpty():
    """Open a new pseudo-terminal pair.

    Returns a pair (master_fd, slave_fd) of file descriptors for the
    master and slave ends of the pseudo-terminal.

    Raises:
        NotImplementedError: If os.openpty is not available.
    """
    if hasattr(os, "openpty"):
        return os.openpty()
    raise NotImplementedError(
        "pty.openpty() requires os.openpty(), which is not available"
    )


def fork():
    """Fork and connect the child's controlling terminal to a pty.

    Returns a pair (pid, fd). In the child process, pid is 0 and fd
    is -1. In the parent, pid is the child's PID and fd is the file
    descriptor of the master end of the pseudo-terminal.

    Raises:
        NotImplementedError: If os.forkpty is not available.
    """
    if hasattr(os, "forkpty"):
        return os.forkpty()
    raise NotImplementedError(
        "pty.fork() requires os.forkpty(), which is not available"
    )


def spawn(argv, master_read=None, stdin_read=None):
    """Spawn a process and connect its controlling terminal to the
    current process's standard I/O.

    This is a convenience wrapper around fork().

    Args:
        argv: Command argument list (e.g. ['/bin/sh']).
        master_read: Optional callback to read from master fd.
        stdin_read: Optional callback to read from stdin.

    Raises:
        NotImplementedError: Pseudo-terminal support is not available.
    """
    raise NotImplementedError(
        "pty.spawn() is not available in this environment"
    )
