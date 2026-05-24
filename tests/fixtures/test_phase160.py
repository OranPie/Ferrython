# test_phase160.py - recursive dict view repr guard

d = {}
d[42] = d.values()
values_repr = repr(d)
assert isinstance(values_repr, str)
assert "dict_values" in values_repr
assert "..." in values_repr

d[42] = d.items()
items_repr = repr(d)
assert isinstance(items_repr, str)
assert "dict_items" in items_repr
assert "..." in items_repr

print("test_phase160 passed")
