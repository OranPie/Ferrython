# This should test the edge case
try:
    f = lambda a, /, b: a + b
    print("Parse succeeded")
    print("Result:", f(1, 2))
except Exception as e:
    print("Error:", type(e).__name__, "-", e)
