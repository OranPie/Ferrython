# Test phase 89: Enhanced subprocess (env, input, check, capture_output)
import subprocess

passed = 0
failed = 0

def check(cond, msg):
    global passed, failed
    if cond:
        passed += 1
    else:
        failed += 1
        print(f"FAIL: {msg}")

# 1. Basic run with text mode
result = subprocess.run(["echo", "hello"], text=True, capture_output=True)
check(result.returncode == 0, "run echo returncode")
check("hello" in result.stdout, "run echo stdout")

# 2. check=True with success
result2 = subprocess.run(["true"], check=True)
check(result2.returncode == 0, "check=True success")

# 3. check=True with failure
try:
    subprocess.run(["false"], check=True)
    check(False, "check=True should raise on failure")
except Exception as e:
    check("non-zero" in str(e), "check=True raises on failure")

# 4. input parameter (piping stdin data)
result4 = subprocess.run(["cat"], input=b"hello world", capture_output=True)
check(b"hello world" in result4.stdout, "input bytes piped to stdin")

# 5. input with text mode
result5 = subprocess.run(["cat"], input="text input", text=True, capture_output=True)
check("text input" in result5.stdout, "input text piped to stdin")

# 6. env parameter
result6 = subprocess.run(["env"], text=True, capture_output=True, env={"MY_VAR": "hello123"})
check("MY_VAR=hello123" in result6.stdout, "env parameter")

# 7. cwd parameter
result7 = subprocess.run(["pwd"], text=True, capture_output=True, cwd="/tmp")
check("/tmp" in result7.stdout.strip(), "cwd parameter")

# 8. shell=True
result8 = subprocess.run(["echo $((2+3))"], shell=True, text=True, capture_output=True)
check("5" in result8.stdout, "shell=True arithmetic")

# 9. universal_newlines (alias for text)
result9 = subprocess.run(["echo", "test"], universal_newlines=True, capture_output=True)
check(isinstance(result9.stdout, str), "universal_newlines gives str")

# 10. call() returns exit code
rc = subprocess.call(["true"])
check(rc == 0, "call returns 0")

rc2 = subprocess.call(["false"])
check(rc2 != 0, "call returns nonzero for false")

# 11. check_output()
out = subprocess.check_output(["echo", "captured"])
check(b"captured" in out, "check_output captures stdout")

# 12. PIPE/STDOUT/DEVNULL constants
check(subprocess.PIPE == -1, "PIPE constant")
check(subprocess.STDOUT == -2, "STDOUT constant")
check(subprocess.DEVNULL == -3, "DEVNULL constant")

print(f"test_phase89: {passed} passed, {failed} failed")
