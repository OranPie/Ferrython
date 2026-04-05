# Try to call with keyword argument when should be positional-only
try:
    f = lambda a, /, b: a + b
    result = f(a=1, b=2)  # Should error: can't pass 'a' as keyword
    print("ERROR: Should have failed!")
except TypeError as e:
    print("Correctly caught:", e)
