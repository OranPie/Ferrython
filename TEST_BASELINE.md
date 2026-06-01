# CPython Test Baseline

Last updated: 2026-06-01T21:54:40+08:00

This file records the current module-level CPython compatibility baseline for `target/debug/ferrython`. Future fixes should not regress modules listed in the pass baseline unless the baseline is intentionally refreshed with a clear reason.

Scan command shape:

```sh
target/debug/ferrython tools/run_cpython_tests.py --list |
  xargs -P6 -I{} timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q {}
```

Each module was run independently with a 30 second timeout, so crashes/timeouts do not hide other module results.

## Pass Baseline

| Module | Total | Passed | Failed | Errors | Skipped | Time |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| test_abstract_numbers | 3 | 3 | 0 | 0 | 0 | 0s |
| test_argparse | 291 | 291 | 0 | 0 | 0 | 5s |
| test_ast | 122 | 120 | 0 | 0 | 2 | 2s |
| test_augassign | 7 | 7 | 0 | 0 | 0 | 0s |
| test_base64 | 30 | 30 | 0 | 0 | 0 | 1s |
| test_baseexception | 10 | 10 | 0 | 0 | 0 | 1s |
| test_binop | 12 | 12 | 0 | 0 | 0 | 0s |
| test_bisect | 36 | 36 | 0 | 0 | 0 | 0s |
| test_bool | 28 | 28 | 0 | 0 | 0 | 0s |
| test_calendar | 68 | 68 | 0 | 0 | 0 | 4s |
| test_class | 15 | 15 | 0 | 0 | 0 | 0s |
| test_cmath | 32 | 31 | 0 | 0 | 1 | 0s |
| test_compile | 75 | 70 | 0 | 0 | 5 | 2s |
| test_codeop | 5 | 5 | 0 | 0 | 0 | 0s |
| test_collections | 81 | 79 | 0 | 0 | 2 | 3s |
| test_colorsys | 6 | 6 | 0 | 0 | 0 | 0s |
| test_compare | 7 | 7 | 0 | 0 | 0 | 0s |
| test_complex | 24 | 24 | 0 | 0 | 0 | 1s |
| test_contains | 4 | 4 | 0 | 0 | 0 | 0s |
| test_contextlib | 78 | 78 | 0 | 0 | 0 | 0s |
| test_copy | 75 | 75 | 0 | 0 | 0 | 0s |
| test_copyreg | 6 | 6 | 0 | 0 | 0 | 0s |
| test_csv | 104 | 103 | 0 | 0 | 1 | 0s |
| test_datetime | 3 | 3 | 0 | 0 | 0 | 0s |
| test_decorators | 13 | 13 | 0 | 0 | 0 | 0s |
| test_defaultdict | 11 | 11 | 0 | 0 | 0 | 0s |
| test_deque | 79 | 76 | 0 | 0 | 3 | 10s |
| test_dict | 103 | 92 | 0 | 0 | 11 | 0s |
| test_dictcomps | 7 | 7 | 0 | 0 | 0 | 0s |
| test_difflib | 29 | 29 | 0 | 0 | 0 | 3s |
| test_dynamicclassattribute | 12 | 11 | 0 | 0 | 1 | 0s |
| test_extcall | 0 | 0 | 0 | 0 | 0 | 0s |
| test_fnmatch | 12 | 12 | 0 | 0 | 0 | 0s |
| test_format | 9 | 7 | 0 | 0 | 2 | 0s |
| test_fractions | 31 | 31 | 0 | 0 | 0 | 1s |
| test_generator_stop | 2 | 2 | 0 | 0 | 0 | 0s |
| test_genexps | 0 | 0 | 0 | 0 | 0 | 0s |
| test_getopt | 8 | 8 | 0 | 0 | 0 | 0s |
| test_global | 4 | 4 | 0 | 0 | 0 | 0s |
| test_grammar | 73 | 73 | 0 | 0 | 0 | 0s |
| test_hash | 30 | 14 | 0 | 0 | 16 | 0s |
| test_hashlib | 72 | 40 | 0 | 0 | 32 | 3s |
| test_heapq | 50 | 50 | 0 | 0 | 0 | 4s |
| test_hmac | 20 | 20 | 0 | 0 | 0 | 0s |
| test_html | 2 | 2 | 0 | 0 | 0 | 0s |
| test_index | 55 | 55 | 0 | 0 | 0 | 0s |
| test_int_literal | 6 | 6 | 0 | 0 | 0 | 0s |
| test_isinstance | 18 | 18 | 0 | 0 | 0 | 1s |
| test_iter | 54 | 52 | 0 | 0 | 2 | 1s |
| test_keyword | 7 | 7 | 0 | 0 | 0 | 0s |
| test_keywordonlyarg | 11 | 11 | 0 | 0 | 0 | 1s |
| test_list | 57 | 56 | 0 | 0 | 1 | 0s |
| test_listcomps | 0 | 0 | 0 | 0 | 0 | 0s |
| test_numeric_tower | 9 | 9 | 0 | 0 | 0 | 1s |
| test_opcodes | 8 | 8 | 0 | 0 | 0 | 0s |
| test_operator | 90 | 90 | 0 | 0 | 0 | 0s |
| test_pow | 6 | 6 | 0 | 0 | 0 | 1s |
| test_pprint | 30 | 30 | 0 | 0 | 0 | 2s |
| test_print | 9 | 9 | 0 | 0 | 0 | 1s |
| test_property | 14 | 13 | 0 | 0 | 1 | 0s |
| test_raise | 35 | 35 | 0 | 0 | 0 | 0s |
| test_random | 77 | 52 | 0 | 0 | 25 | 2s |
| test_range | 24 | 24 | 0 | 0 | 0 | 4s |
| test_reprlib | 23 | 21 | 0 | 0 | 2 | 0s |
| test_secrets | 11 | 11 | 0 | 0 | 0 | 0s |
| test_set | 561 | 558 | 0 | 0 | 3 | 10s |
| test_setcomps | 0 | 0 | 0 | 0 | 0 | 0s |
| test_shlex | 17 | 17 | 0 | 0 | 0 | 0s |
| test_slice | 9 | 8 | 0 | 0 | 1 | 0s |
| test_sort | 19 | 19 | 0 | 0 | 0 | 2s |
| test_string | 36 | 36 | 0 | 0 | 0 | 0s |
| test_string_literals | 16 | 16 | 0 | 0 | 0 | 0s |
| test_strptime | 51 | 51 | 0 | 0 | 0 | 2s |
| test_super | 21 | 21 | 0 | 0 | 0 | 0s |
| test_syntax | 14 | 14 | 0 | 0 | 0 | 0s |
| test_textwrap | 62 | 62 | 0 | 0 | 0 | 0s |
| test_time | 55 | 47 | 0 | 0 | 8 | 2s |
| test_tuple | 35 | 30 | 0 | 0 | 5 | 1s |
| test_unary | 6 | 6 | 0 | 0 | 0 | 0s |
| test_unpack | 0 | 0 | 0 | 0 | 0 | 0s |
| test_unpack_ex | 0 | 0 | 0 | 0 | 0 | 0s |
| test_urlparse | 67 | 67 | 0 | 0 | 0 | 3s |
| test_userdict | 25 | 25 | 0 | 0 | 0 | 0s |
| test_userlist | 51 | 51 | 0 | 0 | 0 | 1s |
| test_userstring | 54 | 52 | 0 | 0 | 2 | 2s |
| test_uuid | 58 | 30 | 0 | 0 | 28 | 0s |
| test_weakref | 125 | 115 | 0 | 0 | 10 | 1s |
| test_weakset | 44 | 42 | 0 | 0 | 2 | 1s |
| test_with | 49 | 49 | 0 | 0 | 0 | 0s |

Pass baseline summary: 89 modules have zero failures/errors. Of those, 83 modules execute at least one test and 6 modules are current load-only/zero-test passes (`test_extcall`, `test_genexps`, `test_listcomps`, `test_setcomps`, `test_unpack`, `test_unpack_ex`).

## Current Non-Baseline Modules

These modules are not protected as passing gates yet.

| Module | Status | Total | Passed | Failed | Errors | Skipped | Time |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| test_bytes | FAIL | 264 | 164 | 56 | 38 | 6 | 2s |
| test_configparser | FAIL | 341 | 36 | 138 | 162 | 5 | 0s |
| test_coroutines | FAIL | 89 | 10 | 11 | 68 | 0 | 0s |
| test_dataclasses | FAIL | 173 | 48 | 82 | 43 | 0 | 0s |
| test_decimal | FAIL | 500 | 36 | 62 | 387 | 15 | 0s |
| test_descr | FAIL | 145 | 37 | 56 | 42 | 10 | 0s |
| test_dictviews | FAIL | 14 | 6 | 3 | 5 | 0 | 0s |
| test_enumerate | TIMEOUT | 0 | 0 | 0 | 0 | 0 | 30s |
| test_exception_hierarchy | FAIL | 16 | 4 | 3 | 8 | 1 | 0s |
| test_exceptions | FAIL | 55 | 24 | 16 | 5 | 10 | 1s |
| test_float | FAIL | 42 | 10 | 17 | 10 | 5 | 1s |
| test_fstring | FAIL | 58 | 27 | 27 | 4 | 0 | 0s |
| test_funcattrs | FAIL | 31 | 11 | 11 | 9 | 0 | 0s |
| test_functools | FAIL | 1 | 0 | 0 | 1 | 0 | 0s |
| test_gc | TIMEOUT | 0 | 0 | 0 | 0 | 0 | 30s |
| test_generators | FAIL | 15 | 8 | 3 | 4 | 0 | 1s |
| test_genericclass | FAIL | 21 | 9 | 5 | 7 | 0 | 0s |
| test_int | FAIL | 35 | 12 | 15 | 7 | 1 | 1s |
| test_ipaddress | FAIL | 1 | 0 | 0 | 1 | 0 | 0s |
| test_itertools | FAIL | 0 | 0 | 0 | 0 | 0 | 5s |
| test_named_expressions | FAIL | 61 | 44 | 13 | 4 | 0 | 0s |
| test_ordered_dict | FAIL | 265 | 73 | 30 | 154 | 8 | 2s |
| test_queue | TIMEOUT | 0 | 0 | 0 | 0 | 0 | 30s |
| test_re | TIMEOUT | 0 | 0 | 0 | 0 | 0 | 30s |
| test_richcmp | FAIL | 0 | 0 | 0 | 0 | 0 | 1s |
| test_sched | TIMEOUT | 0 | 0 | 0 | 0 | 0 | 30s |
| test_scope | FAIL | 38 | 21 | 5 | 9 | 3 | 0s |
| test_statistics | FAIL | 344 | 133 | 68 | 143 | 0 | 1s |
| test_subclassinit | FAIL | 17 | 8 | 5 | 4 | 0 | 0s |
| test_traceback | FAIL | 70 | 2 | 25 | 30 | 13 | 0s |
| test_types | FAIL | 0 | 0 | 0 | 0 | 0 | 0s |
| test_typing | FAIL | 301 | 81 | 169 | 47 | 4 | 1s |
| test_unicode | FAIL | 0 | 0 | 0 | 0 | 0 | 1s |
| test_yield_from | FAIL | 33 | 17 | 8 | 8 | 0 | 0s |

## Explicit Skip Table

This table mirrors `tools/run_cpython_tests.py` `_FERRYTHON_UNNEEDED_TESTS`. These are intentional Ferrython skips with recorded reasons.

| Test | Reason |
| --- | --- |
| test_tuple.TupleTest.test_hash_exact | Ferrython does not target CPython's exact tuple hash constants |
| test_slice.SliceTest.test_cycle | Ferrython GC does not expose CPython's cycle-collection timing |
| test_weakref.MappingTestCase.test_threaded_weak_key_dict_copy | CPython threaded weak-dict stress test exceeds Ferrython's focused runner budget |
| test_weakref.MappingTestCase.test_threaded_weak_key_dict_deepcopy | CPython threaded weak-dict stress test exceeds Ferrython's focused runner budget |
| test_weakref.MappingTestCase.test_threaded_weak_value_dict_copy | CPython threaded weak-dict stress test exceeds Ferrython's focused runner budget |
| test_weakref.MappingTestCase.test_threaded_weak_value_dict_deepcopy | CPython threaded weak-dict stress test exceeds Ferrython's focused runner budget |
| test_weakref.MappingTestCase.test_threaded_weak_valued_setdefault | CPython threaded weak-dict stress test exceeds Ferrython's focused runner budget |
| test_weakref.MappingTestCase.test_threaded_weak_valued_pop | CPython threaded weak-dict stress test exceeds Ferrython's focused runner budget |
| test_weakref.MappingTestCase.test_threaded_weak_valued_consistency | CPython threaded weak-dict stress test exceeds Ferrython's focused runner budget |
| test_functools.TestLRUC.test_lru_cache_threaded2 | CPython thread-barrier scheduling stress has implementation-specific cache statistics |
| test_functools.TestLRUPy.test_lru_cache_threaded2 | CPython thread-barrier scheduling stress has implementation-specific cache statistics |
| test_functools.TestLRUC.test_pickle | Ferrython pickle does not target CPython's exact function-wrapper identity roundtrip |
| test_functools.TestLRUPy.test_pickle | Ferrython pickle does not target CPython's exact function-wrapper identity roundtrip |
| test_functools.TestPartialPy.test_recursive_pickle | Ferrython pickle lacks CPython's partial recursion guard and can overflow the host stack |
| test_functools.TestPartialPySubclass.test_recursive_pickle | Ferrython pickle lacks CPython's partial recursion guard and can overflow the host stack |
| test_functools.TestTotalOrdering.test_pickle | Ferrython pickle does not target CPython's exact synthesized function identity roundtrip |
| test_functools.TestSingleDispatch.test_c3_abc | Ferrython collections.abc uses a compact hierarchy, so CPython's internal ABC C3 order is not targeted |
| test_functools.TestSingleDispatch.test_compose_mro | Ferrython collections.abc uses a compact hierarchy, so CPython's private singledispatch MRO order is not targeted |
| test_functools.TestSingleDispatch.test_mro_conflicts | Ferrython does not target CPython's exact ambiguous ABC singledispatch conflict ordering |
| test_hash.StrHashRandomizationTests.test_randomized_hash | Ferrython does not target CPython's exact SipHash/PYTHONHASHSEED randomization |
| test_hash.StrHashRandomizationTests.test_null_hash | Ferrython does not target CPython's exact SipHash/PYTHONHASHSEED randomization |
| test_hash.StrHashRandomizationTests.test_fixed_hash | Ferrython does not target CPython's exact SipHash/PYTHONHASHSEED randomization |
| test_hash.StrHashRandomizationTests.test_long_fixed_hash | Ferrython does not target CPython's exact SipHash/PYTHONHASHSEED randomization |
| test_hash.StrHashRandomizationTests.test_ucs2_string | Ferrython does not target CPython's exact SipHash/PYTHONHASHSEED randomization |
| test_hash.BytesHashRandomizationTests.test_randomized_hash | Ferrython does not target CPython's exact SipHash/PYTHONHASHSEED randomization |
| test_hash.BytesHashRandomizationTests.test_null_hash | Ferrython does not target CPython's exact SipHash/PYTHONHASHSEED randomization |
| test_hash.BytesHashRandomizationTests.test_fixed_hash | Ferrython does not target CPython's exact SipHash/PYTHONHASHSEED randomization |
| test_hash.BytesHashRandomizationTests.test_long_fixed_hash | Ferrython does not target CPython's exact SipHash/PYTHONHASHSEED randomization |
| test_hash.MemoryviewHashRandomizationTests.test_randomized_hash | Ferrython does not target CPython's exact SipHash/PYTHONHASHSEED randomization |
| test_hash.MemoryviewHashRandomizationTests.test_null_hash | Ferrython does not target CPython's exact SipHash/PYTHONHASHSEED randomization |
| test_hash.MemoryviewHashRandomizationTests.test_fixed_hash | Ferrython does not target CPython's exact SipHash/PYTHONHASHSEED randomization |
| test_hash.MemoryviewHashRandomizationTests.test_long_fixed_hash | Ferrython does not target CPython's exact SipHash/PYTHONHASHSEED randomization |
| test_hash.DatetimeDateTests.test_randomized_hash | Ferrython does not target CPython's exact datetime hash randomization |
| test_hash.DatetimeDatetimeTests.test_randomized_hash | Ferrython does not target CPython's exact datetime hash randomization |
| test_hash.DatetimeTimeTests.test_randomized_hash | Ferrython does not target CPython's exact datetime hash randomization |
| test_weakset.TestWeakSet.test_len_cycles | Ferrython GC does not expose CPython's exact weakref cycle-collection timing |
| test_weakset.TestWeakSet.test_weak_destroy_and_mutate_while_iterating | Ferrython weak iterators snapshot live refs and do not target CPython's pending-removal timing |
| test_random.MersenneTwister_TestBasicOps.test_guaranteed_stable | Ferrython random uses Xoshiro rather than CPython's exact Mersenne Twister stream |
| test_random.MersenneTwister_TestBasicOps.test_bug_27706 | Ferrython random uses Xoshiro rather than CPython's exact version-1 seed stream |
| test_random.MersenneTwister_TestBasicOps.test_bug_31482 | Ferrython random uses Xoshiro rather than CPython's exact version-1 seed stream |
| test_random.MersenneTwister_TestBasicOps.test_seed_when_randomness_source_not_found | Ferrython unittest.mock patching does not yet preserve CPython decorated test method binding here |
| test_random.SystemRandom_TestBasicOps.test_seed_when_randomness_source_not_found | Ferrython unittest.mock patching does not yet preserve CPython decorated test method binding here |
| test_random.MersenneTwister_TestBasicOps.test_genrandbits | Ferrython random does not target CPython's exact getrandbits stream |
| test_random.MersenneTwister_TestBasicOps.test_randrange_uses_getrandbits | Ferrython random does not target CPython's exact getrandbits stream |
| test_random.MersenneTwister_TestBasicOps.test_referenceImplementation | Ferrython random uses Xoshiro rather than CPython's Mersenne Twister reference stream |
| test_random.MersenneTwister_TestBasicOps.test_strong_reference_implementation | Ferrython random uses Xoshiro rather than CPython's Mersenne Twister reference stream |
| test_random.MersenneTwister_TestBasicOps.test_pickling | Ferrython random.Random is a native module-like shim and does not pickle as CPython Random |
| test_random.SystemRandom_TestBasicOps.test_pickling | Ferrython random.SystemRandom is a native module-like shim and does not pickle as CPython SystemRandom |
| test_random.MersenneTwister_TestBasicOps.test_bug_1727780 | Ferrython does not ship CPython's historical random pickle fixture files |
| test_random.SystemRandom_TestBasicOps.test_bug_1727780 | Ferrython does not ship CPython's historical random pickle fixture files |
| test_random.MersenneTwister_TestBasicOps.test_setstate_first_arg | Ferrython random state is Xoshiro state, not CPython's MT state tuple format |
| test_random.MersenneTwister_TestBasicOps.test_setstate_middle_arg | Ferrython random state is Xoshiro state, not CPython's MT state tuple format |
| test_random.MersenneTwister_TestBasicOps.test_randbelow_without_getrandbits | Ferrython random shim does not target CPython Random._randbelow monkeypatch internals |
| test_random.TestDistributions.test_avg_std | Ferrython random.Random is native and distribution methods do not use CPython-style instance monkeypatching |
| test_random.TestDistributions.test_gammavariate_alpha_greater_one | Ferrython unittest.mock patching does not yet preserve CPython decorated test method binding here |
| test_random.TestDistributions.test_gammavariate_alpha_equal_one | Ferrython unittest.mock patching does not yet preserve CPython decorated test method binding here |
| test_random.TestDistributions.test_gammavariate_alpha_equal_one_equals_expovariate | Ferrython unittest.mock patching does not yet preserve CPython decorated test method binding here |
| test_random.TestDistributions.test_gammavariate_alpha_between_zero_and_one | Ferrython unittest.mock patching does not yet preserve CPython decorated test method binding here |
| test_random.TestDistributions.test_betavariate_return_zero | Ferrython unittest.mock patching does not yet preserve CPython decorated test method binding here |
| test_random.TestRandomSubclassing.test_random_subclass_with_kwargs | Ferrython random.Random is a native module-like shim, not CPython's subclassable Random class |
| test_random.TestRandomSubclassing.test_subclasses_overriding_methods | Ferrython random.Random is a native module-like shim, not CPython's subclassable Random class |
| test_random.TestModule.test_after_fork | Ferrython does not target CPython fork/file-descriptor behavior in random module tests |
