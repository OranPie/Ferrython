# Test Phase 96: ferrython-traceback + ferrython-async crate integration
# Tests the new dedicated Rust crates for traceback and async systems

import sys

checks = 0

# ── traceback module (now backed by ferrython-traceback crate) ──────────

import traceback

# format_exc() when no exception
result = traceback.format_exc()
assert "NoneType: None" in result, f"Expected NoneType: None, got {result}"
checks += 1

# format_exception_only with exception type
lines = traceback.format_exception_only(ValueError, "bad value")
assert len(lines) >= 1, f"Expected at least 1 line, got {len(lines)}"
checks += 1

# format_tb with None returns empty
result = traceback.format_tb(None)
assert isinstance(result, list), "format_tb should return list"
checks += 1

# extract_tb with None returns empty
result = traceback.extract_tb(None)
assert isinstance(result, list), "extract_tb should return list"
checks += 1

# print_exc should not crash
traceback.print_exc()
checks += 1

# print_exception should not crash
traceback.print_exception(ValueError, "test", None)
checks += 1

# Real traceback from caught exception
try:
    1 / 0
except ZeroDivisionError:
    exc_info = sys.exc_info()
    assert exc_info[0] is not None, "exc_info type should be set"
    checks += 1
    assert exc_info[1] is not None, "exc_info value should be set"
    checks += 1

# format_exception with arguments
lines = traceback.format_exception(TypeError, "wrong type", None)
assert any("TypeError" in str(l) for l in lines), f"Expected TypeError in output, got {lines}"
checks += 1

# FrameSummary constructor
fs = traceback.FrameSummary("<test>", 42, "my_func")
assert fs.filename == "<test>", f"Expected '<test>', got {fs.filename}"
checks += 1
assert fs.lineno == 42, f"Expected 42, got {fs.lineno}"
checks += 1
assert fs.name == "my_func", f"Expected 'my_func', got {fs.name}"
checks += 1

# ── asyncio module (now backed by ferrython-async crate) ────────────────

import asyncio

# asyncio.iscoroutine
async def sample_coro():
    return 42

coro = sample_coro()
assert asyncio.iscoroutine(coro) == True, "Should detect coroutine"
checks += 1
assert asyncio.iscoroutine(42) == False, "int is not coroutine"
checks += 1

# asyncio.iscoroutinefunction
assert asyncio.iscoroutinefunction(sample_coro) == True, "Should detect coroutine function"
checks += 1
assert asyncio.iscoroutinefunction(print) == False, "print is not coroutine function"
checks += 1

# asyncio.run with simple coroutine
async def simple():
    return 100

result = asyncio.run(simple())
assert result == 100, f"Expected 100, got {result}"
checks += 1

# asyncio.sleep
async def sleep_test():
    await asyncio.sleep(0)
    return "done"

result = asyncio.run(sleep_test())
assert result == "done", f"Expected 'done', got {result}"
checks += 1

# asyncio.gather
async def add(x):
    return x + 1

async def gather_test():
    results = await asyncio.gather(add(1), add(2), add(3))
    return results

result = asyncio.run(gather_test())
assert result == [2, 3, 4], f"Expected [2, 3, 4], got {result}"
checks += 1

# asyncio.create_task
task = asyncio.create_task(sample_coro())
assert task is not None, "create_task should return a Task"
checks += 1

# asyncio.Future
fut = asyncio.Future()
assert fut.done() == False, "New future should not be done"
checks += 1
assert fut.cancelled() == False, "New future should not be cancelled"
checks += 1
fut.set_result(99)
assert fut.done() == True, "Future should be done after set_result"
checks += 1
assert fut.result() == 99, f"Expected 99, got {fut.result()}"
checks += 1

# Future.cancel
fut2 = asyncio.Future()
assert fut2.cancel() == True, "Cancel should succeed on pending future"
checks += 1
assert fut2.cancelled() == True, "Should be cancelled"
checks += 1

# asyncio.Queue
q = asyncio.Queue()
q.put_nowait(10)
q.put_nowait(20)
assert q.qsize() == 2, f"Expected qsize 2, got {q.qsize()}"
checks += 1
assert q.empty() == False, "Queue should not be empty"
checks += 1

# asyncio.Event
ev = asyncio.Event()
assert ev.is_set() == False, "New event should not be set"
checks += 1
ev.set()
assert ev.is_set() == True, "Event should be set after set()"
checks += 1
ev.clear()
assert ev.is_set() == False, "Event should not be set after clear()"
checks += 1

# asyncio.Lock
lock = asyncio.Lock()
assert lock.locked() == False, "New lock should not be locked"
checks += 1

# asyncio.Semaphore
sem = asyncio.Semaphore(2)
assert sem is not None, "Semaphore should be created"
checks += 1

# asyncio.get_event_loop
loop = asyncio.get_event_loop()
assert loop is not None, "Should return event loop"
checks += 1

# Constants
assert asyncio.FIRST_COMPLETED == "FIRST_COMPLETED"
checks += 1
assert asyncio.ALL_COMPLETED == "ALL_COMPLETED"
checks += 1

# Exception classes exist
assert asyncio.TimeoutError is not None
checks += 1
assert asyncio.CancelledError is not None
checks += 1

print(f"All {checks} checks passed!")
