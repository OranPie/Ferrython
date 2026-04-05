"""filecmp — file and directory comparison utilities."""

import os
import stat


def cmp(f1, f2, shallow=True):
    """Compare two files, returning True if they seem equal.
    
    If shallow is True (default), only os.stat() signatures are compared.
    If shallow is False, file contents are compared.
    """
    s1 = os.stat(f1)
    s2 = os.stat(f2)

    # Quick check: different sizes means different
    if s1.st_size != s2.st_size:
        return False

    if shallow:
        # Same size + same mtime → assume equal
        if s1.st_mtime == s2.st_mtime:
            return True

    # Compare contents
    bufsize = 8192
    with open(f1, 'rb') as fp1, open(f2, 'rb') as fp2:
        while True:
            b1 = fp1.read(bufsize)
            b2 = fp2.read(bufsize)
            if b1 != b2:
                return False
            if not b1:
                return True


def cmpfiles(a, b, common, shallow=True):
    """Compare files in two directories.
    
    Returns (match, mismatch, errors) where each is a list of filenames.
    """
    match = []
    mismatch = []
    errors = []
    for name in common:
        try:
            result = cmp(os.path.join(a, name), os.path.join(b, name), shallow)
            if result:
                match.append(name)
            else:
                mismatch.append(name)
        except (OSError, IOError):
            errors.append(name)
    return match, mismatch, errors


class dircmp:
    """Compare the contents of two directories."""

    def __init__(self, a, b, ignore=None, hide=None):
        self.left = a
        self.right = b
        self.ignore = ignore or ['.', '..']
        self.hide = hide or [os.curdir, os.pardir]
        self._left_list = None
        self._right_list = None
        self._common = None
        self._left_only = None
        self._right_only = None

    def _ensure_lists(self):
        if self._left_list is not None:
            return
        try:
            self._left_list = [x for x in os.listdir(self.left) 
                              if x not in self.hide and x not in self.ignore]
        except OSError:
            self._left_list = []
        try:
            self._right_list = [x for x in os.listdir(self.right)
                               if x not in self.hide and x not in self.ignore]
        except OSError:
            self._right_list = []
        left_set = set(self._left_list)
        right_set = set(self._right_list)
        self._common = sorted(left_set & right_set)
        self._left_only = sorted(left_set - right_set)
        self._right_only = sorted(right_set - left_set)

    @property
    def common(self):
        self._ensure_lists()
        return self._common

    @property
    def left_only(self):
        self._ensure_lists()
        return self._left_only

    @property
    def right_only(self):
        self._ensure_lists()
        return self._right_only

    @property
    def common_dirs(self):
        self._ensure_lists()
        return [x for x in self._common
                if os.path.isdir(os.path.join(self.left, x))]

    @property
    def common_files(self):
        self._ensure_lists()
        return [x for x in self._common
                if os.path.isfile(os.path.join(self.left, x))]

    @property
    def same_files(self):
        match, _, _ = cmpfiles(self.left, self.right, self.common_files)
        return match

    @property
    def diff_files(self):
        _, mismatch, _ = cmpfiles(self.left, self.right, self.common_files)
        return mismatch

    def report(self):
        print('diff', self.left, self.right)
        if self.left_only:
            print('Only in', self.left, ':', self.left_only)
        if self.right_only:
            print('Only in', self.right, ':', self.right_only)
        if self.same_files:
            print('Identical files :', self.same_files)
        if self.diff_files:
            print('Differing files :', self.diff_files)


DEFAULT_IGNORES = [
    'RCS', 'CVS', 'tags', '.git', '.hg', '.bzr', '_darcs', '__pycache__',
]
