# Test positional-only parameters in lambda
result = (lambda a, /, b: a + b)(1, 2)
print(result)  # Should print 3

# Test with multiple posonly args
result2 = (lambda x, y, /, z: x + y + z)(1, 2, 3)
print(result2)  # Should print 6

# Test with default
result3 = (lambda a, /, b=5: a + b)(10)
print(result3)  # Should print 15
