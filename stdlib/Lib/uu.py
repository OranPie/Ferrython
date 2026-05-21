"""Minimal uuencode/uudecode helpers."""


Error = Exception


_UU_CHARS = " !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_"


def _uu_char(value):
    return _UU_CHARS[value & 0x3f]


def _uu_encode_chunk(chunk):
    out = [_uu_char(len(chunk))]
    for i in range(0, len(chunk), 3):
        triple = chunk[i:i + 3]
        while len(triple) < 3:
            triple += b"\0"
        b1, b2, b3 = triple[0], triple[1], triple[2]
        out.append(_uu_char(b1 >> 2))
        out.append(_uu_char(((b1 << 4) | (b2 >> 4)) & 0x3f))
        out.append(_uu_char(((b2 << 2) | (b3 >> 6)) & 0x3f))
        out.append(_uu_char(b3 & 0x3f))
    return "".join(out) + "\n"


def _uu_value(ch):
    if isinstance(ch, str):
        ch = ord(ch)
    return (ch - 32) & 0x3f


def _uu_decode_line(line):
    if not isinstance(line, str):
        line = line.decode()
    if not line:
        return b""
    length = _uu_value(line[0])
    data = bytearray()
    body = line[1:].rstrip("\n")
    for i in range(0, len(body), 4):
        group = body[i:i + 4]
        if len(group) < 4:
            break
        a, b, c, d = [_uu_value(ch) for ch in group]
        data.append((a << 2) | (b >> 4))
        data.append(((b & 0xf) << 4) | (c >> 2))
        data.append(((c & 0x3) << 6) | d)
    return bytes(data[:length])


def _read_all(in_file):
    if hasattr(in_file, "read"):
        return in_file.read()
    with open(in_file, "rb") as f:
        return f.read()


def _write_all(out_file, data):
    if hasattr(out_file, "write"):
        out_file.write(data)
    else:
        with open(out_file, "wb") as f:
            f.write(data)


def encode(in_file, out_file, name=None, mode=None, backtick=False):
    data = _read_all(in_file)
    if not isinstance(data, bytes) and not isinstance(data, bytearray):
        data = data.encode()
    if name is None:
        name = "-"
    if mode is None:
        mode = 0o666
    lines = ["begin %o %s\n" % (mode, name)]
    for i in range(0, len(data), 45):
        lines.append(_uu_encode_chunk(data[i:i + 45]))
    lines.append(" \n")
    lines.append("end\n")
    encoded = "".join(lines)
    if hasattr(out_file, "write"):
        out_file.write(encoded)
    else:
        mode = "w"
        with open(out_file, mode) as f:
            f.write(encoded)


def decode(in_file, out_file=None, mode=None, quiet=False):
    text = _read_all(in_file)
    if not isinstance(text, str):
        text = text.decode()
    lines = text.splitlines()
    start = None
    for i, line in enumerate(lines):
        if line.startswith("begin "):
            start = i
            break
    if start is None:
        raise Error("No valid begin line found")
    data = bytearray()
    for line in lines[start + 1:]:
        if line == "end":
            break
        if not line:
            continue
        data.extend(_uu_decode_line(line))
    result = bytes(data)
    if out_file is None:
        out_file = lines[start].split(" ", 2)[2]
    _write_all(out_file, result)
