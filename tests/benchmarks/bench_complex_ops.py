# Ferrython Complex Workload Benchmarks
# Purpose: exercise realistic mixed operations rather than isolated micro paths.
#
# Run with:
#   target/release/ferrython tests/benchmarks/bench_complex_ops.py
#   python3 tests/benchmarks/bench_complex_ops.py

import time


def bench(name, fn, rounds):
    best = float("inf")
    best_result = None
    for _ in range(3):
        start = time.time()
        result = fn(rounds)
        elapsed = time.time() - start
        if elapsed < best:
            best = elapsed
            best_result = result
    ops_per_sec = rounds / best if best > 0 else 0
    print(f"  {name:42s}  {best:.4f}s  ({ops_per_sec:.0f} rounds/s)  checksum={best_result}")


class Key:
    def __init__(self, group, value):
        self.group = group
        self.value = value

    def __hash__(self):
        return self.group * 1000003 + self.value

    def __eq__(self, other):
        try:
            return self.group == other.group and self.value == other.value
        except AttributeError:
            return False


class CounterBox:
    def __init__(self, base):
        self.base = base
        self.total = 0
        self.last = base

    def bump(self, value):
        self.last = self.base + value
        self.total += self.last
        return self.total


def bench_dynamic_str_dict(rounds):
    total = 0
    for r in range(rounds):
        d = {}
        prefix = "row:" + str(r) + ":"
        for i in range(200):
            d[prefix + str(i)] = i + r
        for i in range(200):
            total += d[prefix + str(i)]
    return total


def bench_nested_collections(rounds):
    total = 0
    for r in range(rounds):
        rows = []
        for outer in range(30):
            row = {}
            for inner in range(12):
                key = "f" + str(outer) + ":" + str(inner + r)
                row[key] = [outer, inner, outer + inner + r]
            rows.append(row)
        probe = "f" + str(r % 30) + ":" + str((r % 12) + r)
        for row in rows:
            values = row.get(probe)
            if values is not None:
                total += values[2]
            total += len(row)
    return total


def bench_int_dict_update(rounds):
    total = 0
    d = {i: i for i in range(1024)}
    for r in range(rounds):
        base = r * 17
        for i in range(256):
            key = (base + i) % 2048
            d[key] = d.get(key, 0) + i
        for i in range(256):
            total += d.get((base + i * 3) % 2048, -1)
    return total + len(d)


def bench_custom_key_dict(rounds):
    total = 0
    keys = [Key(i % 31, i) for i in range(300)]
    d = {}
    for key in keys:
        d[key] = key.value
    for r in range(rounds):
        offset = r % 300
        for i in range(120):
            value = (offset + i) % 300
            total += d.get(Key(value % 31, value), 0)
    return total + len(d)


def bench_int_set_churn(rounds):
    total = 0
    s = set(range(512))
    for r in range(rounds):
        base = r * 13
        for i in range(240):
            value = (base + i) % 2048
            s.add(value)
            if value - 7 in s:
                s.discard(value - 7)
            if value in s:
                total += 1
    return total + len(s)


def bench_custom_set_lookup(rounds):
    total = 0
    s = set()
    for i in range(256):
        s.add(Key(i % 19, i))
    for r in range(rounds):
        base = r % 256
        for i in range(128):
            value = (base + i) % 256
            if Key(value % 19, value) in s:
                total += value
    return total + len(s)


def bench_object_method_attrs(rounds):
    total = 0
    boxes = [CounterBox(i) for i in range(64)]
    for r in range(rounds):
        for i, box in enumerate(boxes):
            total += box.bump(r + i)
            box.extra = box.last + box.base
            total += box.extra
    return total


def bench_iterator_pipeline(rounds):
    total = 0
    data = [list(range(i, i + 40)) for i in range(0, 240, 40)]
    for r in range(rounds):
        for idx, row in enumerate(data):
            shifted = list(map(lambda x: x + idx + r, row))
            filtered = [x for x in shifted if x % 3 == 0]
            for left, right in zip(filtered, filtered[1:]):
                total += right - left
    return total


def bench_string_processing(rounds):
    total = 0
    base = "alpha,beta,gamma,delta,epsilon,zeta,eta,theta"
    for r in range(rounds):
        for i in range(80):
            text = base + "," + str(r) + "," + str(i)
            parts = text.split(",")
            parts[1] = parts[1].replace("e", "E")
            joined = "|".join(parts[1:7])
            total += len(joined[2:24])
    return total


def bench_record_index(rounds):
    total = 0
    records = []
    for i in range(180):
        records.append({"group": i % 12, "name": "name_" + str(i), "score": i * 3})
    for r in range(rounds):
        index = {}
        for record in records:
            group = (record["group"] + r) % 12
            bucket = index.setdefault(group, [])
            bucket.append(record["name"])
        for group, names in index.items():
            total += group + len(names)
    return total


print("=" * 76)
print("Ferrython Complex Workload Benchmarks")
print("=" * 76)
print()

bench("dynamic_str_dict insert+lookup", bench_dynamic_str_dict, 90)
bench("nested_collection build+probe", bench_nested_collections, 70)
bench("int_dict update+miss/hit", bench_int_dict_update, 160)
bench("custom_key_dict eq/hash lookup", bench_custom_key_dict, 120)
bench("int_set add/discard/membership", bench_int_set_churn, 160)
bench("custom_set eq/hash membership", bench_custom_set_lookup, 140)
bench("object method+attr churn", bench_object_method_attrs, 160)
bench("iterator map/filter/zip pipeline", bench_iterator_pipeline, 140)
bench("string split/replace/join/slice", bench_string_processing, 120)
bench("record indexing with setdefault", bench_record_index, 120)

print()
print("=" * 76)
print("Done.")
