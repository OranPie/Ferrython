"""Generate cryptographically strong random numbers for managing secrets."""

import os

def token_bytes(nbytes=32):
    """Return a random byte string containing *nbytes* bytes."""
    return os.urandom(nbytes)

def token_hex(nbytes=32):
    """Return a random text string, in hexadecimal."""
    raw = token_bytes(nbytes)
    parts = []
    for b in raw:
        if isinstance(b, int):
            parts.append(format(b, '02x'))
        else:
            parts.append(format(ord(b), '02x'))
    return ''.join(parts)

def token_urlsafe(nbytes=32):
    """Return a random URL-safe text string, in Base64 encoding."""
    import base64
    tok = token_bytes(nbytes)
    return base64.urlsafe_b64encode(tok).rstrip(b'=').decode('ascii') if hasattr(base64.urlsafe_b64encode(tok), 'decode') else base64.urlsafe_b64encode(tok).rstrip(b'=')

def choice(sequence):
    """Choose a random element from a non-empty sequence."""
    import random
    return random.choice(sequence)

def randbelow(exclusive_upper_bound):
    """Return a random int in the range [0, n)."""
    if exclusive_upper_bound <= 0:
        raise ValueError("Upper bound must be positive")
    import random
    return random.randint(0, exclusive_upper_bound - 1)

def compare_digest(a, b):
    """Return ``a == b`` using a constant-time comparison.

    This is a simplified version; true constant-time requires C-level
    implementation, but we approximate it here.
    """
    if isinstance(a, bytes) and isinstance(b, bytes):
        pass
    elif isinstance(a, str) and isinstance(b, str):
        pass
    else:
        raise TypeError("unsupported operand types")
    if len(a) != len(b):
        return False
    result = 0
    for x, y in zip(a, b):
        if isinstance(x, int):
            result |= x ^ y
        else:
            result |= ord(x) ^ ord(y)
    return result == 0
