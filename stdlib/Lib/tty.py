"""tty — Terminal control functions."""

import sys
import os

# Indexes into the mode list returned by termios.tcgetattr()
IFLAG = 0
OFLAG = 1
CFLAG = 2
LFLAG = 3
ISPEED = 4
OSPEED = 5
CC = 6


def setraw(fd, when=None):
    """Put terminal into raw mode.

    Args:
        fd: File descriptor for the terminal.
        when: Optional; when to apply the change (termios constant).
              Defaults to termios.TCSAFLUSH.
    """
    import termios
    if when is None:
        when = termios.TCSAFLUSH
    mode = termios.tcgetattr(fd)
    mode[IFLAG] = mode[IFLAG] & ~(termios.BRKINT | termios.ICRNL |
                                   termios.INPCK | termios.ISTRIP |
                                   termios.IXON)
    mode[OFLAG] = mode[OFLAG] & ~(termios.OPOST)
    mode[CFLAG] = mode[CFLAG] & ~(termios.CSIZE | termios.PARENB)
    mode[CFLAG] = mode[CFLAG] | termios.CS8
    mode[LFLAG] = mode[LFLAG] & ~(termios.ECHO | termios.ICANON |
                                   termios.IEXTEN | termios.ISIG)
    mode[CC][termios.VMIN] = 1
    mode[CC][termios.VTIME] = 0
    termios.tcsetattr(fd, when, mode)


def setcbreak(fd, when=None):
    """Put terminal into cbreak mode.

    In cbreak mode, characters are available one at a time but special
    characters (interrupt, quit, etc.) are still processed.

    Args:
        fd: File descriptor for the terminal.
        when: Optional; when to apply the change (termios constant).
              Defaults to termios.TCSAFLUSH.
    """
    import termios
    if when is None:
        when = termios.TCSAFLUSH
    mode = termios.tcgetattr(fd)
    mode[LFLAG] = mode[LFLAG] & ~(termios.ECHO | termios.ICANON)
    mode[CC][termios.VMIN] = 1
    mode[CC][termios.VTIME] = 0
    termios.tcsetattr(fd, when, mode)
