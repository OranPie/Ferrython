# Edge case 1: Only positional-only args
result1 = (lambda /, a: a)(1)
print("Test 1 (/, a):", result1)

# Edge case 2: Slash with star
result2 = (lambda a, /, *, b: a + b)(1, b=2)
print("Test 2 (a, /, *, b):", result2)

# Edge case 3: Multiple with slash
result3 = (lambda a, b, /, c, d: a + b + c + d)(1, 2, 3, 4)
print("Test 3 (a, b, /, c, d):", result3)

# Edge case 4: Just slash (no positional-only)
try:
    result4 = (lambda /: 5)()
    print("Test 4 (/):", result4)
except Exception as e:
    print("Test 4 error:", e)
