"""Fraction class implementing exact rational arithmetic.

Note: If a Rust-native fractions module is loaded instead of this one,
that's fine - both provide the same interface.
"""

__all__ = ['Fraction']


def _gcd(a, b):
    """Compute the greatest common divisor of a and b."""
    a = abs(a)
    b = abs(b)
    while b:
        a, b = b, a % b
    return a


def gcd(a, b):
    """Public gcd function."""
    return _gcd(a, b)


class Fraction:
    """Represents a rational number as numerator/denominator.

    Fraction(3, 4) -> 3/4
    Fraction('3/4') -> 3/4
    Fraction(0.75) -> 3/4
    """

    __slots__ = ('_numerator', '_denominator')

    def __init__(self, numerator=0, denominator=None):
        if denominator is None:
            if isinstance(numerator, int):
                self._numerator = numerator
                self._denominator = 1
                return
            if isinstance(numerator, float):
                self._from_float_init(numerator)
                return
            if isinstance(numerator, str):
                self._from_string_init(numerator)
                return
            if isinstance(numerator, Fraction):
                self._numerator = numerator._numerator
                self._denominator = numerator._denominator
                return
            raise TypeError("argument should be a string or a Rational instance")
        else:
            if not isinstance(numerator, int) or not isinstance(denominator, int):
                raise TypeError("both arguments should be Rational instances")
            if denominator == 0:
                raise ZeroDivisionError("Fraction(%s, 0)" % numerator)
            g = _gcd(numerator, denominator)
            if denominator < 0:
                numerator = -numerator
                denominator = -denominator
            self._numerator = numerator // g
            self._denominator = denominator // g

    def _from_float_init(self, f):
        """Initialize from a float value."""
        if f != f:  # NaN check
            raise ValueError("Cannot convert NaN to Fraction")
        if f == float('inf') or f == float('-inf'):
            raise OverflowError("Cannot convert infinity to Fraction")
        # Use a simple float-to-fraction via string representation
        s = repr(f)
        self._from_string_init(s)

    def _from_string_init(self, s):
        """Initialize from a string."""
        s = s.strip()
        if '/' in s:
            num_str, den_str = s.split('/', 1)
            num = int(num_str.strip())
            den = int(den_str.strip())
            if den == 0:
                raise ZeroDivisionError("Fraction(%s)" % s)
            g = _gcd(num, den)
            if den < 0:
                num = -num
                den = -den
            self._numerator = num // g
            self._denominator = den // g
        elif '.' in s:
            # Decimal string like '0.75'
            neg = False
            if s.startswith('-'):
                neg = True
                s = s[1:]
            if 'e' in s or 'E' in s:
                # Scientific notation
                f = float(('-' if neg else '') + s)
                self._from_decimal_string(f)
                return
            parts = s.split('.')
            integer_part = parts[0] if parts[0] else '0'
            decimal_part = parts[1] if len(parts) > 1 else '0'
            num = int(integer_part + decimal_part)
            den = 10 ** len(decimal_part)
            if neg:
                num = -num
            g = _gcd(num, den)
            self._numerator = num // g
            self._denominator = den // g
        else:
            self._numerator = int(s)
            self._denominator = 1

    def _from_decimal_string(self, f):
        """Convert float to fraction using continued fraction algorithm."""
        if f < 0:
            neg = True
            f = -f
        else:
            neg = False
        # Limit precision - use multiplication approach
        # Multiply by powers of 2 to get integer ratio
        num, den = f.as_integer_ratio() if hasattr(f, 'as_integer_ratio') else _float_to_ratio(f)
        if neg:
            num = -num
        g = _gcd(num, den)
        self._numerator = num // g
        self._denominator = den // g

    @classmethod
    def from_float(cls, f):
        """Convert a finite float to a Fraction."""
        if not isinstance(f, (int, float)):
            raise TypeError("%s is not a float or int" % type(f).__name__)
        if isinstance(f, int):
            return cls(f)
        return cls(f)

    @classmethod
    def from_decimal(cls, dec):
        """Convert a Decimal to a Fraction."""
        return cls(str(dec))

    @property
    def numerator(self):
        return self._numerator

    @property
    def denominator(self):
        return self._denominator

    def _add(self, other):
        num = self._numerator * other._denominator + other._numerator * self._denominator
        den = self._denominator * other._denominator
        return Fraction(num, den)

    def _sub(self, other):
        num = self._numerator * other._denominator - other._numerator * self._denominator
        den = self._denominator * other._denominator
        return Fraction(num, den)

    def _mul(self, other):
        num = self._numerator * other._numerator
        den = self._denominator * other._denominator
        return Fraction(num, den)

    def _truediv(self, other):
        if other._numerator == 0:
            raise ZeroDivisionError("Fraction division by zero")
        num = self._numerator * other._denominator
        den = self._denominator * other._numerator
        return Fraction(num, den)

    def _to_fraction(self, other):
        """Convert other to a Fraction if possible."""
        if isinstance(other, Fraction):
            return other
        if isinstance(other, int):
            return Fraction(other, 1)
        if isinstance(other, float):
            return Fraction(other)
        return NotImplemented

    def __add__(self, other):
        other = self._to_fraction(other)
        if other is NotImplemented:
            return NotImplemented
        return self._add(other)

    def __radd__(self, other):
        other = self._to_fraction(other)
        if other is NotImplemented:
            return NotImplemented
        return other._add(self)

    def __sub__(self, other):
        other = self._to_fraction(other)
        if other is NotImplemented:
            return NotImplemented
        return self._sub(other)

    def __rsub__(self, other):
        other = self._to_fraction(other)
        if other is NotImplemented:
            return NotImplemented
        return other._sub(self)

    def __mul__(self, other):
        other = self._to_fraction(other)
        if other is NotImplemented:
            return NotImplemented
        return self._mul(other)

    def __rmul__(self, other):
        other = self._to_fraction(other)
        if other is NotImplemented:
            return NotImplemented
        return other._mul(self)

    def __truediv__(self, other):
        other = self._to_fraction(other)
        if other is NotImplemented:
            return NotImplemented
        return self._truediv(other)

    def __rtruediv__(self, other):
        other = self._to_fraction(other)
        if other is NotImplemented:
            return NotImplemented
        return other._truediv(self)

    def __floordiv__(self, other):
        other = self._to_fraction(other)
        if other is NotImplemented:
            return NotImplemented
        return (self._numerator * other._denominator) // (self._denominator * other._numerator)

    def __rfloordiv__(self, other):
        other = self._to_fraction(other)
        if other is NotImplemented:
            return NotImplemented
        return (other._numerator * self._denominator) // (other._denominator * self._numerator)

    def __mod__(self, other):
        other = self._to_fraction(other)
        if other is NotImplemented:
            return NotImplemented
        div = self.__floordiv__(other)
        return self - other * div

    def __pow__(self, other):
        if isinstance(other, int):
            if other >= 0:
                return Fraction(self._numerator ** other, self._denominator ** other)
            else:
                if self._numerator == 0:
                    raise ZeroDivisionError("Fraction(0, 1) cannot be raised to a negative power")
                return Fraction(self._denominator ** (-other), self._numerator ** (-other))
        return NotImplemented

    def __neg__(self):
        return Fraction(-self._numerator, self._denominator)

    def __pos__(self):
        return Fraction(self._numerator, self._denominator)

    def __abs__(self):
        return Fraction(abs(self._numerator), self._denominator)

    def __float__(self):
        return self._numerator / self._denominator

    def __int__(self):
        if self._numerator < 0:
            return -(-self._numerator // self._denominator)
        return self._numerator // self._denominator

    def __bool__(self):
        return self._numerator != 0

    def _compare(self, other):
        """Return -1, 0, or 1 for comparison."""
        other = self._to_fraction(other)
        if other is NotImplemented:
            return NotImplemented
        left = self._numerator * other._denominator
        right = other._numerator * self._denominator
        if left < right:
            return -1
        elif left == right:
            return 0
        else:
            return 1

    def __eq__(self, other):
        c = self._compare(other)
        if c is NotImplemented:
            return NotImplemented
        return c == 0

    def __ne__(self, other):
        c = self._compare(other)
        if c is NotImplemented:
            return NotImplemented
        return c != 0

    def __lt__(self, other):
        c = self._compare(other)
        if c is NotImplemented:
            return NotImplemented
        return c < 0

    def __le__(self, other):
        c = self._compare(other)
        if c is NotImplemented:
            return NotImplemented
        return c <= 0

    def __gt__(self, other):
        c = self._compare(other)
        if c is NotImplemented:
            return NotImplemented
        return c > 0

    def __ge__(self, other):
        c = self._compare(other)
        if c is NotImplemented:
            return NotImplemented
        return c >= 0

    def __hash__(self):
        if self._denominator == 1:
            return hash(self._numerator)
        return hash((self._numerator, self._denominator))

    def __repr__(self):
        if self._denominator == 1:
            return "Fraction(%d, 1)" % self._numerator
        return "Fraction(%d, %d)" % (self._numerator, self._denominator)

    def __str__(self):
        if self._denominator == 1:
            return str(self._numerator)
        return "%d/%d" % (self._numerator, self._denominator)

    def limit_denominator(self, max_denominator=10**6):
        """Closest Fraction to self with denominator at most max_denominator.

        Uses the continued fraction algorithm."""
        if max_denominator < 1:
            raise ValueError("max_denominator should be at least 1")
        if self._denominator <= max_denominator:
            return Fraction(self._numerator, self._denominator)

        p0, q0 = 0, 1
        p1, q1 = 1, 0
        n, d = self._numerator, self._denominator
        while True:
            a = n // d
            q2 = q0 + a * q1
            if q2 > max_denominator:
                break
            p0, q0, p1, q1 = p1, q1, p0 + a * p1, q2
            n, d = d, n - a * d

        k = (max_denominator - q0) // q1
        bound1 = Fraction(p0 + k * p1, q0 + k * q1)
        bound2 = Fraction(p1, q1)

        if abs(bound2 - self) <= abs(bound1 - self):
            return bound2
        else:
            return bound1


def _float_to_ratio(f):
    """Fallback float to integer ratio conversion."""
    if f == 0.0:
        return (0, 1)
    sign = 1 if f > 0 else -1
    f = abs(f)
    # Multiply to get integers
    den = 1
    while f != int(f) and den < 10**15:
        f = f * 2
        den = den * 2
    num = int(f) * sign
    g = _gcd(num, den)
    return (num // g, den // g)
