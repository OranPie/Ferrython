import io

import chunk
import imghdr
import nturl2path
import sndhdr
import uu
import xdrlib


assert nturl2path.pathname2url("C:\\Temp\\hello world.txt") == "/C:/Temp/hello%20world.txt"
assert nturl2path.url2pathname("/C:/Temp/hello%20world.txt") == "C:\\Temp\\hello world.txt"

assert imghdr.what(None, b"\x89PNG\r\n\x1a\nrest") == "png"
assert imghdr.what(None, b"GIF89a...") == "gif"
assert imghdr.what(None, b"not an image") is None

assert sndhdr.whathdr(None, b"RIFF\x00\x00\x00\x00WAVEfmt ")[:1] == ("wav",)
assert sndhdr.whathdr(None, b"not sound") is None

data = io.BytesIO(b"TEST\x00\x00\x00\x03abc\x00NEXT\x00\x00\x00\x01z\x00")
c = chunk.Chunk(data)
assert c.getname() == b"TEST"
assert c.getsize() == 3
assert c.read() == b"abc"
c2 = chunk.Chunk(data)
assert c2.getname() == b"NEXT"
assert c2.read() == b"z"

encoded = io.StringIO()
uu.encode(io.BytesIO(b"hello"), encoded, "hello.txt")
decoded = io.BytesIO()
uu.decode(io.StringIO(encoded.getvalue()), decoded)
assert decoded.getvalue() == b"hello"

p = xdrlib.Packer()
p.pack_uint(7)
p.pack_int(-3)
p.pack_string(b"abc")
u = xdrlib.Unpacker(p.get_buffer())
assert u.unpack_uint() == 7
assert u.unpack_int() == -3
assert u.unpack_string() == b"abc"
u.done()

print("missing stdlibs: ok")
