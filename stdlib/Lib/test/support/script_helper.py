"""test.support.script_helper — stub for CPython's script_helper.

Provides minimal stubs so tests that import script_helper don't crash.
Most functions skip or raise SkipTest since Ferrython doesn't support
subprocess-based script execution in tests.
"""

import subprocess
import sys
import os
import unittest


def _interpreter_requires_environment():
    return False


def assert_python_ok(*args, **env_vars):
    """Run ferrython with *args* and assert exit code 0."""
    cmd = [sys.executable] + list(args)
    env = os.environ.copy()
    env.update(env_vars)
    proc = subprocess.run(cmd, capture_output=True, text=True, env=env,
                          timeout=30)
    if proc.returncode != 0:
        raise AssertionError(
            f"Process returned {proc.returncode}\n"
            f"stdout: {proc.stdout}\nstderr: {proc.stderr}"
        )
    return proc.returncode, proc.stdout, proc.stderr


def assert_python_failure(*args, **env_vars):
    """Run ferrython with *args* and assert non-zero exit code."""
    cmd = [sys.executable] + list(args)
    env = os.environ.copy()
    env.update(env_vars)
    proc = subprocess.run(cmd, capture_output=True, text=True, env=env,
                          timeout=30)
    if proc.returncode == 0:
        raise AssertionError(
            f"Process did not fail\nstdout: {proc.stdout}\nstderr: {proc.stderr}"
        )
    return proc.returncode, proc.stdout, proc.stderr


def spawn_python(*args, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, **kw):
    """Spawn a ferrython subprocess."""
    cmd = [sys.executable] + list(args)
    return subprocess.Popen(cmd, stdout=stdout, stderr=stderr, **kw)


def kill_python(p):
    """Kill a ferrython subprocess and return its output."""
    p.stdin.close() if p.stdin else None
    data = p.stdout.read() if p.stdout else b""
    p.stdout.close() if p.stdout else None
    p.wait()
    return data


def make_script(script_dir, script_basename, source, omit_suffix=False):
    """Create a script in *script_dir* with the given source."""
    suffix = "" if omit_suffix else ".py"
    script_filename = os.path.join(script_dir, script_basename + suffix)
    with open(script_filename, "w") as f:
        f.write(source)
    return script_filename
