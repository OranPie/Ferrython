"""termios — POSIX style tty control.

This is a stub implementation that provides the standard constants
and function signatures. The functions raise NotImplementedError
since low-level terminal I/O requires native OS support.
"""

# --- tcsetattr 'when' constants ---
TCSANOW = 0
TCSADRAIN = 1
TCSAFLUSH = 2

# --- Input mode flags (c_iflag) ---
IGNBRK = 0o000001
BRKINT = 0o000002
IGNPAR = 0o000004
PARMRK = 0o000010
INPCK = 0o000020
ISTRIP = 0o000040
INLCR = 0o000100
IGNCR = 0o000200
ICRNL = 0o000400
IUCLC = 0o001000
IXON = 0o002000
IXANY = 0o004000
IXOFF = 0o010000
IMAXBEL = 0o020000
IUTF8 = 0o040000

# --- Output mode flags (c_oflag) ---
OPOST = 0o000001
OLCUC = 0o000002
ONLCR = 0o000004
OCRNL = 0o000010
ONOCR = 0o000020
ONLRET = 0o000040
OFILL = 0o000100
OFDEL = 0o000200

# --- Control mode flags (c_cflag) ---
CSIZE = 0o000060
CS5 = 0o000000
CS6 = 0o000020
CS7 = 0o000040
CS8 = 0o000060
CSTOPB = 0o000100
CREAD = 0o000200
PARENB = 0o000400
PARODD = 0o001000
HUPCL = 0o002000
CLOCAL = 0o004000

# --- Local mode flags (c_lflag) ---
ISIG = 0o000001
ICANON = 0o000002
ECHO = 0o000010
ECHOE = 0o000020
ECHOK = 0o000040
ECHONL = 0o000100
NOFLSH = 0o000200
TOSTOP = 0o000400
IEXTEN = 0o100000

# --- Special control character indexes ---
VINTR = 0
VQUIT = 1
VERASE = 2
VKILL = 3
VEOF = 4
VTIME = 5
VMIN = 6
VSTART = 8
VSTOP = 9
VSUSP = 10
VEOL = 11
VLNEXT = 15
VWERASE = 14
VREPRINT = 12
VDISCARD = 13

# --- Baud rates ---
B0 = 0o000000
B50 = 0o000001
B75 = 0o000002
B110 = 0o000003
B134 = 0o000004
B150 = 0o000005
B200 = 0o000006
B300 = 0o000007
B600 = 0o000010
B1200 = 0o000011
B1800 = 0o000012
B2400 = 0o000013
B4800 = 0o000014
B9600 = 0o000015
B19200 = 0o000016
B38400 = 0o000017
B57600 = 0o010001
B115200 = 0o010002
B230400 = 0o010003

# --- tcflush queue selectors ---
TCIFLUSH = 0
TCOFLUSH = 1
TCIOFLUSH = 2

# --- tcflow actions ---
TCOOFF = 0
TCOON = 1
TCIOFF = 2
TCION = 3

# Custom error for this stub module
error = OSError


def tcgetattr(fd):
    """Get the tty attributes for file descriptor *fd*.

    Returns a list: [iflag, oflag, cflag, lflag, ispeed, ospeed, cc]
    where cc is a list of special characters.

    Args:
        fd: File descriptor of the terminal.

    Raises:
        NotImplementedError: This is a stub implementation.
    """
    raise NotImplementedError(
        "termios.tcgetattr() is not available in this environment"
    )


def tcsetattr(fd, when, attributes):
    """Set the tty attributes for file descriptor *fd*.

    Args:
        fd: File descriptor of the terminal.
        when: When to apply changes (TCSANOW, TCSADRAIN, or TCSAFLUSH).
        attributes: A list in the form returned by tcgetattr().

    Raises:
        NotImplementedError: This is a stub implementation.
    """
    raise NotImplementedError(
        "termios.tcsetattr() is not available in this environment"
    )


def tcsendbreak(fd, duration):
    """Send a break on file descriptor *fd*.

    Args:
        fd: File descriptor of the terminal.
        duration: Duration of the break; zero sends a break for 0.25–0.5s.

    Raises:
        NotImplementedError: This is a stub implementation.
    """
    raise NotImplementedError(
        "termios.tcsendbreak() is not available in this environment"
    )


def tcdrain(fd):
    """Wait until all output written to *fd* has been transmitted.

    Args:
        fd: File descriptor of the terminal.

    Raises:
        NotImplementedError: This is a stub implementation.
    """
    raise NotImplementedError(
        "termios.tcdrain() is not available in this environment"
    )


def tcflush(fd, queue):
    """Discard queued data on file descriptor *fd*.

    Args:
        fd: File descriptor of the terminal.
        queue: TCIFLUSH for input, TCOFLUSH for output, TCIOFLUSH for both.

    Raises:
        NotImplementedError: This is a stub implementation.
    """
    raise NotImplementedError(
        "termios.tcflush() is not available in this environment"
    )


def tcflow(fd, action):
    """Suspend or resume input or output on file descriptor *fd*.

    Args:
        fd: File descriptor of the terminal.
        action: TCOOFF, TCOON, TCIOFF, or TCION.

    Raises:
        NotImplementedError: This is a stub implementation.
    """
    raise NotImplementedError(
        "termios.tcflow() is not available in this environment"
    )
