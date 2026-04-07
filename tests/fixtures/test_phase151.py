# test_phase151: __index__ slice protocol, enum custom __str__, Fraction string parsing,
#                asyncio.gather with create_task, tokenize.tokenize alias

# Group 1: __index__ protocol for slicing
class Idx:
    def __init__(self, n):
        self.n = n
    def __index__(self):
        return self.n

lst = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]
assert lst[Idx(2)] == 2, "direct __index__ subscript"
assert lst[Idx(1):Idx(4)] == [1, 2, 3], f"__index__ slice: got {lst[Idx(1):Idx(4)]}"
assert lst[Idx(0):Idx(8):Idx(2)] == [0, 2, 4, 6], "__index__ slice with step"

# Group 2: Enum custom __str__ and __repr__
from enum import Enum

class Status(Enum):
    ACTIVE = 'active'
    INACTIVE = 'inactive'
    def __str__(self):
        return self.value

assert str(Status.ACTIVE) == 'active', f"enum custom __str__: got {str(Status.ACTIVE)}"
assert Status('active') is Status.ACTIVE, "enum value lookup"

# Default enum str (no custom __str__)
class Color(Enum):
    RED = 1
    GREEN = 2
    BLUE = 3

assert 'Color.RED' in str(Color.RED), f"default enum str: got {str(Color.RED)}"

# Group 3: Fraction from string
from fractions import Fraction

f1 = Fraction('0.5')
assert f1 == Fraction(1, 2), f"Fraction('0.5'): got {f1}"
f2 = Fraction('3.14')
assert f2 == Fraction(157, 50), f"Fraction('3.14'): got {f2}"
f3 = Fraction('1/3')
assert f3 == Fraction(1, 3), f"Fraction('1/3'): got {f3}"
f4 = Fraction('42')
assert f4 == Fraction(42, 1), f"Fraction('42'): got {f4}"

# Group 4: asyncio.gather with create_task
import asyncio

async def double(n):
    await asyncio.sleep(0)
    return n * 2

async def gather_tasks():
    tasks = [asyncio.create_task(double(i)) for i in range(5)]
    results = await asyncio.gather(*tasks)
    return list(results)

assert asyncio.run(gather_tasks()) == [0, 2, 4, 6, 8], "gather with create_task"

# Group 5: tokenize module has tokenize function
import tokenize
assert hasattr(tokenize, 'tokenize'), "tokenize.tokenize exists"
assert hasattr(tokenize, 'generate_tokens'), "tokenize.generate_tokens exists"

print("test_phase151 passed")
