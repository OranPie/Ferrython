# Phase 144: slice.indices, property class-level access, Exception.add_note,
# JSON subclass inheritance, memoryview methods

def test_slice_indices_basic():
    s = slice(1, 10, 2)
    assert s.indices(8) == (1, 8, 2), f"Expected (1,8,2), got {s.indices(8)}"

def test_slice_indices_none():
    s = slice(None, None)
    assert s.indices(5) == (0, 5, 1)

def test_slice_indices_negative():
    s = slice(-3, None)
    assert s.indices(10) == (7, 10, 1)

def test_slice_indices_negative_step():
    s = slice(None, None, -1)
    assert s.indices(5) == (4, -1, -1)

def test_property_class_access():
    class P:
        @property
        def x(self):
            return 42
    assert type(P.x).__name__ == "property"

def test_property_fget_access():
    class P:
        @property
        def x(self):
            return 99
    assert P.x.fget is not None
    assert P.x.fget(P()) == 99

def test_property_fset_access():
    class S:
        @property
        def y(self):
            return self._y
        @y.setter
        def y(self, val):
            self._y = val
    assert S.y.fset is not None

def test_property_instance_still_works():
    class P:
        @property
        def x(self):
            return 42
    assert P().x == 42

def test_exception_add_note():
    e = ValueError("test")
    e.add_note("note 1")
    e.add_note("note 2")
    assert e.__notes__ == ["note 1", "note 2"]

def test_exception_with_traceback():
    e = TypeError("t")
    ret = e.with_traceback(None)
    assert ret is e

def test_json_decoder_subclass():
    import json
    class MyDecoder(json.JSONDecoder):
        pass
    md = MyDecoder()
    assert md.decode('{"a": 1}') == {"a": 1}

def test_json_encoder_subclass():
    import json
    class MyEncoder(json.JSONEncoder):
        pass
    me = MyEncoder()
    assert me.encode([1, 2]) == "[1, 2]"

def test_json_decoder_raw_decode():
    import json
    d = json.JSONDecoder()
    val, idx = d.raw_decode('{"b": 2}')
    assert val == {"b": 2}
    assert idx > 0

def test_memoryview_tobytes():
    mv = memoryview(b"hello")
    assert mv.tobytes() == b"hello"

def test_memoryview_tolist():
    mv = memoryview(b"\x01\x02\x03")
    assert mv.tolist() == [1, 2, 3]

def test_memoryview_release():
    mv = memoryview(b"data")
    mv.release()  # should not raise

def test_memoryview_slice():
    mv = memoryview(b"abcdef")
    assert bytes(mv[1:4]) == b"bcd"

def test_cooperative_super():
    class A:
        def m(self): return ["A"]
    class B(A):
        def m(self): return ["B"] + super().m()
    class C(A):
        def m(self): return ["C"] + super().m()
    class D(B, C):
        def m(self): return ["D"] + super().m()
    assert D().m() == ["D", "B", "C", "A"]

if __name__ == "__main__":
    test_slice_indices_basic()
    test_slice_indices_none()
    test_slice_indices_negative()
    test_slice_indices_negative_step()
    test_property_class_access()
    test_property_fget_access()
    test_property_fset_access()
    test_property_instance_still_works()
    test_exception_add_note()
    test_exception_with_traceback()
    test_json_decoder_subclass()
    test_json_encoder_subclass()
    test_json_decoder_raw_decode()
    test_memoryview_tobytes()
    test_memoryview_tolist()
    test_memoryview_release()
    test_memoryview_slice()
    test_cooperative_super()
    print("All phase 144 tests passed!")
