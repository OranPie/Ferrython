# Test phase 88: Enhanced argparse (nargs, choices, actions, parse_known_args)
import argparse

passed = 0
failed = 0

def check(cond, msg):
    global passed, failed
    if cond:
        passed += 1
    else:
        failed += 1
        print(f"FAIL: {msg}")

# 1. Basic positional + optional
parser = argparse.ArgumentParser(description="test")
parser.add_argument("name")
parser.add_argument("--count", type="int", default=1)
ns = parser.parse_args(["hello", "--count", "5"])
check(ns.name == "hello", "positional arg")
check(ns.count == 5, "optional int arg")

# 2. store_true / store_false
parser2 = argparse.ArgumentParser()
parser2.add_argument("--verbose", action="store_true")
parser2.add_argument("--no-cache", action="store_false", dest="cache")
ns2 = parser2.parse_args(["--verbose"])
check(ns2.verbose == True, "store_true set")
check(ns2.cache == True, "store_false default")

ns2b = parser2.parse_args(["--no-cache"])
check(ns2b.verbose == False, "store_true default")
check(ns2b.cache == False, "store_false set")

# 3. nargs='*' (zero or more)
parser3 = argparse.ArgumentParser()
parser3.add_argument("--files", nargs="*")
ns3 = parser3.parse_args(["--files", "a.txt", "b.txt", "c.txt"])
check(ns3.files == ["a.txt", "b.txt", "c.txt"], "nargs=* multiple")

ns3b = parser3.parse_args([])
check(ns3b.files is None, "nargs=* empty default is None")

# 4. nargs='+' (one or more)
parser4 = argparse.ArgumentParser()
parser4.add_argument("--items", nargs="+")
ns4 = parser4.parse_args(["--items", "x", "y"])
check(ns4.items == ["x", "y"], "nargs=+ multiple")

# 5. nargs with integer
parser5 = argparse.ArgumentParser()
parser5.add_argument("--pair", nargs="2")
ns5 = parser5.parse_args(["--pair", "a", "b"])
check(ns5.pair == ["a", "b"], "nargs=2")

# 6. choices validation
parser6 = argparse.ArgumentParser()
parser6.add_argument("--color", choices=["red", "green", "blue"])
ns6 = parser6.parse_args(["--color", "red"])
check(ns6.color == "red", "choices valid")

# 7. action='count'
parser7 = argparse.ArgumentParser()
parser7.add_argument("-v", action="count")
ns7 = parser7.parse_args(["-v", "-v", "-v"])
check(ns7.v == 3, "action=count")

# 8. action='append'
parser8 = argparse.ArgumentParser()
parser8.add_argument("--include", action="append")
ns8 = parser8.parse_args(["--include", "foo", "--include", "bar"])
check(ns8.include == ["foo", "bar"], "action=append")

# 9. parse_known_args
parser9 = argparse.ArgumentParser()
parser9.add_argument("--known")
result = parser9.parse_known_args(["--known", "val", "--unknown", "stuff"])
ns9 = result[0]
remaining = result[1]
check(ns9.known == "val", "parse_known_args known")
check("--unknown" in remaining, "parse_known_args unknown flag")

# 10. positional with nargs='*'
parser10 = argparse.ArgumentParser()
parser10.add_argument("files", nargs="*")
ns10 = parser10.parse_args(["a.py", "b.py"])
check(ns10.files == ["a.py", "b.py"], "positional nargs=*")

# 11. Namespace class
ns_direct = argparse.Namespace(foo="bar", count=42)
check(ns_direct.foo == "bar", "Namespace direct kwargs")
check(ns_direct.count == 42, "Namespace direct int kwarg")

# 12. -- separator
parser12 = argparse.ArgumentParser()
parser12.add_argument("cmd")
parser12.add_argument("rest", nargs="*")
ns12 = parser12.parse_args(["run", "--", "extra1"])
check(ns12.cmd == "run", "-- separator positional before")

print(f"test_phase88: {passed} passed, {failed} failed")
