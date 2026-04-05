# Test positional-only with DEFAULTS BEFORE the slash
# This is the potential bug case

# Case 1: Simple posonly with default
f1 = lambda a=10, /, b=20: a + b
print("Test 1 - f1():", f1())            # Should be 30
print("Test 1 - f1(5):", f1(5))          # Should be 25
print("Test 1 - f1(5, 7):", f1(5, 7))    # Should be 12

# Case 2: Multiple posonly with defaults
f2 = lambda x=1, y=2, /, z=3: x + y + z
print("Test 2 - f2():", f2())            # Should be 6
print("Test 2 - f2(10):", f2(10))        # Should be 15
print("Test 2 - f2(10, 20):", f2(10, 20))  # Should be 33
print("Test 2 - f2(10, 20, 30):", f2(10, 20, 30))  # Should be 60
