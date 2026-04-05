"""Basic Decimal arithmetic module.

This module provides a Decimal class for decimal floating-point arithmetic.
Named decimal_ to avoid collision with the Rust-native 'decimal' module.

Usage:
    from decimal_ import Decimal
"""

__all__ = ['Decimal', 'InvalidOperation', 'DivisionByZero']


class InvalidOperation(ArithmeticError):
    """Invalid operation on Decimal."""
    pass


class DivisionByZero(ZeroDivisionError):
    """Division by zero in Decimal."""
    pass


class Decimal:
    """Decimal fixed-point and floating-point arithmetic.

    Stores numbers as (sign, coefficient, exponent) where
    value = (-1)**sign * coefficient * 10**exponent

    Decimal('3.14') -> Decimal('3.14')
    Decimal(42) -> Decimal('42')
    Decimal('1.23E+4') -> Decimal('1.23E+4')
    """

    __slots__ = ('_sign', '_coeff', '_exp', '_is_special')

    def __init__(self, value=0):
        self._is_special = False
        if isinstance(value, Decimal):
            self._sign = value._sign
            self._coeff = value._coeff
            self._exp = value._exp
            self._is_special = value._is_special
        elif isinstance(value, int):
            self._sign = 1 if value < 0 else 0
            self._coeff = abs(value)
            self._exp = 0
        elif isinstance(value, float):
            self._from_float(value)
        elif isinstance(value, str):
            self._from_string(value)
        else:
            raise TypeError("Cannot convert %s to Decimal" % type(value).__name__)

    def _from_float(self, f):
        """Initialize from a float."""
        if f != f:
            self._sign = 0
            self._coeff = 0
            self._exp = 0
            self._is_special = True
            return
        if f == float('inf'):
            self._sign = 0
            self._coeff = 0
            self._exp = 0
            self._is_special = True
            return
        if f == float('-inf'):
            self._sign = 1
            self._coeff = 0
            self._exp = 0
            self._is_special = True
            return
        self._from_string(repr(f))

    def _from_string(self, s):
        """Parse a decimal string."""
        s = s.strip()
        if not s:
            raise InvalidOperation("Invalid string for Decimal: empty")

        # Handle sign
        if s[0] == '-':
            self._sign = 1
            s = s[1:]
        elif s[0] == '+':
            self._sign = 0
            s = s[1:]
        else:
            self._sign = 0

        # Handle special values
        s_lower = s.lower()
        if s_lower in ('inf', 'infinity'):
            self._coeff = 0
            self._exp = 0
            self._is_special = True
            return
        if s_lower == 'nan':
            self._coeff = 0
            self._exp = 0
            self._is_special = True
            return

        # Handle scientific notation
        if 'e' in s_lower:
            parts = s_lower.split('e')
            mantissa = parts[0]
            exp_part = int(parts[1])
        else:
            mantissa = s
            exp_part = 0

        # Parse mantissa
        if '.' in mantissa:
            int_part, frac_part = mantissa.split('.', 1)
            if not int_part:
                int_part = '0'
            coeff_str = int_part + frac_part
            exp_part -= len(frac_part)
        else:
            coeff_str = mantissa

        # Remove leading zeros but keep at least one digit
        coeff_str = coeff_str.lstrip('0') or '0'
        self._coeff = int(coeff_str)
        self._exp = exp_part

    def _value(self):
        """Return the numeric value as an integer tuple (numerator, denominator power of 10)."""
        if self._is_special:
            return None
        return self._coeff, self._exp

    def _as_tuple(self):
        """Return (sign, coefficient, exponent)."""
        return (self._sign, self._coeff, self._exp)

    def _normalize_pair(self, other):
        """Align two Decimals to the same exponent."""
        if self._exp == other._exp:
            return self._coeff, other._coeff, self._exp
        if self._exp < other._exp:
            diff = other._exp - self._exp
            return self._coeff, other._coeff * (10 ** diff), self._exp
        else:
            diff = self._exp - other._exp
            return self._coeff * (10 ** diff), other._coeff, other._exp

    def _to_decimal(self, other):
        """Convert other to Decimal."""
        if isinstance(other, Decimal):
            return other
        if isinstance(other, int):
            return Decimal(other)
        if isinstance(other, float):
            return Decimal(other)
        return NotImplemented

    def _signed_coeff(self):
        """Return coefficient with sign applied."""
        return -self._coeff if self._sign else self._coeff

    def __add__(self, other):
        other = self._to_decimal(other)
        if other is NotImplemented:
            return NotImplemented
        a_coeff, b_coeff, exp = self._normalize_pair(other)
        a_val = -a_coeff if self._sign else a_coeff
        b_val = -b_coeff if other._sign else b_coeff
        result = a_val + b_val
        d = Decimal(0)
        d._sign = 1 if result < 0 else 0
        d._coeff = abs(result)
        d._exp = exp
        return d

    def __radd__(self, other):
        other = self._to_decimal(other)
        if other is NotImplemented:
            return NotImplemented
        return other.__add__(self)

    def __sub__(self, other):
        other = self._to_decimal(other)
        if other is NotImplemented:
            return NotImplemented
        neg_other = Decimal(0)
        neg_other._sign = 1 - other._sign
        neg_other._coeff = other._coeff
        neg_other._exp = other._exp
        return self.__add__(neg_other)

    def __rsub__(self, other):
        other = self._to_decimal(other)
        if other is NotImplemented:
            return NotImplemented
        return other.__sub__(self)

    def __mul__(self, other):
        other = self._to_decimal(other)
        if other is NotImplemented:
            return NotImplemented
        d = Decimal(0)
        d._sign = self._sign ^ other._sign
        d._coeff = self._coeff * other._coeff
        d._exp = self._exp + other._exp
        return d

    def __rmul__(self, other):
        other = self._to_decimal(other)
        if other is NotImplemented:
            return NotImplemented
        return other.__mul__(self)

    def __truediv__(self, other):
        other = self._to_decimal(other)
        if other is NotImplemented:
            return NotImplemented
        if other._coeff == 0:
            raise DivisionByZero("Division by zero")
        # Scale up for precision (28 significant digits like CPython default)
        precision = 28
        scale = 10 ** precision
        a = self._coeff * scale
        q = a // other._coeff
        d = Decimal(0)
        d._sign = self._sign ^ other._sign
        d._coeff = q
        d._exp = self._exp - other._exp - precision
        # Trim trailing zeros
        while d._coeff and d._coeff % 10 == 0 and d._exp < 0:
            d._coeff = d._coeff // 10
            d._exp = d._exp + 1
        return d

    def __rtruediv__(self, other):
        other = self._to_decimal(other)
        if other is NotImplemented:
            return NotImplemented
        return other.__truediv__(self)

    def __floordiv__(self, other):
        other = self._to_decimal(other)
        if other is NotImplemented:
            return NotImplemented
        if other._coeff == 0:
            raise DivisionByZero("Division by zero")
        a_coeff, b_coeff, exp = self._normalize_pair(other)
        a_val = -a_coeff if self._sign else a_coeff
        b_val = -b_coeff if other._sign else b_coeff
        result = a_val // b_val
        return Decimal(result)

    def __mod__(self, other):
        other = self._to_decimal(other)
        if other is NotImplemented:
            return NotImplemented
        q = self.__floordiv__(other)
        return self - other * q

    def __pow__(self, other):
        if isinstance(other, int):
            if other == 0:
                return Decimal(1)
            if other < 0:
                return Decimal(1) / (self ** (-other))
            result = Decimal(1)
            base = Decimal(self)
            while other > 0:
                if other % 2 == 1:
                    result = result * base
                base = base * base
                other = other // 2
            return result
        return NotImplemented

    def __neg__(self):
        d = Decimal(0)
        d._sign = 1 - self._sign
        d._coeff = self._coeff
        d._exp = self._exp
        return d

    def __pos__(self):
        return Decimal(self)

    def __abs__(self):
        d = Decimal(0)
        d._sign = 0
        d._coeff = self._coeff
        d._exp = self._exp
        return d

    def _cmp_value(self):
        """Return a comparable float value."""
        val = self._coeff * (10.0 ** self._exp)
        return -val if self._sign else val

    def __eq__(self, other):
        other = self._to_decimal(other)
        if other is NotImplemented:
            return NotImplemented
        if self._sign != other._sign and (self._coeff != 0 or other._coeff != 0):
            return False
        a_coeff, b_coeff, _ = self._normalize_pair(other)
        return a_coeff == b_coeff

    def __ne__(self, other):
        eq = self.__eq__(other)
        if eq is NotImplemented:
            return NotImplemented
        return not eq

    def __lt__(self, other):
        other = self._to_decimal(other)
        if other is NotImplemented:
            return NotImplemented
        return self._cmp_value() < other._cmp_value()

    def __le__(self, other):
        other = self._to_decimal(other)
        if other is NotImplemented:
            return NotImplemented
        return self._cmp_value() <= other._cmp_value()

    def __gt__(self, other):
        other = self._to_decimal(other)
        if other is NotImplemented:
            return NotImplemented
        return self._cmp_value() > other._cmp_value()

    def __ge__(self, other):
        other = self._to_decimal(other)
        if other is NotImplemented:
            return NotImplemented
        return self._cmp_value() >= other._cmp_value()

    def __float__(self):
        return self._cmp_value()

    def __int__(self):
        val = self._coeff
        if self._exp > 0:
            val = val * (10 ** self._exp)
        elif self._exp < 0:
            val = val // (10 ** (-self._exp))
        return -val if self._sign else val

    def __bool__(self):
        return self._coeff != 0

    def __hash__(self):
        return hash(float(self))

    def __repr__(self):
        return "Decimal('%s')" % str(self)

    def __str__(self):
        if self._is_special:
            return 'NaN'  # Simplified
        if self._coeff == 0:
            if self._exp >= 0:
                return '-0' if self._sign else '0'
            else:
                s = '0.' + '0' * (-self._exp)
                return ('-' + s) if self._sign else s

        sign = '-' if self._sign else ''
        coeff_str = str(self._coeff)

        if self._exp == 0:
            return sign + coeff_str
        elif self._exp > 0:
            return sign + coeff_str + '0' * self._exp
        else:
            # Negative exponent: need decimal point
            abs_exp = -self._exp
            if abs_exp < len(coeff_str):
                # Insert decimal point
                int_part = coeff_str[:len(coeff_str) - abs_exp]
                frac_part = coeff_str[len(coeff_str) - abs_exp:]
                return sign + int_part + '.' + frac_part
            else:
                # Need leading zeros
                zeros = abs_exp - len(coeff_str)
                return sign + '0.' + '0' * zeros + coeff_str

    def quantize(self, exp):
        """Return value quantized to the given exponent.

        exp can be an int (number of decimal places) or a Decimal.
        Decimal('3.14159').quantize(Decimal('0.01')) -> Decimal('3.14')
        """
        if isinstance(exp, Decimal):
            target_exp = exp._exp
        elif isinstance(exp, int):
            target_exp = -exp
        else:
            raise TypeError("quantize requires a Decimal or int argument")

        if self._exp == target_exp:
            return Decimal(self)

        if self._exp < target_exp:
            # Need to reduce precision (round)
            diff = target_exp - self._exp
            divisor = 10 ** diff
            new_coeff = (self._coeff + divisor // 2) // divisor  # round half up
        else:
            # Need more precision
            diff = self._exp - target_exp
            new_coeff = self._coeff * (10 ** diff)

        d = Decimal(0)
        d._sign = self._sign
        d._coeff = new_coeff
        d._exp = target_exp
        return d

    def to_eng_string(self):
        """Convert to engineering string (exponent multiple of 3)."""
        if self._is_special:
            return str(self)

        sign = '-' if self._sign else ''

        if self._coeff == 0:
            return sign + '0'

        # Get the adjusted exponent
        coeff_str = str(self._coeff)
        adj_exp = self._exp + len(coeff_str) - 1

        # Engineering notation: exponent must be multiple of 3
        eng_exp = (adj_exp // 3) * 3
        if eng_exp > adj_exp:
            eng_exp = eng_exp - 3

        shift = adj_exp - eng_exp
        # shift is 0, 1, or 2

        if eng_exp == 0:
            return sign + str(self)
        else:
            # Place decimal point after (shift+1) digits
            if shift + 1 < len(coeff_str):
                int_part = coeff_str[:shift + 1]
                frac_part = coeff_str[shift + 1:]
                mantissa = int_part + '.' + frac_part
            else:
                mantissa = coeff_str + '0' * (shift + 1 - len(coeff_str))

            return sign + mantissa + 'E' + ('%+d' % eng_exp)

    def is_zero(self):
        """Return True if the value is zero."""
        return self._coeff == 0 and not self._is_special

    def is_signed(self):
        """Return True if the sign is 1 (negative)."""
        return self._sign == 1

    def copy_abs(self):
        """Return the absolute value."""
        return abs(self)

    def copy_negate(self):
        """Return the negation."""
        return -self
