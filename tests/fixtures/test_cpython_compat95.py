# test_cpython_compat95.py - More exception handling

passed95 = 0
total95 = 0

def check95(desc, got, expected):
    global passed95, total95
    total95 += 1
    if got == expected:
        passed95 += 1
    else:
        print(f"FAIL: {desc}: got {got!r}, expected {expected!r}")

# Basic exception catching
try:
    result95_1 = 1 / 0
    check95("ZeroDivisionError not raised", False, True)
except ZeroDivisionError:
    check95("ZeroDivisionError caught", True, True)

# Exception args
try:
    raise ValueError("test message")
except ValueError as e:
    check95("exception args", e.args, ("test message",))
    check95("exception str", str(e), "test message")

# Multiple exception types in one except
try:
    raise TypeError("bad type")
except (ValueError, TypeError) as e:
    check95("multiple except types", type(e).__name__, "TypeError")

# Exception chaining with raise from (explicit cause)
try:
    try:
        raise ValueError("original")
    except ValueError as orig:
        raise RuntimeError("wrapper") from orig
except RuntimeError as e:
    check95("chained exception __cause__ type", type(e.__cause__).__name__, "ValueError")
    check95("chained exception __cause__ message", str(e.__cause__), "original")
    check95("chained exception message", str(e), "wrapper")

# Implicit exception chaining (__context__)
try:
    try:
        raise ValueError("first")
    except ValueError:
        raise TypeError("second")
except TypeError as e:
    check95("implicit chain __context__ type", type(e.__context__).__name__, "ValueError")
    check95("implicit chain __context__ message", str(e.__context__), "first")

# Suppress chaining with raise from None
try:
    try:
        raise ValueError("original")
    except ValueError:
        raise RuntimeError("clean") from None
except RuntimeError as e:
    check95("suppress chain __cause__ is None", e.__cause__, None)
    check95("suppress chain __suppress_context__", e.__suppress_context__, True)

# Custom exception with attributes
class CustomError95(Exception):
    def __init__(self, code, message):
        super().__init__(message)
        self.code = code
        self.message = message

try:
    raise CustomError95(404, "not found")
except CustomError95 as e:
    check95("custom exception code", e.code, 404)
    check95("custom exception message", e.message, "not found")
    check95("custom exception str", str(e), "not found")
    check95("custom exception isinstance", isinstance(e, Exception), True)

# Custom exception hierarchy
class AppError95(Exception):
    pass

class DatabaseError95(AppError95):
    pass

class ConnectionError95(DatabaseError95):
    pass

try:
    raise ConnectionError95("db down")
except AppError95 as e:
    check95("exception hierarchy caught by parent", type(e).__name__, "ConnectionError95")

try:
    raise DatabaseError95("query failed")
except ConnectionError95:
    check95("exception hierarchy wrong - should not catch", False, True)
except DatabaseError95:
    check95("exception hierarchy correct subclass", True, True)

# BaseException subclass
class MyKeyboardInterrupt95(BaseException):
    pass

try:
    raise MyKeyboardInterrupt95("interrupted")
except BaseException as e:
    check95("BaseException subclass caught", type(e).__name__, "MyKeyboardInterrupt95")
    check95("BaseException subclass message", str(e), "interrupted")

# BaseException not caught by Exception
try:
    try:
        raise KeyboardInterrupt("stop")
    except Exception:
        check95("KeyboardInterrupt caught by Exception - wrong", False, True)
except KeyboardInterrupt:
    check95("KeyboardInterrupt not caught by Exception", True, True)

# SystemExit
try:
    raise SystemExit(42)
except SystemExit as e:
    check95("SystemExit code", e.code, 42)
    check95("SystemExit is BaseException", isinstance(e, BaseException), True)
    check95("SystemExit is not Exception", isinstance(e, Exception), False)

# SystemExit with 0
try:
    raise SystemExit(0)
except SystemExit as e:
    check95("SystemExit code 0", e.code, 0)

# finally always runs
finally_ran_95 = False
try:
    x95_tmp = 1
finally:
    finally_ran_95 = True
check95("finally runs on normal exit", finally_ran_95, True)

# finally runs on exception
finally_ran2_95 = False
try:
    try:
        raise ValueError("boom")
    finally:
        finally_ran2_95 = True
except ValueError:
    pass
check95("finally runs on exception", finally_ran2_95, True)

# Exception in finally
result95_fin = []
try:
    try:
        result95_fin.append("try")
        raise ValueError("original")
    except ValueError:
        result95_fin.append("except")
    finally:
        result95_fin.append("finally")
except Exception:
    result95_fin.append("outer-except")
check95("exception in try/except/finally order", result95_fin, ["try", "except", "finally"])

# Else clause runs when no exception
else_ran_95 = False
try:
    x95_tmp2 = 42
except Exception:
    pass
else:
    else_ran_95 = True
check95("else clause runs on no exception", else_ran_95, True)

# Else clause does not run on exception
else_ran2_95 = False
try:
    raise ValueError("oops")
except ValueError:
    pass
else:
    else_ran2_95 = True
check95("else clause skipped on exception", else_ran2_95, False)

# Nested try/except
result95_nested = "none"
try:
    try:
        raise ValueError("inner")
    except TypeError:
        result95_nested = "wrong"
except ValueError:
    result95_nested = "correct"
check95("nested try/except propagation", result95_nested, "correct")

# Re-raising exception
result95_reraise = "none"
try:
    try:
        raise ValueError("reraised")
    except ValueError:
        raise
except ValueError as e:
    result95_reraise = str(e)
check95("re-raised exception preserved", result95_reraise, "reraised")

# Exception with multiple args
try:
    raise ValueError("msg", 42, [1, 2])
except ValueError as e:
    check95("exception multiple args", e.args, ("msg", 42, [1, 2]))

# Exception with no args
try:
    raise ValueError()
except ValueError as e:
    check95("exception no args", e.args, ())
    check95("exception no args str", str(e), "")

# isinstance checks on exception types
v_exc = ValueError("test")
check95("ValueError isinstance Exception", isinstance(v_exc, Exception), True)
check95("ValueError isinstance BaseException", isinstance(v_exc, BaseException), True)
check95("ValueError not isinstance TypeError", isinstance(v_exc, TypeError), False)

# Exception equality (by identity, not value)
e1_95 = ValueError("same")
e2_95 = ValueError("same")
check95("exceptions not equal by value", e1_95 == e2_95, False)

# issubclass with exceptions
check95("ValueError subclass of Exception", issubclass(ValueError, Exception), True)
check95("Exception subclass of BaseException", issubclass(Exception, BaseException), True)
check95("KeyError subclass of LookupError", issubclass(KeyError, LookupError), True)
check95("IndexError subclass of LookupError", issubclass(IndexError, LookupError), True)

print(f"Tests: {total95} | Passed: {passed95} | Failed: {total95 - passed95}")
