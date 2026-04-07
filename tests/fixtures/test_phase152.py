# Phase 152: ExceptionGroup.subgroup/split, pickle BytesIO, types.ModuleType

# --- ExceptionGroup.subgroup ---
eg = ExceptionGroup("errors", [ValueError("v1"), TypeError("t1"), ValueError("v2")])
sg = eg.subgroup(ValueError)
assert sg is not None, "subgroup should not be None"
excs = list(sg.exceptions)
assert len(excs) == 2, f"expected 2, got {len(excs)}"
sg2 = eg.subgroup(KeyError)
assert sg2 is None, "subgroup of non-matching should be None"

# --- ExceptionGroup.split ---
matched, rest = eg.split(TypeError)
assert matched is not None
assert rest is not None
m_excs = list(matched.exceptions)
r_excs = list(rest.exceptions)
assert len(m_excs) == 1, f"expected 1 matched, got {len(m_excs)}"
assert len(r_excs) == 2, f"expected 2 rest, got {len(r_excs)}"

# --- pickle dump/load with BytesIO ---
import pickle, io
data = {"key": [1, 2, 3], "nested": {"a": True}}
buf = io.BytesIO()
pickle.dump(data, buf)
buf.seek(0)
loaded = pickle.load(buf)
assert loaded == data, f"pickle round-trip failed: {loaded}"

# --- types.ModuleType constructor ---
import types
m = types.ModuleType("mymod", "my docstring")
assert hasattr(m, "__name__"), "module should have __name__"
assert m.__name__ == "mymod", f"expected 'mymod', got {m.__name__}"

print("All phase 152 tests passed!")
