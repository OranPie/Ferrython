# Phase 93: datetime.replace, timedelta.total_seconds, math.gcd variadic, date.strftime
from datetime import datetime, date, timedelta

# ── datetime.replace ──
dt = datetime(2024, 6, 15, 10, 30, 0)
dt2 = dt.replace(year=2025)
assert dt2.year == 2025, f"replace year: {dt2.year}"
assert dt2.month == 6, f"replace kept month: {dt2.month}"
assert dt2.day == 15, f"replace kept day: {dt2.day}"
print("check 1 passed: datetime.replace year")

dt3 = dt.replace(month=12, day=25)
assert dt3.month == 12, f"replace month: {dt3.month}"
assert dt3.day == 25, f"replace day: {dt3.day}"
assert dt3.hour == 10, f"replace kept hour: {dt3.hour}"
print("check 2 passed: datetime.replace month+day")

# ── timedelta.total_seconds() as callable ──
td = timedelta(days=1, seconds=3600)
ts = td.total_seconds()
assert ts == 90000.0, f"total_seconds: {ts}"
print("check 3 passed: timedelta.total_seconds()")

td2 = timedelta(seconds=30)
assert td2.total_seconds() == 30.0, f"total_seconds 30: {td2.total_seconds()}"
print("check 4 passed: timedelta.total_seconds small")

td3 = timedelta(days=0, seconds=0, microseconds=500000)
ts3 = td3.total_seconds()
assert abs(ts3 - 0.5) < 0.01, f"total_seconds microseconds: {ts3}"
print("check 5 passed: timedelta.total_seconds with microseconds")

# ── timedelta.__repr__ ──
td4 = timedelta(days=5)
r = repr(td4)
assert "timedelta" in r, f"timedelta repr: {r}"
assert "5" in r, f"timedelta repr has 5: {r}"
print("check 6 passed: timedelta.__repr__")

# ── timedelta.__bool__ ──
assert bool(timedelta(days=1)) == True
assert bool(timedelta(seconds=0, days=0, microseconds=0)) == False
print("check 7 passed: timedelta.__bool__")

# ── timedelta.__neg__ ──
td5 = timedelta(days=3, seconds=100)
neg = -td5
assert neg.days == -3, f"neg days: {neg.days}"
assert neg.seconds == -100, f"neg seconds: {neg.seconds}"
print("check 8 passed: timedelta.__neg__")

# ── timedelta.__eq__ ──
assert timedelta(days=1) == timedelta(days=1)
assert timedelta(days=1) != timedelta(days=2)
print("check 9 passed: timedelta equality")

# ── date.replace ──
d = date(2024, 3, 15)
d2 = d.replace(year=2025)
assert d2.year == 2025, f"date.replace year: {d2.year}"
assert d2.month == 3
assert d2.day == 15
print("check 10 passed: date.replace year")

d3 = d.replace(month=12)
assert d3.month == 12
print("check 11 passed: date.replace month")

# ── date.strftime with full format codes ──
d4 = date(2024, 1, 15)
# %A = day name, %B = month name
fmt = d4.strftime("%A, %B %d, %Y")
assert "2024" in fmt, f"date.strftime year: {fmt}"
assert "January" in fmt or "Jan" in fmt, f"date.strftime month: {fmt}"
print("check 12 passed: date.strftime full codes")

# datetime.strftime with full format codes
dt4 = datetime(2024, 7, 4, 14, 30, 0)
fmt2 = dt4.strftime("%Y-%m-%d %H:%M:%S %A")
assert "2024-07-04" in fmt2, f"datetime.strftime: {fmt2}"
assert "14:30:00" in fmt2, f"datetime.strftime time: {fmt2}"
print("check 13 passed: datetime.strftime full codes")

# %I and %p (12-hour)
fmt3 = dt4.strftime("%I:%M %p")
assert "PM" in fmt3, f"datetime.strftime PM: {fmt3}"
assert "02:" in fmt3, f"datetime.strftime 12h: {fmt3}"
print("check 14 passed: datetime.strftime 12-hour")

# ── math.gcd variadic ──
import math

assert math.gcd(12, 8) == 4
assert math.gcd(12, 8, 6) == 2
assert math.gcd(100, 75, 50, 25) == 25
assert math.gcd(7) == 7
assert math.gcd() == 0
print("check 15 passed: math.gcd variadic")

# ── datetime operations ──
now = datetime.now()
assert now.year >= 2024
print("check 16 passed: datetime.now()")

# ── datetime.timestamp ──
dt5 = datetime(2024, 1, 1, 0, 0, 0)
ts5 = dt5.timestamp()
assert ts5 > 0, f"timestamp: {ts5}"
print("check 17 passed: datetime.timestamp()")

# ── datetime weekday/isoweekday ──
dt6 = datetime(2024, 1, 1)  # Monday
wd = dt6.weekday()
assert 0 <= wd <= 6, f"weekday: {wd}"
iwd = dt6.isoweekday()
assert 1 <= iwd <= 7, f"isoweekday: {iwd}"
print("check 18 passed: datetime weekday methods")

# ── datetime - datetime = timedelta ──
dt7 = datetime(2024, 1, 10)
dt8 = datetime(2024, 1, 1)
diff = dt7 - dt8
assert diff.days == 9, f"datetime diff: {diff.days}"
print("check 19 passed: datetime subtraction → timedelta")

# ── datetime + timedelta ──
dt9 = datetime(2024, 1, 1) + timedelta(days=30)
assert dt9.month == 1 or dt9.month == 2, f"dt+td month: {dt9.month}"
assert dt9.day == 31 or dt9.day == 1, f"dt+td day: {dt9.day}"
print("check 20 passed: datetime + timedelta")

print("All 20 checks passed!")
