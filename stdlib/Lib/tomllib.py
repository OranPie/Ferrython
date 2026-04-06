"""tomllib — Parse TOML files.

Simplified implementation for Ferrython.
Supports basic TOML parsing: strings, integers, floats, booleans,
arrays, tables, and inline tables.
"""

__all__ = ["loads", "load", "TOMLDecodeError"]


class TOMLDecodeError(ValueError):
    """Error raised when TOML cannot be decoded."""
    pass


def load(fp):
    """Read a TOML file and return a dict."""
    return loads(fp.read())


def loads(s):
    """Parse a TOML string and return a dict."""
    return _TOMLParser(s).parse()


class _TOMLParser:
    def __init__(self, text):
        self.text = text
        self.pos = 0
        self.result = {}

    def parse(self):
        current = self.result
        current_path = []
        for line in self.text.split("\n"):
            stripped = line.strip()
            if not stripped or stripped.startswith("#"):
                continue

            # Table header [section] or [[array]]
            if stripped.startswith("[[") and stripped.endswith("]]"):
                key = stripped[2:-2].strip()
                parts = self._split_key(key)
                target = self.result
                for part in parts[:-1]:
                    if part not in target:
                        target[part] = {}
                    target = target[part]
                last = parts[-1]
                if last not in target:
                    target[last] = []
                if not isinstance(target[last], list):
                    target[last] = [target[last]]
                new_table = {}
                target[last].append(new_table)
                current = new_table
                current_path = parts
                continue

            if stripped.startswith("[") and stripped.endswith("]"):
                key = stripped[1:-1].strip()
                parts = self._split_key(key)
                current = self.result
                for part in parts:
                    if part not in current:
                        current[part] = {}
                    current = current[part]
                current_path = parts
                continue

            # Key = value
            if "=" in stripped:
                eq_pos = stripped.index("=")
                key = stripped[:eq_pos].strip().strip('"').strip("'")
                val_str = stripped[eq_pos + 1:].strip()
                # Remove inline comments
                val_str = self._remove_comment(val_str)
                current[key] = self._parse_value(val_str)

        return self.result

    def _split_key(self, key):
        parts = []
        current = ""
        in_quotes = False
        quote_char = None
        for ch in key:
            if in_quotes:
                if ch == quote_char:
                    in_quotes = False
                else:
                    current += ch
            elif ch in ('"', "'"):
                in_quotes = True
                quote_char = ch
            elif ch == ".":
                parts.append(current.strip())
                current = ""
            else:
                current += ch
        if current.strip():
            parts.append(current.strip())
        return parts

    def _remove_comment(self, s):
        in_str = False
        quote_ch = None
        for i, ch in enumerate(s):
            if in_str:
                if ch == quote_ch:
                    in_str = False
            elif ch in ('"', "'"):
                in_str = True
                quote_ch = ch
            elif ch == "#":
                return s[:i].rstrip()
        return s

    def _parse_value(self, s):
        s = s.strip()
        if not s:
            return ""

        # Multi-line strings (basic)
        if s.startswith('"""') or s.startswith("'''"):
            quote = s[:3]
            end = s.find(quote, 3)
            if end >= 0:
                return s[3:end]
            return s[3:]

        # Strings
        if (s.startswith('"') and s.endswith('"')) or \
           (s.startswith("'") and s.endswith("'")):
            inner = s[1:-1]
            if s[0] == '"':
                inner = inner.replace("\\n", "\n").replace("\\t", "\t")
                inner = inner.replace('\\"', '"').replace("\\\\", "\\")
            return inner

        # Booleans
        if s == "true":
            return True
        if s == "false":
            return False

        # Integers (hex, oct, bin)
        if s.startswith("0x") or s.startswith("0X"):
            return int(s, 16)
        if s.startswith("0o") or s.startswith("0O"):
            return int(s, 8)
        if s.startswith("0b") or s.startswith("0B"):
            return int(s, 2)

        # Float special values
        if s in ("inf", "+inf"):
            return float("inf")
        if s == "-inf":
            return float("-inf")
        if s in ("nan", "+nan", "-nan"):
            return float("nan")

        # Arrays
        if s.startswith("[") and s.endswith("]"):
            return self._parse_array(s)

        # Inline tables
        if s.startswith("{") and s.endswith("}"):
            return self._parse_inline_table(s)

        # Numbers
        clean = s.replace("_", "")
        try:
            if "." in clean or "e" in clean or "E" in clean:
                return float(clean)
            return int(clean)
        except (ValueError, OverflowError):
            pass

        return s

    def _parse_array(self, s):
        inner = s[1:-1].strip()
        if not inner:
            return []
        items = self._split_array(inner)
        return [self._parse_value(item.strip()) for item in items if item.strip()]

    def _split_array(self, s):
        items = []
        depth = 0
        current = ""
        in_str = False
        quote_ch = None
        for ch in s:
            if in_str:
                current += ch
                if ch == quote_ch:
                    in_str = False
            elif ch in ('"', "'"):
                in_str = True
                quote_ch = ch
                current += ch
            elif ch in ("[", "{"):
                depth += 1
                current += ch
            elif ch in ("]", "}"):
                depth -= 1
                current += ch
            elif ch == "," and depth == 0:
                items.append(current)
                current = ""
            else:
                current += ch
        if current.strip():
            items.append(current)
        return items

    def _parse_inline_table(self, s):
        inner = s[1:-1].strip()
        if not inner:
            return {}
        result = {}
        for pair in self._split_array(inner):
            pair = pair.strip()
            if "=" in pair:
                eq = pair.index("=")
                key = pair[:eq].strip().strip('"').strip("'")
                val = pair[eq + 1:].strip()
                result[key] = self._parse_value(val)
        return result
