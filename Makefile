# Ferrython build targets
# Usage:
#   make release        - Standard release build (fast compile)
#   make pgo            - PGO-optimized build (slow compile, ~15% faster runtime)
#   make bench          - Run benchmark suite
#   make cpython-test   - Run CPython official regression tests
#   make clean          - Clean build artifacts

LLVM_PROFDATA := $(shell find $$(rustc --print sysroot) -name llvm-profdata 2>/dev/null | head -1)
PGO_DIR := /tmp/ferrython-pgo-data
BENCH := tests/benchmarks/bench_suite.py
BIN := ./target/release/ferrython

.PHONY: release pgo bench cpython-test cpython-test-verbose clean

release:
	cargo build --release

pgo: clean
	@echo "=== PGO Step 1/3: Instrumented build ==="
	RUSTFLAGS="-Cprofile-generate=$(PGO_DIR)" cargo build --release
	@echo "=== PGO Step 2/3: Collecting profile data ==="
	@rm -f $(PGO_DIR)/*.profraw
	$(BIN) $(BENCH) > /dev/null 2>&1
	$(BIN) $(BENCH) > /dev/null 2>&1
	$(BIN) $(BENCH) > /dev/null 2>&1
	@echo "=== PGO Step 3/3: Optimized rebuild ==="
	$(LLVM_PROFDATA) merge -o $(PGO_DIR)/merged.profdata $(PGO_DIR)/*.profraw
	cargo clean
	RUSTFLAGS="-Cprofile-use=$(PGO_DIR)/merged.profdata" cargo build --release
	@echo "=== PGO build complete ==="

bench:
	@echo "--- CPython ---"
	python3 $(BENCH)
	@echo "--- Ferrython ---"
	$(BIN) $(BENCH)

cpython-test: release
	@echo "=== CPython Compatibility Test Suite ==="
	$(BIN) tools/run_cpython_tests.py

cpython-test-verbose: release
	@echo "=== CPython Compatibility Test Suite (verbose) ==="
	$(BIN) tools/run_cpython_tests.py --verbose

clean:
	cargo clean
	rm -rf $(PGO_DIR)
