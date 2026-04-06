# Phase 100: datetime enhancements and timedelta division
import datetime

# 1. datetime.astimezone with UTC source
utc = datetime.timezone.utc
dt = datetime.datetime(2023, 6, 15, 12, 0, 0, tzinfo=utc)
tz5 = datetime.timezone(datetime.timedelta(hours=5))
local = dt.astimezone(tz5)
assert local.hour == 17, f"astimezone hour: {local.hour}"

# 2. astimezone with negative offset
tz_minus3 = datetime.timezone(datetime.timedelta(hours=-3))
local2 = dt.astimezone(tz_minus3)
assert local2.hour == 9, f"astimezone -3: {local2.hour}"

# 3. timedelta / int
td = datetime.timedelta(days=10)
half = td / 2
assert half.days == 5, f"td/2 days: {half.days}"

# 4. timedelta / float
third = datetime.timedelta(hours=3) / 1.5
assert third.total_seconds() == 7200.0, f"td/1.5: {third.total_seconds()}"

# 5. timedelta / timedelta = float ratio
ratio = datetime.timedelta(days=10) / datetime.timedelta(days=5)
assert ratio == 2.0, f"td/td ratio: {ratio}"

# 6. timedelta // int
td2 = datetime.timedelta(days=7) // 2
assert td2.days == 3, f"td//2 days: {td2.days}"

# 7. timedelta // timedelta = int
q = datetime.timedelta(days=10) // datetime.timedelta(days=3)
assert q == 3, f"td//td: {q}"

# 8. Constructor-created datetime has instance methods
dt2 = datetime.datetime(2023, 1, 15, 10, 30, 0)
assert hasattr(dt2, 'isoformat'), "missing isoformat"
assert hasattr(dt2, 'strftime'), "missing strftime"
assert hasattr(dt2, 'weekday'), "missing weekday"
assert dt2.isoformat() == '2023-01-15T10:30:00', f"isoformat: {dt2.isoformat()}"

print("PASS: all 8 checks passed")
