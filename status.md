# Ferrython 修复状态

Last updated: 2026-05-25T18:16:22+08:00

## 已提交成果

- `fbb78f6 fix: support bytes C API compatibility`
  - 增加 `ctypes.pythonapi.PyBytes_FromFormat`、`ctypes.py_object` 和 unsigned ctypes 包装兼容。
  - 补齐 `_testcapi` 测试常量入口，支撑 CPython `test_bytes` 相关用例。
  - 修复 BigInt 下 `%#x` / percent formatting 路径。
  - 增加通用 queued finalizer 机制，确保 `__del__` 在最后强引用释放后进入 unraisable hook 流程。
  - 为 builtin-value subclass 的 `iter()` / `reversed()` 迭代器保活 owner，避免迭代期间底层对象被提前释放。

## 已通过验证

- `cargo build --release -p ferrython-cli --bin ferrython -j6`
- `target/release/ferrython tools/run_cpython_tests.py -v test_bytes`
  - `run=264 pass=264 fail=0 err=0 skip=0`
- `target/release/ferrython tools/run_cpython_tests.py -v test_exceptions test_grammar test_compile test_print`
- `git diff --check`

## 当前工作树

- 代码区当前无未提交源码改动。
- 未跟踪项：`.codex-work/`，保留为本地工作资料，不纳入提交。

## 当前修复候选

- `test_iter` 单项运行触发 Rust stack overflow，需要继续缩小具体 case。
- 已定位首个兼容性失败：
  - `test_iter.TestCase.test_iter_class_for`
  - `pickle.loads(pickle.dumps(user_iterator))` 后生成同名空类，丢失 `__iter__` / `__next__`。
  - 结果导致 `isinstance(it, collections.abc.Iterator)` 为 `False`。
- 候选通用修复方向：
  - `pickle.loads()` 重建用户类实例时，优先从当前 globals / 模块对象中解析已有类。
  - 找不到已有类时再 fallback 到当前空类兼容逻辑。
  - 这是 pickle 类解析逻辑修复，不是针对 `test_iter` 的源码特判。

## 后续修复队列

1. 修复 pickle 用户类复用问题，并只 rebuild 一次验证。
2. 运行 `test_iter.TestCase('test_iter_class_for')` 聚焦验证。
3. 继续按 case 扫描 `test_iter`，找出 stack overflow 的真实触发用例。
4. 扩展小批候选：
   - `test_iter`
   - `test_list`
   - `test_tuple`
   - `test_dict`
   - `test_set`
   - `test_weakref`
   - `test_copy`
   - `test_deque`
5. 性能队列：
   - 避免全量 CPython 测试长跑，继续单项/小批探测。
   - 对比 CPython baseline 后再做优化判断。
   - 重点关注 loader、argparse、测试执行器输出和常见对象路径。

## 更新规则

- 每完成一个 focused fix 并通过验证后更新本文件。
- 每次提交后追加 commit hash、修复点和验证命令。
- 遇到新的全局失败、stack overflow 或明显性能热点时，将其加入队列并标注当前证据。
