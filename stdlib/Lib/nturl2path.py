"""Utilities to convert between DOS paths and file URLs."""

import urllib.parse


def url2pathname(url):
    """Convert a URL path to a DOS path."""
    url = urllib.parse.unquote(url)
    if url.startswith("///"):
        url = url[2:]
    elif url.startswith("//"):
        parts = url[2:].split("/", 1)
        host = parts[0]
        rest = parts[1] if len(parts) > 1 else ""
        return "\\\\" + host + "\\" + rest.replace("/", "\\")

    if len(url) >= 3 and url[0] == "/" and url[2] in "|:":
        url = url[1] + ":" + url[3:]
    return url.replace("/", "\\")


def pathname2url(path):
    """Convert a DOS path to a URL path."""
    path = path.replace("\\", "/")
    if path.startswith("//"):
        return "//" + urllib.parse.quote(path[2:])
    if len(path) >= 2 and path[1] == ":":
        return "/" + path[0] + ":" + urllib.parse.quote(path[2:])
    return urllib.parse.quote(path)
