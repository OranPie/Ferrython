"""Small XDR packer and unpacker compatible with common stdlib use."""


class Error(Exception):
    pass


class ConversionError(Error):
    pass


class Packer:
    def __init__(self):
        self.reset()

    def reset(self):
        self._buf = bytearray()

    def get_buffer(self):
        return bytes(self._buf)

    def pack_uint(self, x):
        self._buf.extend(int(x).to_bytes(4, "big", signed=False))

    pack_enum = pack_uint

    def pack_int(self, x):
        self._buf.extend(int(x).to_bytes(4, "big", signed=True))

    def pack_bool(self, x):
        self.pack_uint(1 if x else 0)

    def pack_uhyper(self, x):
        self._buf.extend(int(x).to_bytes(8, "big", signed=False))

    def pack_hyper(self, x):
        self._buf.extend(int(x).to_bytes(8, "big", signed=True))

    def pack_fstring(self, n, s):
        if not isinstance(s, bytes) and not isinstance(s, bytearray):
            s = s.encode()
        s = bytes(s)
        if len(s) != n:
            raise ValueError("fstring size mismatch")
        self._buf.extend(s)
        self._buf.extend(b"\0" * ((4 - n % 4) % 4))

    def pack_string(self, s):
        if not isinstance(s, bytes) and not isinstance(s, bytearray):
            s = s.encode()
        self.pack_uint(len(s))
        self.pack_fstring(len(s), s)

    pack_bytes = pack_string
    pack_opaque = pack_string

    def pack_list(self, items, pack_item):
        for item in items:
            self.pack_uint(1)
            pack_item(item)
        self.pack_uint(0)

    def pack_array(self, items, pack_item):
        self.pack_uint(len(items))
        for item in items:
            pack_item(item)


class Unpacker:
    def __init__(self, data):
        self.reset(data)

    def reset(self, data):
        self._buf = bytes(data)
        self._pos = 0

    def get_position(self):
        return self._pos

    def set_position(self, pos):
        self._pos = pos

    def done(self):
        if self._pos != len(self._buf):
            raise Error("unextracted data remains")

    def _read(self, n):
        if self._pos + n > len(self._buf):
            raise EOFError
        data = self._buf[self._pos:self._pos + n]
        self._pos += n
        return data

    def unpack_uint(self):
        return int.from_bytes(self._read(4), "big", signed=False)

    unpack_enum = unpack_uint

    def unpack_int(self):
        return int.from_bytes(self._read(4), "big", signed=True)

    def unpack_bool(self):
        return bool(self.unpack_uint())

    def unpack_uhyper(self):
        return int.from_bytes(self._read(8), "big", signed=False)

    def unpack_hyper(self):
        return int.from_bytes(self._read(8), "big", signed=True)

    def unpack_fstring(self, n):
        data = self._read(n)
        pad = (4 - n % 4) % 4
        if pad:
            self._read(pad)
        return data

    def unpack_string(self):
        n = self.unpack_uint()
        return self.unpack_fstring(n)

    unpack_bytes = unpack_string
    unpack_opaque = unpack_string

    def unpack_list(self, unpack_item):
        items = []
        while self.unpack_uint():
            items.append(unpack_item())
        return items

    def unpack_array(self, unpack_item):
        return [unpack_item() for _ in range(self.unpack_uint())]
