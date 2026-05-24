# test_phase158.py - recursive container comparison and deepcopy guards

import copy
import operator


def assert_raises_recursion(func, *args):
    try:
        func(*args)
    except RecursionError:
        return
    raise AssertionError("expected RecursionError")


left = []
left.append(left)
right = copy.deepcopy(left)

for op in (operator.eq, operator.ne, operator.lt, operator.le, operator.gt, operator.ge):
    assert_raises_recursion(op, left, right)

left_tuple = ([],)
left_tuple[0].append(left_tuple)
right_tuple = copy.deepcopy(left_tuple)

assert right_tuple is not left_tuple
assert right_tuple[0] is not left_tuple[0]
assert right_tuple[0][0] is right_tuple

for op in (operator.eq, operator.ne, operator.lt, operator.le, operator.gt, operator.ge):
    assert_raises_recursion(op, left_tuple, right_tuple)

left_dict = {}
left_dict["self"] = left_dict
right_dict = copy.deepcopy(left_dict)

assert right_dict is not left_dict
assert right_dict["self"] is right_dict

assert_raises_recursion(operator.eq, left_dict, right_dict)
assert_raises_recursion(operator.ne, left_dict, right_dict)

print("test_phase158 passed")
