"""Small sndhdr-compatible sound type detector."""

SndHeaders = tuple


def what(filename):
    if hasattr(filename, "read"):
        pos = None
        try:
            pos = filename.tell()
        except Exception:
            pass
        h = filename.read(512)
        if pos is not None:
            try:
                filename.seek(pos)
            except Exception:
                pass
    else:
        with open(filename, "rb") as f:
            h = f.read(512)
    return whathdr(filename, h)


def whathdr(filename, h):
    if h.startswith(b"RIFF") and h[8:12] == b"WAVE":
        return ("wav", None, None, None, None)
    if h.startswith(b"FORM") and h[8:12] in (b"AIFF", b"AIFC"):
        return ("aiff", None, None, None, None)
    if h.startswith(b".snd"):
        return ("au", None, None, None, None)
    if h.startswith(b"ID3") or (len(h) > 2 and h[0] == 255 and (h[1] & 224) == 224):
        return ("mp3", None, None, None, None)
    return None
