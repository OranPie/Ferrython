"""numbers module — Abstract base classes for numbers."""

class Number:
    """All numbers derive from this."""
    __hash__ = None
    
    def __init__(self):
        pass

class Complex(Number):
    """Complex defines operations for complex numbers."""
    
    def __init__(self, real=0, imag=0):
        self._real = real
        self._imag = imag
    
    @property
    def real(self):
        return self._real
    
    @property
    def imag(self):
        return self._imag
    
    def conjugate(self):
        return Complex(self._real, -self._imag)
    
    def __add__(self, other):
        if isinstance(other, Complex):
            return Complex(self._real + other._real, self._imag + other._imag)
        return Complex(self._real + other, self._imag)
    
    def __sub__(self, other):
        if isinstance(other, Complex):
            return Complex(self._real - other._real, self._imag - other._imag)
        return Complex(self._real - other, self._imag)
    
    def __mul__(self, other):
        if isinstance(other, Complex):
            r = self._real * other._real - self._imag * other._imag
            i = self._real * other._imag + self._imag * other._real
            return Complex(r, i)
        return Complex(self._real * other, self._imag * other)
    
    def __abs__(self):
        return (self._real ** 2 + self._imag ** 2) ** 0.5
    
    def __eq__(self, other):
        if isinstance(other, Complex):
            return self._real == other._real and self._imag == other._imag
        if isinstance(other, (int, float)):
            return self._imag == 0 and self._real == other
        return NotImplemented
    
    def __repr__(self):
        if self._imag >= 0:
            return "({0}+{1}j)".format(self._real, self._imag)
        return "({0}{1}j)".format(self._real, self._imag)
    
    def __bool__(self):
        return self._real != 0 or self._imag != 0

class Real(Complex):
    """Real adds operations specific to real numbers."""
    
    def __init__(self, value=0):
        self._real = value
        self._imag = 0
    
    def __float__(self):
        return float(self._real)
    
    def __trunc__(self):
        return int(self._real)
    
    def __floor__(self):
        import math
        return math.floor(self._real)
    
    def __ceil__(self):
        import math
        return math.ceil(self._real)
    
    def __round__(self, ndigits=None):
        if ndigits is None:
            return round(self._real)
        return round(self._real, ndigits)

class Rational(Real):
    """Rational adds a numerator/denominator property."""
    
    def __init__(self, numerator=0, denominator=1):
        if denominator == 0:
            raise ZeroDivisionError("Rational denominator cannot be zero")
        self._numerator = numerator
        self._denominator = denominator
        self._real = numerator / denominator
        self._imag = 0
    
    @property
    def numerator(self):
        return self._numerator
    
    @property
    def denominator(self):
        return self._denominator

class Integral(Rational):
    """Integral adds methods for integer operations."""
    
    def __init__(self, value=0):
        self._value = int(value)
        self._numerator = self._value
        self._denominator = 1
        self._real = self._value
        self._imag = 0
    
    def __int__(self):
        return self._value
    
    def __index__(self):
        return self._value
    
    def __pow__(self, exponent, modulus=None):
        if modulus is None:
            return self._value ** exponent
        return pow(self._value, exponent, modulus)
    
    def __lshift__(self, other):
        return self._value << other
    
    def __rshift__(self, other):
        return self._value >> other
    
    def __and__(self, other):
        return self._value & other
    
    def __or__(self, other):
        return self._value | other
    
    def __xor__(self, other):
        return self._value ^ other
    
    def __invert__(self):
        return ~self._value
