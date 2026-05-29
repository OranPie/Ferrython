# Focused csv compatibility probes for dialects, quoting, field limits, and
# logical records spanning physical lines.

import csv
import io


# StringIO text split should preserve CRLF as one record ending.
buf = io.StringIO()
writer = csv.writer(buf)
writer.writerow(["a", "b"])
writer.writerow(["c", "d"])
buf.seek(0)
assert list(csv.reader(buf)) == [["a", "b"], ["c", "d"]]


# DictReader should share reader logical-record handling for quoted newlines.
rows = list(csv.DictReader(io.StringIO('name,notes\nAlice,"x\ny"\n')))
assert rows == [{"name": "Alice", "notes": "x\ny"}]


# QUOTE_NONNUMERIC writes based on original object type, not string contents.
buf = io.StringIO()
writer = csv.writer(buf, quoting=csv.QUOTE_NONNUMERIC)
writer.writerow(["1", 1, 1.5, True, None, "x"])
assert buf.getvalue() == '"1",1,1.5,True,"","x"\r\n'

buf = io.StringIO()
writer = csv.DictWriter(buf, ["a", "b"], quoting=csv.QUOTE_NONNUMERIC)
writer.writeheader()
writer.writerow({"a": "1", "b": 2})
assert buf.getvalue() == '"a","b"\r\n"1",2\r\n'


# quotechar=None implies QUOTE_NONE when quoting is omitted.
assert list(csv.reader(['1,",3,",5'], quotechar=None, escapechar="\\")) == [
    ["1", '"', "3", '"', "5"]
]
buf = io.StringIO()
writer = csv.writer(buf, quotechar=None, quoting=csv.QUOTE_NONE, escapechar="\\")
writer.writerow(["a,b"])
assert buf.getvalue() == "a\\,b\r\n"
csv.register_dialect("nq", quotechar=None)
try:
    dialect = csv.get_dialect("nq")
    assert dialect.quoting == csv.QUOTE_NONE
    assert dialect.quotechar is None
finally:
    csv.unregister_dialect("nq")


# field_size_limit validates arguments and is enforced while parsing.
old_limit = csv.field_size_limit()
try:
    assert csv.field_size_limit(2) == old_limit
    assert csv.field_size_limit() == 2
    try:
        list(csv.reader(["abc"]))
        assert False, "field larger than limit should fail"
    except csv.Error:
        pass

    for args in [(None,), (1, None)]:
        try:
            csv.field_size_limit(*args)
            assert False, "bad field_size_limit args should fail"
        except TypeError:
            pass
finally:
    csv.field_size_limit(old_limit)


print("test_csv_compat passed")
