# Ferrython 修复状态

Last updated: 2026-05-25T23:53:20+08:00

## 已提交成果

- `a542303 fix: collect dict and set iterator cycles`
  - dict views 增加 owner 保活，dict subclass / dict 临时对象的 view 和 iterator 生命周期与 CPython 对齐。
  - cycle GC 遍历 dict/set storage、HashableKey identity/custom key、dict view、RefIter/RevRefIter、VecIter 和 IteratorData 引用。
  - set 对象纳入 cycle GC tracking，dict/set iterator cycle 能在一次 `gc.collect()` 后让 weakref 失效。
  - cycle GC 清理 instance 时同步清空 dict_storage，并用 bit flag 标记 weakref 已被 cycle GC 清理，避免弱引用看到半清理对象。

- `1ed1214 fix: use Python copy protocol implementation`
  - `copy` 模块改走 `stdlib/Lib/copy.py`，避免 Rust stub 遮住 pure-Python copyreg/reduce/memo 语义。
  - `copy.py` 补齐 dispatch table、copyreg registry、`__reduce__` / `__reduce_ex__`、`__getnewargs__` / `__getnewargs_ex__`、state/slotstate、list/dict/tuple subclass、memo keepalive 和 bound method deepcopy。
  - `dict.get()` VM 快路径保留 str/int/bool borrowed lookup，同时对 class/function/custom key fallback 到通用 hashable lookup，修复 `copyreg.dispatch_table.get(cls)`。
  - 核心属性层不再为缺失的 `__copy__` / `__deepcopy__` 制造 builtin placeholder，让协议探测能正确 fallback。
  - bound method 暴露 `__self__` / `__func__`，weakref.ref 对象增加稳定标记，`copy.copy()` / `copy.deepcopy()` 对 weakref ref 返回原对象。

- `a500095 docs: track test repair status`
  - 新增本文件，记录 CPython 兼容修复、已过验证、当前候选和后续队列。

- `fbb78f6 fix: support bytes C API compatibility`
  - 增加 `ctypes.pythonapi.PyBytes_FromFormat`、`ctypes.py_object` 和 unsigned ctypes 包装兼容。
  - 补齐 `_testcapi` 测试常量入口，支撑 CPython `test_bytes` 相关用例。
  - 修复 BigInt 下 `%#x` / percent formatting 路径。
  - 增加通用 queued finalizer 机制，确保 `__del__` 在最后强引用释放后进入 unraisable hook 流程。
  - 为 builtin-value subclass 的 `iter()` / `reversed()` 迭代器保活 owner，避免迭代期间底层对象被提前释放。

- `54c6ac1 fix: support iterator setstate semantics`
  - 基础 iterator 按 CPython 语义暴露 `__setstate__`，覆盖 list/tuple/str iterator 和旧序列协议 `SeqIter`。
  - `__setstate__` 负 state clamp 到 0，越界 state clamp 到耗尽位置，已耗尽 iterator 保持 sink-state。
  - 旧序列协议 iterator 在接近 `sys.maxsize` 时抛 `OverflowError: iter index too large`，避免 index wrap。
  - `next()` 对 VM lazy iterator fallback 到 VM 级推进，直接 `next(iter(seq_protocol_obj))` 不再被 core-only path 拒绝。
  - VM `iter(set)` 改回 `VecIter` 快照迭代，避免 set iterator 错误继承 list iterator 的 `__setstate__`。
  - `tools/run_cpython_tests.py` 支持 dotted 单例选择器，便于单项探测。

- `c323207 fix: release exhausted sequence iterables`
  - 旧序列协议 `SeqIter` 在真实耗尽后释放源对象强引用，允许 iterable 被 GC/finalizer 回收。
  - 保持 exhausted sink-state，重复 `next()` 不再重新访问源对象。

## 本轮修复成果

- 2026-05-25 追加：
  - 基础 iterator 按 CPython 语义暴露 `__setstate__`，覆盖 list/tuple/str iterator 和旧序列协议 `SeqIter`。
  - `__setstate__` 负 state clamp 到 0，越界 state clamp 到耗尽位置，已耗尽 iterator 保持 sink-state。
  - 旧序列协议 iterator 在接近 `sys.maxsize` 时抛 `OverflowError: iter index too large`，避免 index wrap。
  - `next()` 对 VM lazy iterator fallback 到 VM 级推进，直接 `next(iter(seq_protocol_obj))` 不再被 core-only path 拒绝。
  - VM `iter(set)` 改回 `VecIter` 快照迭代，避免 set iterator 错误继承 list iterator 的 `__setstate__`。
  - `tools/run_cpython_tests.py` 支持 dotted 单例选择器，如 `test_iter.TestCase.test_iter_neg_setstate`，便于单项探测。
  - 旧序列协议 `SeqIter` 在耗尽后释放源对象强引用，并保持 sink-state，修复 exhausted iterator 不释放 iterable 的兼容问题。
  - list reverse iterator 改成持有源 list 的 `RevRefIter`，避免 `reversed(list)` 先复制快照导致 pickle 共享引用断开。
  - pickle 支持 list `RefIter` / `RevRefIter` 内部 reduce，保留 source/index/exhausted 状态；真实耗尽后才进入 sink-state。
  - dict view payload 改为携带可选 owner，dict subclass 的 view/iterator 不再提前释放底层对象。
  - cycle GC 遍历 dict/set keys、dict view、iterator、VecIter、RefIter/RevRefIter 和 IteratorData 内部引用，并追踪 Set。
  - cycle GC 清理 Instance/List/Dict/Set，并在 Instance 被 cycle GC 清理后让 weakref 立即失效。
  - 修复 `test_dict.DictTest.test_container_iterator` 与 `test_set.TestSet.test_container_iterator`。
  - `copy` 模块切到 pure-Python protocol 实现，补齐 copyreg/reduce/state/slot/bound method/weakref ref 等通用 copy 语义。
  - `dict.get()` 快路径支持非 str/int/bool hashable key fallback，`copyreg.dispatch_table.get(cls)` 不再误返回 default。
  - 移除核心层缺失 `__copy__` / `__deepcopy__` 的假 builtin placeholder，协议探测按 AttributeError/default 正常工作。
  - bound method 暴露 `__self__` / `__func__`，weakref.ref 添加 `__weakref_ref__` 标记用于 atomic copy/deepcopy。

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
- `WeakValueDictionary` 改为保留原始 key 对象与 hashable key，避免用字符串化 key 破坏 identity 和碰撞语义。
- `WeakKeyDictionary` / `WeakValueDictionary` 补齐 `items()` / `keys()` / `values()` / `get()` / `__delitem__` / equality 基础语义，支撑 copy/deepcopy 复制弱引用映射。
- `copy.py` 对 weak key/value dict 走专用路径：WeakKeyDictionary deepcopy 保留 key、deepcopy value；WeakValueDictionary deepcopy key、保留 value。
- VM 不再为每次 `call_object()` 无条件生成 `f_locals` 当前帧快照，避免 `weakref.ref()()`、weakdict `len()` 等路径被临时 frame snapshot 拖延对象释放；`sys._getframe` 和 trace/profile 仍按需生成。
- cycle GC 对 trial deletion 候选集二次按候选内部入边收敛，避免活 list/dict 持有的普通 instance 被误判成循环垃圾并清空 `__dict__`。

## 已通过验证

- `cargo build --release -p ferrython-cli --bin ferrython -j6`
- `target/release/ferrython tools/run_cpython_tests.py -v test_bytes`
  - `run=264 pass=264 fail=0 err=0 skip=0`
- `target/release/ferrython tools/run_cpython_tests.py -v test_exceptions test_grammar test_compile test_print`
- 本轮按用户要求优先使用 debug/dev 构建加快修复迭代：
  - `cargo build -p ferrython-cli --bin ferrython -j6`
  - 最近一次 dev build: `Finished dev profile [optimized + debuginfo] target(s) in 1m 26s`
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
  - `test_iter.TestCase.test_iter_neg_setstate`
  - `test_iter.TestCase.test_iter_overflow` (skip=1, CPython-only)
  - `test_iter.TestCase.test_sinkstate_sequence`
  - `test_iter.TestCase.test_sinkstate_list`
  - `test_iter.TestCase.test_sinkstate_dict`
  - `test_iter.TestCase.test_free_after_iterating`
  - `test_iter.TestCase.test_error_iter`
  - `test_iter.TestCase.test_mutating_seq_class_exhausted_iter`
  - `test_list.ListTest.test_iterator_pickle`
  - `test_list.ListTest.test_reversed_pickle`
  - `test_tuple.TupleTest.test_iterator_pickle`
  - `test_tuple.TupleTest.test_reversed_pickle`
  - `test_dict.DictTest.test_container_iterator`
  - `test_set.TestSet.test_container_iterator`
  - `test_dict.DictTest.test_free_after_iterating`
  - `test_set.TestSet.test_free_after_iterating`
- 本轮 smoke 通过：
  - `hasattr(iter({1:2}), "__setstate__") == False`
  - `hasattr(iter({1,2}), "__setstate__") == False`
  - `hasattr(iter([1]), "__setstate__") == True`
  - `hasattr(iter((1,)), "__setstate__") == True`
  - 旧序列 iterator `__setstate__(sys.maxsize - 2)` 后第三次 `next()` 抛 `OverflowError`
  - 旧序列协议 iterator 耗尽后 `gc.collect()` 触发源对象 `__del__`
  - `pickle.dumps((reversed(l), l), 0)` 中 reverse iterator source 与原 list 共享 memo `g0`
  - CPython baseline 对比：`reversed(list)` 在列表变短/追加场景与 Ferrython 当前行为一致
  - dict view cycle smoke: `dict.keys` / `dict.values` / `dict.items` 经 `gc.collect()` 后 weakref 返回 `None`
  - set iterator cycle smoke: `iter(set([obj, 1]))` 经 `gc.collect()` 后 weakref 返回 `None`
  - weakref smoke: 普通 alive/dead 对象 weakref 正常，self-cycle 经 `gc.collect()` 后 weakref 返回 `None`
  - copy smoke: `dict.get(class_key)`、weakref ref copy/deepcopy identity、bound method deepcopy rebind 均通过。
- `test_iter` 单项扫描当前已推进到：
  - `target/debug/ferrython tools/run_cpython_tests.py -v test_iter`
  - `run=54 pass=52 fail=0 err=0 skip=2`，模块级通过，耗时 1.11s
  - 下一个候选：转向 `test_dict` / `test_set` / `test_copy` 等小批单项扫描

- list/tuple iterator pickle focused 验证：
  - `target/debug/ferrython tools/run_cpython_tests.py -v test_list.ListTest.test_iterator_pickle test_list.ListTest.test_reversed_pickle`
    - `run=2 pass=2 fail=0 err=0 skip=0`
  - `target/debug/ferrython tools/run_cpython_tests.py -v test_tuple.TupleTest.test_iterator_pickle test_tuple.TupleTest.test_reversed_pickle`
    - `run=2 pass=2 fail=0 err=0 skip=0`

- dict/set iterator cycle focused 验证：
  - `target/debug/ferrython tools/run_cpython_tests.py -v test_dict.DictTest.test_container_iterator test_set.TestSet.test_container_iterator test_dict.DictTest.test_free_after_iterating test_set.TestSet.test_free_after_iterating`
    - `run=4 pass=4 fail=0 err=0 skip=0`
  - `target/debug/ferrython tools/run_cpython_tests.py -v test_iter`
    - `run=54 pass=52 fail=0 err=0 skip=2`

- copy protocol focused 验证：
  - `target/debug/ferrython tools/run_cpython_tests.py -v test_copy.TestCopy.test_exceptions test_copy.TestCopy.test_copy_atomic test_copy.TestCopy.test_copy_bytearray test_copy.TestCopy.test_copy_tuple test_copy.TestCopy.test_copy_registry test_copy.TestCopy.test_deepcopy_registry test_copy.TestCopy.test_copy_reduce test_copy.TestCopy.test_copy_reduce_ex test_copy.TestCopy.test_deepcopy_reduce test_copy.TestCopy.test_deepcopy_reduce_ex test_copy.TestCopy.test_copy_cant test_copy.TestCopy.test_deepcopy_cant test_copy.TestCopy.test_copy_inst_getnewargs test_copy.TestCopy.test_copy_inst_getnewargs_ex test_copy.TestCopy.test_deepcopy_inst_getnewargs test_copy.TestCopy.test_deepcopy_inst_getnewargs_ex test_copy.TestCopy.test_copy_slots test_copy.TestCopy.test_deepcopy_slots test_copy.TestCopy.test_deepcopy_bound_method test_copy.TestCopy.test_copy_weakref test_copy.TestCopy.test_deepcopy_weakref`
    - `run=21 pass=21 fail=0 err=0 skip=0`
  - `target/debug/ferrython tools/run_cpython_tests.py -v test_copy.TestCopy.test_reconstruct_string test_copy.TestCopy.test_reconstruct_nostate test_copy.TestCopy.test_reconstruct_state test_copy.TestCopy.test_reconstruct_state_setstate test_copy.TestCopy.test_reconstruct_reflexive test_copy.TestCopy.test_copy_list_subclass test_copy.TestCopy.test_deepcopy_list_subclass test_copy.TestCopy.test_copy_tuple_subclass test_copy.TestCopy.test_deepcopy_tuple_subclass test_copy.TestCopy.test_deepcopy_dict_subclass`
    - `run=10 pass=10 fail=0 err=0 skip=0`
  - 回归：
    - `target/debug/ferrython tools/run_cpython_tests.py -v test_iter.TestCase.test_iter_neg_setstate test_iter.TestCase.test_sinkstate_list test_dict.DictTest.test_container_iterator test_set.TestSet.test_container_iterator`
      - `run=4 pass=4 fail=0 err=0 skip=0`
  - weakdict copy/deepcopy:
    - `target/debug/ferrython tools/run_cpython_tests.py -v test_copy.TestCopy.test_copy_weakkeydict test_copy.TestCopy.test_copy_weakvaluedict test_copy.TestCopy.test_deepcopy_weakkeydict test_copy.TestCopy.test_deepcopy_weakvaluedict`
      - `run=4 pass=4 fail=0 err=0 skip=0`
  - weakref/copy 回归：
    - `target/debug/ferrython tools/run_cpython_tests.py -v test_copy.TestCopy.test_copy_weakref test_copy.TestCopy.test_deepcopy_weakref test_copy.TestCopy.test_deepcopy_bound_method`
      - `run=3 pass=3 fail=0 err=0 skip=0`
  - GC/runner smoke:
    - `weakref.ref()()` 不再提高 referent 持久 refcount，`del o` 后第一次 `r()` 返回 `None`。
    - weakdict copy 后 `del c,d`，第一次 `len(v)` 即为 `1`。
    - self-cycle instance 经 `gc.collect()` 后 weakref 返回 `None`。
    - runner `ModuleReport` 保存在活 list 中时不再被 cycle GC 清空 `__dict__`。

## 当前工作树

- 本轮代码修复涉及 weakdict copy/deepcopy、WeakValueDictionary key identity、VM 当前帧快照保活、cycle GC candidate refinement，以及 copy protocol weakdict 专用路径。
- 未跟踪项：`.codex-work/`，保留为本地工作资料，不纳入提交。

## 当前修复候选

- `test_copy` 已关闭 weakref ref、bound method、weak key/value dict copy/deepcopy；下一步优先继续扫描 weakref/deque 小批候选。
  - 方向：优先找不需要全量测试的单例失败；遇到长耗时 case 记录并跳过。
  - 已知残留：
    - `test_copy.TestCopy.test_deepcopy_range` 仍受 RangeData 只保存 i64、无法保留 int subclass endpoint 限制影响。

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
- `test_iter.TestCase.test_iter_neg_setstate`
  - 修复：基础 iterator `__setstate__` 支持负 state 归零，并保留耗尽 sink-state。
- `test_iter.TestCase.test_free_after_iterating`
  - 修复：旧序列协议 `SeqIter` 耗尽后释放源对象强引用，允许源对象 `__del__` 在 GC 后运行。
- `test_list.ListTest.test_iterator_pickle` / `test_list.ListTest.test_reversed_pickle`
  - 修复：list 正向/反向 iterator pickle 保留源 list 共享引用、当前位置和耗尽状态；`reversed(list)` 不再复制临时 list。
- `test_tuple.TupleTest.test_iterator_pickle` / `test_tuple.TupleTest.test_reversed_pickle`
  - 验证：tuple iterator pickle 行为未被 list iterator 修复回归。
- `test_dict.DictTest.test_container_iterator`
  - 修复：dict view/iterator 纳入 cycle GC 遍历，HashableKey 中保存的 instance key 也计入内部引用，cycle GC 后 weakref 立即失效。
- `test_set.TestSet.test_container_iterator`
  - 修复：set 纳入 cycle GC tracking，set storage 中 identity/custom key 引用参与 cycle 判断和清理。
- `test_dict.DictTest.test_free_after_iterating` / `test_set.TestSet.test_free_after_iterating`
  - 验证：dict/set iterator owner 生命周期修复未回归。
- `test_copy` copy protocol 批次：
  - 修复：`copy` 模块使用 pure-Python protocol 实现；copyreg registry、reduce/reconstruct、state/slotstate、`__getnewargs__`/`__getnewargs_ex__`、实例 fallback、weakref ref identity、bound method deepcopy rebind 等通过 focused 验证。
  - 修复：`dict.get()` 支持 class/custom hashable key fallback，`copyreg.dispatch_table.get(cls)` 正常命中。
  - 修复：核心属性层不再暴露缺失的 `__copy__` / `__deepcopy__` 假方法。
- `test_copy.TestCopy.test_copy_weakkeydict` / `test_copy.TestCopy.test_copy_weakvaluedict`
  - 修复：weakdict copy 专用路径复制活 `(key, value)` item，底层容器 decouple，死 weak key/value 首次 `len()` 即清理。
- `test_copy.TestCopy.test_deepcopy_weakkeydict` / `test_copy.TestCopy.test_deepcopy_weakvaluedict`
  - 修复：WeakKeyDictionary deepcopy 保留 key/deepcopy value；WeakValueDictionary deepcopy key/保留 value，并保持 weak mapping equality 与 item identity。
- runner `ModuleReport` 被 GC 清空属性
  - 修复：cycle GC candidate refinement 不再把活 list 持有的普通 instance 当作循环垃圾。

## 修复原则

- 不硬编码 CPython test case；所有改动落在通用语义：
  - pickle 全局类解析和 memo 语义。
  - iterator protocol / old sequence protocol。
  - module-backed file iterator。
  - sink-state / stateful iterator 行为。
- 候选通用修复方向：
  - 对 `__setstate__` 等 pickle state API 做通用 iterator 支持。

## 后续修复队列

1. `test_iter` 当前模块级已过，list/tuple iterator pickle focused 已过，dict/set iterator cycle focused 已过；继续按 case 扫描 copy/weakref/deque 小批队列，找出后续失败或 stack overflow 的真实触发用例。
2. 保持 dotted 单例 runner 用法，避免长跑全量测试。
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
