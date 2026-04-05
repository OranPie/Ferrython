# Phase 75: Pure Python stdlib modules + Rust decimal/datetime improvements
passed = 0
failed = 0

def check(name, got, expected):
    global passed, failed
    if got == expected:
        passed = passed + 1
    else:
        failed = failed + 1
        print("FAIL:", name, "got:", got, "expected:", expected)

def check_close(name, got, expected, tol=0.01):
    global passed, failed
    if abs(got - expected) < tol:
        passed = passed + 1
    else:
        failed = failed + 1
        print("FAIL:", name, "got:", got, "expected:", expected)

# ── colorsys ──
import colorsys

# RGB -> HSV -> RGB roundtrip
h, s, v = colorsys.rgb_to_hsv(1.0, 0.0, 0.0)
check_close("hsv_red_h", h, 0.0)
check_close("hsv_red_s", s, 1.0)
check_close("hsv_red_v", v, 1.0)

r, g, b = colorsys.hsv_to_rgb(h, s, v)
check_close("hsv_roundtrip_r", r, 1.0)
check_close("hsv_roundtrip_g", g, 0.0)
check_close("hsv_roundtrip_b", b, 0.0)

# Green
h2, s2, v2 = colorsys.rgb_to_hsv(0.0, 1.0, 0.0)
check_close("hsv_green_h", h2, 1.0/3.0)

# RGB -> HLS -> RGB roundtrip
h3, l3, s3 = colorsys.rgb_to_hls(0.0, 0.0, 1.0)
check_close("hls_blue_h", h3, 2.0/3.0)
check_close("hls_blue_l", l3, 0.5)

r3, g3, b3 = colorsys.hls_to_rgb(h3, l3, s3)
check_close("hls_roundtrip_r", r3, 0.0)
check_close("hls_roundtrip_b", b3, 1.0)

# RGB -> YIQ -> RGB roundtrip
y, i, q = colorsys.rgb_to_yiq(0.5, 0.5, 0.5)
check_close("yiq_gray_y", y, 0.5)
check_close("yiq_gray_i", i, 0.0, 0.001)
check_close("yiq_gray_q", q, 0.0, 0.001)

# White
h_w, s_w, v_w = colorsys.rgb_to_hsv(1.0, 1.0, 1.0)
check_close("hsv_white_s", s_w, 0.0)
check_close("hsv_white_v", v_w, 1.0)

# Black
h_b, s_b, v_b = colorsys.rgb_to_hsv(0.0, 0.0, 0.0)
check_close("hsv_black_v", v_b, 0.0)

# ── gettext ──
import gettext

msg = gettext.gettext("Hello")
check("gettext_identity", msg, "Hello")

s = gettext.ngettext("apple", "apples", 1)
check("ngettext_singular", s, "apple")
s2 = gettext.ngettext("apple", "apples", 5)
check("ngettext_plural", s2, "apples")

t = gettext.NullTranslations()
check("null_trans_gettext", t.gettext("test"), "test")
check("null_trans_ngettext", t.ngettext("item", "items", 2), "items")

t2 = gettext.translation("test", fallback=True)
check("translation_fallback", t2.gettext("hello"), "hello")

check("gettext_underscore", gettext._("hi"), "hi")

# ── keyword ──
import keyword

check("iskeyword_if", keyword.iskeyword("if"), True)
check("iskeyword_for", keyword.iskeyword("for"), True)
check("iskeyword_def", keyword.iskeyword("def"), True)
check("iskeyword_class", keyword.iskeyword("class"), True)
check("iskeyword_foo", keyword.iskeyword("foo"), False)
check("iskeyword_print", keyword.iskeyword("print"), False)

check("issoftkeyword_match", keyword.issoftkeyword("match"), True)
check("issoftkeyword_case", keyword.issoftkeyword("case"), True)
check("issoftkeyword_if", keyword.issoftkeyword("if"), False)

check("kwlist_contains_True", "True" in keyword.kwlist, True)
check("kwlist_contains_return", "return" in keyword.kwlist, True)
check("kwlist_length", len(keyword.kwlist) > 30, True)

# ── decimal improvements ──
from decimal import Decimal, getcontext

# sqrt
d = Decimal("4.0")
sq = d.sqrt()
check_close("decimal_sqrt_4", float(sq), 2.0)

d2 = Decimal("2.0")
sq2 = d2.sqrt()
check_close("decimal_sqrt_2", float(sq2), 1.4142, 0.001)

d9 = Decimal("9")
sq9 = d9.sqrt()
check_close("decimal_sqrt_9", float(sq9), 3.0)

# exp
d_one = Decimal("1")
e = d_one.exp()
check_close("decimal_exp_1", float(e), 2.71828, 0.001)

d_zero = Decimal("0")
e0 = d_zero.exp()
check_close("decimal_exp_0", float(e0), 1.0)

# ln
d_e = Decimal("2.718281828")
ln_e = d_e.ln()
check_close("decimal_ln_e", float(ln_e), 1.0, 0.001)

d10 = Decimal("10")
ln10 = d10.ln()
check_close("decimal_ln_10", float(ln10), 2.302585, 0.001)

# is_zero
check("decimal_is_zero_true", Decimal("0").is_zero(), True)
check("decimal_is_zero_false", Decimal("1").is_zero(), False)
check("decimal_is_zero_0.0", Decimal("0.0").is_zero(), True)

# is_nan
check("decimal_is_nan_true", Decimal("NaN").is_nan(), True)
check("decimal_is_nan_false", Decimal("1").is_nan(), False)

# is_infinite
check("decimal_is_infinite_true", Decimal("Infinity").is_infinite(), True)
check("decimal_is_infinite_neg", Decimal("-Infinity").is_infinite(), True)
check("decimal_is_infinite_false", Decimal("1").is_infinite(), False)

# to_eng_string
s = Decimal("0").to_eng_string()
check("decimal_eng_zero", s, "0")

# getcontext
ctx = getcontext()
check("getcontext_prec", ctx.prec, 28)
check("getcontext_rounding", ctx.rounding, "ROUND_HALF_EVEN")

# ── datetime improvements ──
import datetime

# isoformat
dt = datetime.datetime(2024, 1, 15, 10, 30, 45)
iso = dt.isoformat()
check("datetime_isoformat", iso, "2024-01-15T10:30:45")

# date isoformat
d = datetime.date(2024, 6, 15)
check("date_isoformat", d.isoformat(), "2024-06-15")

# timestamp
dt2 = datetime.datetime(1970, 1, 1, 0, 0, 0)
ts = dt2.timestamp()
check_close("datetime_timestamp_epoch", ts, 0.0, 1.0)

# timedelta
td = datetime.timedelta(days=1, hours=2, minutes=30)
check("timedelta_days", td.days, 1)
ts_td = td.total_seconds()
check_close("timedelta_total_seconds", ts_td, 86400.0 + 7200.0 + 1800.0)

# date + timedelta
d1 = datetime.date(2024, 1, 1)
d2 = d1 + datetime.timedelta(days=31)
check("date_add_timedelta_month", d2.month, 2)
check("date_add_timedelta_day", d2.day, 1)

# datetime - datetime = timedelta
dt_a = datetime.datetime(2024, 3, 1, 0, 0, 0)
dt_b = datetime.datetime(2024, 1, 1, 0, 0, 0)
diff = dt_a - dt_b
check("datetime_diff_days", diff.days, 60)

# combine
d_comb = datetime.date(2024, 12, 25)
t_comb = datetime.time(14, 30, 0)
dt_comb = datetime.datetime.combine(d_comb, t_comb)
check("combine_year", dt_comb.year, 2024)
check("combine_month", dt_comb.month, 12)
check("combine_day", dt_comb.day, 25)
check("combine_hour", dt_comb.hour, 14)
check("combine_minute", dt_comb.minute, 30)

# date.today
today = datetime.date.today()
check("date_today_has_year", today.year > 2020, True)

# isocalendar
dt_iso = datetime.datetime(2024, 1, 1, 0, 0, 0)
iso_cal = dt_iso.isocalendar()
check("isocalendar_year", iso_cal[0], 2024)
check("isocalendar_week", iso_cal[1], 1)
check("isocalendar_weekday", iso_cal[2], 1)  # Monday

# ── final report ──
print("Phase 75 Tests:", passed + failed, "| Passed:", passed, "| Failed:", failed)
if failed > 0:
    raise Exception("TESTS FAILED: " + str(failed))
print("ALL PHASE 75 TESTS PASSED!")
