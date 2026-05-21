"""Read IFF-style chunked binary files."""


class Chunk:
    def __init__(self, file, align=True, bigendian=True, inclheader=False):
        self.file = file
        self.align = align
        self.closed = False
        self.size_read = 0
        self.chunkname = file.read(4)
        if len(self.chunkname) < 4:
            raise EOFError
        raw = file.read(4)
        if len(raw) < 4:
            raise EOFError
        order = "big" if bigendian else "little"
        self.chunksize = int.from_bytes(raw, order)
        if inclheader:
            self.chunksize = self.chunksize - 8
        try:
            self.offset = file.tell()
        except Exception:
            self.offset = None

    def getname(self):
        return self.chunkname

    def getsize(self):
        return self.chunksize

    def read(self, size=-1):
        if self.closed:
            raise ValueError("I/O operation on closed file")
        remaining = self.chunksize - self.size_read
        if size is None or size < 0 or size > remaining:
            size = remaining
        data = self.file.read(size)
        self.size_read += len(data)
        if self.size_read == self.chunksize and self.align and self.chunksize % 2:
            self.file.read(1)
        return data

    def skip(self):
        if self.closed:
            raise ValueError("I/O operation on closed file")
        remaining = self.chunksize - self.size_read
        if remaining > 0:
            try:
                self.file.seek(remaining, 1)
            except Exception:
                self.file.read(remaining)
            self.size_read = self.chunksize
        if self.align and self.chunksize % 2:
            self.file.read(1)

    def close(self):
        if not self.closed:
            self.skip()
            self.closed = True

    def seek(self, pos, whence=0):
        if self.offset is None:
            raise OSError("cannot seek")
        if whence == 0:
            target = pos
        elif whence == 1:
            target = self.size_read + pos
        elif whence == 2:
            target = self.chunksize + pos
        else:
            raise ValueError("invalid whence")
        if target < 0 or target > self.chunksize:
            raise RuntimeError("seek out of range")
        self.file.seek(self.offset + target)
        self.size_read = target

    def tell(self):
        return self.size_read
