"""Small imghdr-compatible image type detector."""


tests = []


def what(file, h=None):
    """Return the image type detected from a filename or file object."""
    if h is None:
        if isinstance(file, (bytes, bytearray)):
            h = bytes(file)
        elif hasattr(file, "read"):
            pos = None
            try:
                pos = file.tell()
            except Exception:
                pass
            h = file.read(32)
            if pos is not None:
                try:
                    file.seek(pos)
                except Exception:
                    pass
        else:
            with open(file, "rb") as f:
                h = f.read(32)

    for test in tests:
        result = test(h, file)
        if result:
            return result
    return None


def test_jpeg(h, f):
    if h[:3] == b"\xff\xd8\xff" or h[6:10] in (b"JFIF", b"Exif"):
        return "jpeg"


def test_png(h, f):
    if h.startswith(b"\211PNG\r\n\032\n"):
        return "png"


def test_gif(h, f):
    if h[:6] in (b"GIF87a", b"GIF89a"):
        return "gif"


def test_tiff(h, f):
    if h[:2] in (b"MM", b"II"):
        return "tiff"


def test_rgb(h, f):
    if h.startswith(b"\001\332"):
        return "rgb"


def test_pbm(h, f):
    if len(h) >= 3 and h[0:1] == b"P" and h[1:2] in (b"1", b"4") and h[2:3] in b" \t\n\r":
        return "pbm"


def test_pgm(h, f):
    if len(h) >= 3 and h[0:1] == b"P" and h[1:2] in (b"2", b"5") and h[2:3] in b" \t\n\r":
        return "pgm"


def test_ppm(h, f):
    if len(h) >= 3 and h[0:1] == b"P" and h[1:2] in (b"3", b"6") and h[2:3] in b" \t\n\r":
        return "ppm"


tests.extend([test_jpeg, test_png, test_gif, test_tiff, test_rgb, test_pbm, test_pgm, test_ppm])
