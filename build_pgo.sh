#!/bin/bash
# Profile-Guided Optimization build script for Ferrython
# Produces a binary optimized based on actual runtime behavior.
#
# Usage: ./build_pgo.sh
#
# Requires: rustup component add llvm-tools-preview
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"

PGO_DIR="/tmp/ferrython-pgo-data"
rm -rf "$PGO_DIR"
mkdir -p "$PGO_DIR"

echo "=== Step 1/3: Instrumented build ==="
RUSTFLAGS="-C target-cpu=native -Cprofile-generate=$PGO_DIR" \
  cargo build --release 2>&1 | tail -3

echo "=== Step 2/3: Training workloads ==="
# Run representative workloads to collect profile data
./target/release/ferrython tests/benchmarks/bench_suite.py 2>/dev/null || true
cat > /tmp/_pgo_train.py << 'PYEOF'
def fib(n):
    if n < 2: return n
    return fib(n-1) + fib(n-2)
for _ in range(50): fib(20)

s = 0
for i in range(100000): s += i

items = list(range(1000))
for _ in range(100):
    t = 0
    for x in items: t += x

d = {}
for i in range(1000): d[str(i)] = i
for i in range(1000): _ = d[str(i)]

class Pt:
    def __init__(s, x, y): s.x = x; s.y = y
    def dist(s): return (s.x**2 + s.y**2)**0.5
pts = [Pt(i, i+1) for i in range(500)]
for p in pts: p.dist()
PYEOF
./target/release/ferrython /tmp/_pgo_train.py 2>/dev/null
rm -f /tmp/_pgo_train.py

echo "=== Step 3/3: PGO-optimized build ==="
SYSROOT=$(rustc --print sysroot)
PROFDATA="$SYSROOT/lib/rustlib/x86_64-unknown-linux-gnu/bin/llvm-profdata"
"$PROFDATA" merge -o "$PGO_DIR/merged.profdata" "$PGO_DIR/"

RUSTFLAGS="-C target-cpu=native -Cprofile-use=$PGO_DIR/merged.profdata" \
  cargo build --release 2>&1 | tail -3

rm -rf "$PGO_DIR"
echo "=== Done! PGO-optimized binary at target/release/ferrython ==="
