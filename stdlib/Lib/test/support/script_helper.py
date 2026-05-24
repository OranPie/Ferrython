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
    cmd = [_python_exe()] + list(args)
    env = os.environ.copy()
    env.update(env_vars)
    proc = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env,
                          timeout=30)
    if proc.returncode != 0:
        raise AssertionError(
            f"Process returned {proc.returncode}\n"
            f"stdout: {proc.stdout}\nstderr: {proc.stderr}"
        )
    return _PythonRunResult(proc.returncode, proc.stdout, proc.stderr)


def assert_python_failure(*args, **env_vars):
    """Run ferrython with *args* and assert non-zero exit code."""
    cmd = [_python_exe()] + list(args)
    env = os.environ.copy()
    env.update(env_vars)
    proc = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env,
                          timeout=30)
    if proc.returncode == 0:
        raise AssertionError(
            f"Process did not fail\nstdout: {proc.stdout}\nstderr: {proc.stderr}"
        )
    return _PythonRunResult(proc.returncode, proc.stdout, proc.stderr)


class _PythonProcess:
    def __init__(self, proc, stderr_is_stdout=False):
        self._proc = proc
        self._stderr_is_stdout = stderr_is_stdout

    def __enter__(self):
        self._proc.__enter__()
        return self

    def __exit__(self, *exc_info):
        return self._proc.__exit__(*exc_info)

    def __getattr__(self, name):
        return getattr(self._proc, name)

    def communicate(self, *args, **kwargs):
        out, err = self._proc.communicate(*args, **kwargs)
        if self._stderr_is_stdout:
            err = None
        return out, err


def spawn_python(*args, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, **kw):
    """Spawn a ferrython subprocess."""
    cmd = [_python_exe()] + list(args)
    proc = subprocess.Popen(cmd, stdin=stdin, stdout=stdout, stderr=stderr, **kw)
    return _PythonProcess(proc, stderr is subprocess.STDOUT)


def kill_python(p):
    """Kill a ferrython subprocess and return its output."""
    p.stdin.close() if p.stdin else None
    data = p.stdout.read() if p.stdout else b""
    p.stdout.close() if p.stdout else None
    p.wait()
    return data


class _PythonRunResult:
    def __init__(self, rc, out, err):
        self.rc = rc
        self.out = out
        self.err = err

    def __iter__(self):
        yield self.rc
        yield self.out
        yield self.err


def _python_exe():
    env_exe = os.environ.get("FERRYTHON_EXECUTABLE")
    if env_exe:
        return env_exe
    if os.path.isabs(sys.executable) and os.path.exists(sys.executable):
        return sys.executable
    workspace_exe = os.path.join(os.getcwd(), "target", "debug", "ferrython")
    if os.path.exists(workspace_exe):
        return workspace_exe
    return sys.executable


def run_python_until_end(*args, **env_vars):
    """Run ferrython and return a CPython-like completed-process tuple."""
    cmd = [_python_exe()] + list(args)
    env = os.environ.copy()
    env.update(env_vars)
    proc = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                          env=env, timeout=30)
    result = _PythonRunResult(proc.returncode, proc.stdout, proc.stderr)
    return result, cmd


def make_script(script_dir, script_basename, source, omit_suffix=False):
    """Create a script in *script_dir* with the given source."""
    suffix = "" if omit_suffix else ".py"
    script_filename = os.path.join(script_dir, script_basename + suffix)
    with open(script_filename, "w") as f:
        f.write(source)
    return script_filename
