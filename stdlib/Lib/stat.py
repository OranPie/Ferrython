"""Generic interface to all platform-specific stat constants.

Defines constants for interpreting the results of os.stat() and os.fstat().
"""

# Encoding of the file mode bits
S_IFDIR  = 0o040000  # directory
S_IFCHR  = 0o020000  # character device
S_IFBLK  = 0o060000  # block device
S_IFREG  = 0o100000  # regular file
S_IFIFO  = 0o010000  # fifo (named pipe)
S_IFLNK  = 0o120000  # symbolic link
S_IFSOCK = 0o140000  # socket file
S_IFMT   = 0o170000  # mask for type of file

# Mode bits
S_ISUID = 0o4000  # set UID bit
S_ISGID = 0o2000  # set GID bit
S_ISVTX = 0o1000  # sticky bit

S_IRWXU = 0o0700  # owner mask
S_IRUSR = 0o0400  # owner read
S_IWUSR = 0o0200  # owner write
S_IXUSR = 0o0100  # owner execute

S_IRWXG = 0o0070  # group mask
S_IRGRP = 0o0040  # group read
S_IWGRP = 0o0020  # group write
S_IXGRP = 0o0010  # group execute

S_IRWXO = 0o0007  # other mask
S_IROTH = 0o0004  # other read
S_IWOTH = 0o0002  # other write
S_IXOTH = 0o0001  # other execute

# Names for some frequently used constants
S_ENFMT = S_ISGID
S_IREAD = S_IRUSR
S_IWRITE = S_IWUSR
S_IEXEC = S_IXUSR


def S_ISDIR(mode):
    """Return True if mode is from a directory."""
    return (mode & S_IFMT) == S_IFDIR

def S_ISCHR(mode):
    """Return True if mode is from a character special device file."""
    return (mode & S_IFMT) == S_IFCHR

def S_ISBLK(mode):
    """Return True if mode is from a block special device file."""
    return (mode & S_IFMT) == S_IFBLK

def S_ISREG(mode):
    """Return True if mode is from a regular file."""
    return (mode & S_IFMT) == S_IFREG

def S_ISFIFO(mode):
    """Return True if mode is from a FIFO (named pipe)."""
    return (mode & S_IFMT) == S_IFIFO

def S_ISLNK(mode):
    """Return True if mode is from a symbolic link."""
    return (mode & S_IFMT) == S_IFLNK

def S_ISSOCK(mode):
    """Return True if mode is from a socket."""
    return (mode & S_IFMT) == S_IFSOCK

def S_IMODE(mode):
    """Return the portion of the file's mode that can be set by os.chmod()."""
    return mode & 0o7777

def S_IFMT_func(mode):
    """Return the portion of the file's mode that describes the file type."""
    return mode & S_IFMT

def filemode(mode):
    """Convert a file's mode to a string of the form '-rwxrwxrwx'."""
    perm = []
    _filemode_table = (
        ((S_IFLNK,         "l"),
         (S_IFREG,         "-"),
         (S_IFBLK,         "b"),
         (S_IFDIR,         "d"),
         (S_IFCHR,         "c"),
         (S_IFIFO,         "p")),
        ((S_IRUSR,         "r"),),
        ((S_IWUSR,         "w"),),
        ((S_IXUSR|S_ISUID, "s"),
         (S_ISUID,         "S"),
         (S_IXUSR,         "x")),
        ((S_IRGRP,         "r"),),
        ((S_IWGRP,         "w"),),
        ((S_IXGRP|S_ISGID, "s"),
         (S_ISGID,         "S"),
         (S_IXGRP,         "x")),
        ((S_IROTH,         "r"),),
        ((S_IWOTH,         "w"),),
        ((S_IXOTH|S_ISVTX, "t"),
         (S_ISVTX,         "T"),
         (S_IXOTH,         "x"))
    )
    for table in _filemode_table:
        for bit, char in table:
            if mode & bit == bit:
                perm.append(char)
                break
        else:
            perm.append("-")
    return "".join(perm)


# Windows-specific constants (provide them but as 0 on non-Windows)
FILE_ATTRIBUTE_ARCHIVE = 32
FILE_ATTRIBUTE_COMPRESSED = 2048
FILE_ATTRIBUTE_DEVICE = 64
FILE_ATTRIBUTE_DIRECTORY = 16
FILE_ATTRIBUTE_ENCRYPTED = 16384
FILE_ATTRIBUTE_HIDDEN = 2
FILE_ATTRIBUTE_INTEGRITY_STREAM = 32768
FILE_ATTRIBUTE_NORMAL = 128
FILE_ATTRIBUTE_NOT_CONTENT_INDEXED = 8192
FILE_ATTRIBUTE_NO_SCRUB_DATA = 131072
FILE_ATTRIBUTE_OFFLINE = 4096
FILE_ATTRIBUTE_READONLY = 1
FILE_ATTRIBUTE_REPARSE_POINT = 1024
FILE_ATTRIBUTE_SPARSE_FILE = 512
FILE_ATTRIBUTE_SYSTEM = 4
FILE_ATTRIBUTE_TEMPORARY = 256
FILE_ATTRIBUTE_VIRTUAL = 65536
