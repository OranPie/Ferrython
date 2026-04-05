# test_phase79: Pure Python stdlib extensions + redirect_stdout verification
results = []

# ── 1. stat module (pure Python) ──
try:
    import stat
    
    assert stat.S_IFDIR == 0o040000
    assert stat.S_IFREG == 0o100000
    assert stat.S_IRUSR == 0o0400
    assert stat.S_IWUSR == 0o0200
    assert stat.S_IXUSR == 0o0100
    
    # S_ISDIR / S_ISREG
    assert stat.S_ISDIR(0o040755) == True
    assert stat.S_ISREG(0o100644) == True
    assert stat.S_ISDIR(0o100644) == False
    
    # S_IMODE
    assert stat.S_IMODE(0o100755) == 0o755
    
    results.append("PASS stat")
except Exception as e:
    results.append(f"FAIL stat: {e}")

# ── 2. redirect_stdout actually captures print output ──
try:
    import io
    from contextlib import redirect_stdout
    
    buf = io.StringIO()
    with redirect_stdout(buf):
        print("hello from redirect")
        print("second line")
    
    captured = buf.getvalue()
    assert "hello from redirect" in captured, f"redirect failed: {captured!r}"
    assert "second line" in captured, f"redirect missed second: {captured!r}"
    
    # After exit, print goes back to normal stdout
    # (just verify no crash)
    print("back to normal")
    
    results.append("PASS redirect_stdout")
except Exception as e:
    results.append(f"FAIL redirect_stdout: {e}")

# ── 3. io.StringIO seek/tell/truncate advanced ──
try:
    import io
    
    sio = io.StringIO()
    sio.write("0123456789")
    
    # tell at end
    assert sio.tell() == 10
    
    # seek to middle
    sio.seek(5)
    assert sio.tell() == 5
    assert sio.read(3) == "567"
    assert sio.tell() == 8
    
    # seek from start
    sio.seek(0)
    assert sio.read() == "0123456789"
    
    # truncate
    sio.seek(5)
    sio.truncate()
    assert sio.getvalue() == "01234"
    
    # write after truncate
    sio.write("XY")
    assert sio.getvalue() == "01234XY"
    
    results.append("PASS io_advanced")
except Exception as e:
    results.append(f"FAIL io_advanced: {e}")

# ── 4. Nested redirect_stdout ──
try:
    import io
    from contextlib import redirect_stdout
    
    buf1 = io.StringIO()
    buf2 = io.StringIO()
    
    with redirect_stdout(buf1):
        print("outer")
        with redirect_stdout(buf2):
            print("inner")
        print("back to outer")
    
    assert "outer" in buf1.getvalue()
    assert "back to outer" in buf1.getvalue()
    assert "inner" in buf2.getvalue()
    assert "inner" not in buf1.getvalue()
    
    results.append("PASS nested_redirect")
except Exception as e:
    results.append(f"FAIL nested_redirect: {e}")

# ── 5. functools.wraps and partial ──
try:
    from functools import wraps, partial, reduce
    
    # reduce with initial
    assert reduce(lambda a, b: a + b, [1, 2, 3, 4, 5]) == 15
    assert reduce(lambda a, b: a + b, [], 0) == 0
    assert reduce(lambda a, b: a + b, [1], 100) == 101
    
    # partial with kwargs
    def greet(name, greeting="Hello"):
        return f"{greeting}, {name}!"
    
    hi = partial(greet, greeting="Hi")
    assert hi("World") == "Hi, World!"
    
    results.append("PASS functools_extended")
except Exception as e:
    results.append(f"FAIL functools_extended: {e}")

# ── 6. collections.Counter, deque ──
try:
    from collections import Counter, deque
    
    # Counter
    c = Counter("abracadabra")
    mc = c.most_common(2)
    assert mc[0][0] == 'a', f"most_common: {mc}"
    assert mc[0][1] == 5, f"most_common count: {mc}"
    
    # deque
    d = deque([1, 2, 3], maxlen=5)
    d.append(4)
    d.append(5)
    d.append(6)  # should drop 1
    assert 1 not in d or len(d) <= 5
    assert 6 in d
    
    results.append("PASS collections_ext")
except Exception as e:
    results.append(f"FAIL collections_ext: {e}")

# ── 7. dataclasses with defaults and field() ──
try:
    from dataclasses import dataclass, field, asdict, replace
    
    @dataclass
    class Config:
        host: str = "localhost"
        port: int = 8080
        debug: bool = False
    
    cfg = Config()
    assert cfg.host == "localhost"
    assert cfg.port == 8080
    assert cfg.debug == False
    
    cfg2 = Config(host="example.com", port=443, debug=True)
    assert cfg2.host == "example.com"
    assert cfg2.port == 443
    
    # asdict
    d = asdict(cfg2)
    assert d == {"host": "example.com", "port": 443, "debug": True}
    
    # replace
    cfg3 = replace(cfg, port=9090)
    assert cfg3.port == 9090
    assert cfg3.host == "localhost"
    
    # equality
    assert Config() == Config()
    assert Config(port=80) != Config(port=443)
    
    results.append("PASS dataclasses_ext")
except Exception as e:
    results.append(f"FAIL dataclasses_ext: {e}")

# ── 8. types.SimpleNamespace ──
try:
    from types import SimpleNamespace
    
    ns = SimpleNamespace(a=1, b="hello")
    assert ns.a == 1
    assert ns.b == "hello"
    
    # Dynamic attributes
    ns.c = [1, 2, 3]
    assert ns.c == [1, 2, 3]
    
    # Empty namespace
    ns2 = SimpleNamespace()
    ns2.x = 42
    assert ns2.x == 42
    
    results.append("PASS types_ns")
except Exception as e:
    results.append(f"FAIL types_ns: {e}")

# ── Summary ──
for r in results:
    print(r)

passed = sum(1 for r in results if r.startswith("PASS"))
total = len(results)
print(f"\n{passed}/{total} checks passed")
assert passed == total, f"Some checks failed!"
