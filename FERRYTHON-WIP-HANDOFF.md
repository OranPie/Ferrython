# Ferrython WIP Handoff

Date: 2026-05-24

## Commits Created In This Cleanup

- `b53327c fix: handle escaped newlines in -c`
- `3d5fb2f stdlib: expand unittest test support`
- `4c62fb8 vm: improve runtime protocol semantics`
- `fbc6c27 stdlib: expand import and regex support`
- `fb55e9d math: improve numeric compatibility`

## Verified In This Pass

- `cargo test -p ferrython-cli --test cli_command`
- `cargo build -p ferrython-cli --bin ferrython`
- CLI `-c` escaped newline handling.
- Core/VM smoke for large shifts, `float.fromhex('0.c')`, instance equality, and sequence comparison.
- Stdlib smoke for `argparse`, `re.compile`, `sre_compile._generate_overlap_table`, `unicodedata.name`, and `unittest.mock.patch`.
- Math smoke for `comb`, `perm`, `ldexp`, `exp(inf)`, `modf(inf)`, `log2(2**1023)`, `prod` with `Decimal`, `Fraction` equality/isclose, `fsum`, and `pow(-inf, -3.0)`.

## Known Remaining Issues

- Full `target/debug/ferrython stdlib/Lib/test/test_math.py` did not reliably complete during this pass and should be rerun under an explicit timeout/profiler. Treat this as a performance risk, not a passing result.
- `bin`, `hex`, and `oct` still need a general BigInt `__index__` path instead of forcing `i64`.
- `math.hypot`, `math.dist`, and `math.remainder` need focused retesting against huge integer and huge float precision cases.
- CPython `math_testcases.txt` and `cmath_testcases.txt` are still missing. Use official data only; do not fabricate resource files.
- `test_re` still has semantic gaps around backrefs, lookaround, locale bytes matching, zero-width split/finditer behavior, bytearray match views, and scoped flag validation. See `.codex-work/test_re/NOTES.md`.

## Deliberately Not Committed

- `stdlib/Lib/test/re_tests.py` was an empty/minimal placeholder and should not be committed. It hides CPython test data gaps rather than implementing behavior.
- `.codex-work/` remains local working notes for compaction continuity.

## Constraints To Preserve

- Do not hardcode around individual CPython test samples.
- Prefer broad CPython-like semantics in runtime, `unittest`, and `test.support`.
- Keep performance overhead visible; use direct payload fast paths where they are correct.
- Do not revert unrelated dirty worktree changes.
