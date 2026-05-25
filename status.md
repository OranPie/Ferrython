# Ferrython 修复状态

Last updated: 2026-05-25T19:15:17+08:00

## 已提交成果

- `a500095 docs: track test repair status`
  - 新增本文件，记录 CPython 兼容修复、已过验证、当前候选和后续队列。

- `fbb78f6 fix: support bytes C API compatibility`
  - 增加 `ctypes.pythonapi.PyBytes_FromFormat`、`ctypes.py_object` 和 unsigned ctypes 包装兼容。
  - 补齐 `_testcapi` 测试常量入口，支撑 CPython `test_bytes` 相关用例。
  - 修复 BigInt 下 `%#x` / percent formatting 路径。
  - 增加通用 queued finalizer 机制，确保 `__del__` 在最后强引用释放后进入 unraisable hook 流程。
  - 为 builtin-value subclass 的 `iter()` / `reversed()` 迭代器保活 owner，避免迭代期间底层对象被提前释放。

## 本轮修复成果

- 类创建补齐 `__module__`，用于 pickle 全局类定位。
- pickle 用户实例反序列化优先复用模块/当前 globals 中已有类，避免重建空类丢失方法。
- pickle writer 增加 identity memo，保留列表、字典、mappingproxy、实例的共享引用。
- pickle 支持 `SeqIter` 内部 reduce，保留 sequence iterator 的 source/index/exhausted 状态。
- 普通 Python instance 的旧序列协议 `__getitem__` 生成 lazy `SeqIter`。
- `iter()`/`__iter__` 返回值现在要求具备 `__next__`，错误非 iterator 返回值会变成 TypeError。
- `ForIterStoreFast` fallback 的错误返回改走正常异常展开，修复函数内 `try/except` 捕获 iterator 异常。
- membership (`in`/`not in`) 使用 VM 级迭代推进 Python instance 和 module-backed file iterator。
- `to_list()` / VM collect 支持 module-backed iterator、Python instance iterator、dict-backed `RefIter`。
- `operator.indexOf` 改成流式迭代，命中即停；`operator.countOf` 流式消费全部，保留文件 iterator 语义。
- two-arg `iter(callable, sentinel)` 增加 sink-state，遇到 sentinel 后永久耗尽。

## 已通过验证

- `cargo build --release -p ferrython-cli --bin ferrython -j6`
- `target/release/ferrython tools/run_cpython_tests.py -v test_bytes`
  - `run=264 pass=264 fail=0 err=0 skip=0`
- `target/release/ferrython tools/run_cpython_tests.py -v test_exceptions test_grammar test_compile test_print`
- 本轮按用户要求优先使用 debug/dev 构建加快修复迭代：
  - `cargo build -p ferrython-cli --bin ferrython -j6`
  - 最近一次 dev build: `Finished dev profile [optimized + debuginfo] target(s) in 40.71s`
- 本轮 focused 通过：
  - `test_iter.TestCase.test_iter_class_for`
  - `test_iter.TestCase.test_seq_class_for`
  - `test_iter.TestCase.test_mutating_seq_class_iter_pickle`
  - `test_iter.TestCase.test_new_style_iter_class`
  - `test_iter.TestCase.test_exception_function`
  - `test_iter.TestCase.test_in_and_not_in`
  - `test_iter.TestCase.test_countOf`
  - `test_iter.TestCase.test_indexOf`
  - `test_iter.TestCase.test_sinkstate_callable`
  - `test_iter.TestCase.test_sinkstate_dict`
  - `test_operator.PyOperatorTestCase.test_countOf`
  - `test_operator.PyOperatorTestCase.test_indexOf`
  - `test_operator.COperatorTestCase.test_countOf`
  - `test_operator.COperatorTestCase.test_indexOf`
- `test_iter` 单项扫描当前已推进到：
  - 通过 `test_sinkstate_yield`
  - 通过 `test_sinkstate_range`
  - 通过 `test_sinkstate_enumerate`
  - 通过 `test_3720`
  - 通过 `test_extending_list_with_iterator_does_not_segfault`
  - 通过 `test_iter_overflow`
  - 下一个失败：`test_iter_neg_setstate`

## 当前工作树

- 本轮代码修复涉及 iterator / pickle / operator / core conversion 路径。
- 未跟踪项：`.codex-work/`，保留为本地工作资料，不纳入提交。

## 当前修复候选

- `test_iter.TestCase.test_iter_neg_setstate`
  - 当前错误：`'iterator' object has no attribute '__setstate__'`
  - 方向：补齐基础 iterator `__setstate__` 边界语义，尤其负 state 处理。
- 后续仍需继续单项扫描 `test_iter`，确认是否还有 stack overflow 或长耗时 case。

## 已关闭候选

- `test_iter.TestCase.test_iter_class_for`
  - 修复：pickle 反序列化用户实例时复用已有类，并补齐类 `__module__`。
- `test_iter.TestCase.test_exception_function`
  - 修复：`ForIterStoreFast` fallback 不再用 `?`/early return 绕开异常展开。
- `test_iter.TestCase.test_in_and_not_in`
  - 修复：membership fallback 改用 VM 级 iterator 推进，支持 instance 和 file-like module。
- `test_iter.TestCase.test_countOf` / `test_iter.TestCase.test_indexOf`
  - 修复：operator 的 sequence count/index 走流式 iterator 语义。
- `test_iter.TestCase.test_sinkstate_callable`
  - 修复：callable sentinel iterator 增加耗尽状态。
- `test_iter.TestCase.test_sinkstate_dict`
  - 修复：VM/core conversion 支持 dict-backed `RefIter` 收集并推进 state。

## 修复原则

- 不硬编码 CPython test case；所有改动落在通用语义：
  - pickle 全局类解析和 memo 语义。
  - iterator protocol / old sequence protocol。
  - module-backed file iterator。
  - sink-state / stateful iterator 行为。
- 候选通用修复方向：
  - 对 `__setstate__` 等 pickle state API 做通用 iterator 支持。

## 后续修复队列

1. 修复 `test_iter.TestCase.test_iter_neg_setstate`。
2. 继续按 case 扫描 `test_iter`，找出后续失败或 stack overflow 的真实触发用例。
3. 提交下一批 focused fix 后继续更新本文件。
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
