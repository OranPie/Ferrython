"""UUID objects (universally unique identifiers) according to RFC 4122."""

import os
import hashlib


class UUID:
    """Instances of the UUID class represent UUIDs."""

    def __init__(self, hex=None, bytes=None, int=None, version=None):
        if hex is not None:
            hex = hex.replace('-', '').replace('{', '').replace('}', '').strip()
            if len(hex) != 32:
                raise ValueError("badly formed hexadecimal UUID string")
            self._int = builtins_int(hex, 16)
        elif bytes is not None:
            if len(bytes) != 16:
                raise ValueError("bytes is not a 16-char string")
            val = 0
            for b in bytes:
                if isinstance(b, builtins_int):
                    val = (val << 8) | b
                else:
                    val = (val << 8) | ord(b)
            self._int = val
        elif int is not None:
            self._int = int
        else:
            raise TypeError("one of hex, bytes, or int must be given")

        if version is not None:
            # set variant to RFC 4122
            self._int = (self._int & ~(0xc000 << 48)) | (0x8000 << 48)
            # set version
            self._int = (self._int & ~(0xf000 << 64)) | (version << 76)

    @property
    def hex(self):
        return format(self._int, '032x')

    @property
    def int(self):
        return self._int

    @property
    def bytes(self):
        result = []
        val = self._int
        for _ in range(16):
            result.append(val & 0xff)
            val >>= 8
        result.reverse()
        return builtins_bytes(result)

    @property
    def version(self):
        return (self._int >> 76) & 0xf

    @property
    def variant(self):
        return "RFC_4122"

    def __str__(self):
        h = self.hex
        return f"{h[:8]}-{h[8:12]}-{h[12:16]}-{h[16:20]}-{h[20:]}"

    def __repr__(self):
        return f"UUID('{self}')"

    def __eq__(self, other):
        if isinstance(other, UUID):
            return self._int == other._int
        return False

    def __hash__(self):
        return hash(self._int)


# Stash built-in names to avoid shadowing
builtins_int = __builtins__['int'] if isinstance(__builtins__, dict) else int
builtins_bytes = __builtins__['bytes'] if isinstance(__builtins__, dict) else bytes


def uuid4():
    """Generate a random UUID."""
    raw = os.urandom(16)
    vals = []
    for b in raw:
        if isinstance(b, builtins_int):
            vals.append(b)
        else:
            vals.append(ord(b))
    # Set version 4
    vals[6] = (vals[6] & 0x0f) | 0x40
    # Set variant RFC 4122
    vals[8] = (vals[8] & 0x3f) | 0x80
    int_val = 0
    for v in vals:
        int_val = (int_val << 8) | v
    return UUID(int=int_val)


def uuid1(node=None, clock_seq=None):
    """Generate a UUID from a host ID, sequence number, and the current time.

    Simplified: falls back to uuid4() since we lack MAC address access.
    """
    return uuid4()


def _name_based_uuid(namespace, name, hash_func, version):
    """Generate a name-based UUID."""
    ns_bytes = namespace.bytes
    name_bytes = name.encode('utf-8') if isinstance(name, str) else name
    combined = []
    for b in ns_bytes:
        if isinstance(b, builtins_int):
            combined.append(b)
        else:
            combined.append(ord(b))
    for b in name_bytes:
        if isinstance(b, builtins_int):
            combined.append(b)
        else:
            combined.append(ord(b))
    h = hash_func(builtins_bytes(combined))
    digest = h.hexdigest()
    # Take first 32 hex chars (16 bytes)
    int_val = builtins_int(digest[:32], 16)
    # Set version
    int_val = (int_val & ~(0xf << 76)) | (version << 76)
    # Set variant
    int_val = (int_val & ~(0xc000 << 48)) | (0x8000 << 48)
    return UUID(int=int_val)


def uuid3(namespace, name):
    """Generate a UUID from the MD5 hash of a namespace UUID and a name."""
    return _name_based_uuid(namespace, name, hashlib.md5, 3)


def uuid5(namespace, name):
    """Generate a UUID from the SHA-1 hash of a namespace UUID and a name."""
    return _name_based_uuid(namespace, name, hashlib.sha1, 5)


# Well-known namespace UUIDs
NAMESPACE_DNS = UUID(hex='6ba7b8109dad11d180b400c04fd430c8')
NAMESPACE_URL = UUID(hex='6ba7b8119dad11d180b400c04fd430c8')
NAMESPACE_OID = UUID(hex='6ba7b8129dad11d180b400c04fd430c8')
NAMESPACE_X500 = UUID(hex='6ba7b8149dad11d180b400c04fd430c8')
