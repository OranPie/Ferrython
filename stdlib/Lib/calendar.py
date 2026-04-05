"""Calendar printing functions.

Note: If a Rust-native calendar module is loaded instead of this one,
that's fine - both provide the same interface.
"""

__all__ = [
    'MONDAY', 'TUESDAY', 'WEDNESDAY', 'THURSDAY', 'FRIDAY', 'SATURDAY', 'SUNDAY',
    'isleap', 'leapdays', 'weekday', 'monthrange', 'monthcalendar',
    'month_name', 'month_abbr', 'day_name', 'day_abbr',
    'TextCalendar', 'calendar', 'month', 'prmonth', 'prcal',
]

# Day constants
MONDAY = 0
TUESDAY = 1
WEDNESDAY = 2
THURSDAY = 3
FRIDAY = 4
SATURDAY = 5
SUNDAY = 6

# Month and day name lists
month_name = [
    '', 'January', 'February', 'March', 'April', 'May', 'June',
    'July', 'August', 'September', 'October', 'November', 'December'
]

month_abbr = [
    '', 'Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun',
    'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'
]

day_name = [
    'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday', 'Sunday'
]

day_abbr = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun']

# Number of days per month (non-leap year)
_mdays = [0, 31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]


def isleap(year):
    """Return True for leap years, False for non-leap years."""
    return year % 4 == 0 and (year % 100 != 0 or year % 400 == 0)


def leapdays(y1, y2):
    """Return number of leap years in range [y1, y2).
    This is just about the number of leap years, not calendar-specific."""
    y1 = y1 - 1
    y2 = y2 - 1
    return (y2 // 4 - y1 // 4) - (y2 // 100 - y1 // 100) + (y2 // 400 - y1 // 400)


def weekday(year, month, day):
    """Return day of the week (0=Monday, 6=Sunday) for year, month, day.
    Uses Zeller-like formula."""
    if month < 3:
        month = month + 12
        year = year - 1
    q = day
    m = month
    k = year % 100
    j = year // 100
    # Zeller's congruence adapted for Monday=0
    h = (q + (13 * (m + 1)) // 5 + k + k // 4 + j // 4 - 2 * j) % 7
    # Convert from Zeller (0=Saturday) to Python (0=Monday)
    return (h + 6) % 7


def monthrange(year, month):
    """Return weekday of first day of month and number of days in month."""
    if not 1 <= month <= 12:
        raise ValueError("bad month number; must be 1-12")
    day1 = weekday(year, month, 1)
    if month == 2 and isleap(year):
        ndays = 29
    else:
        ndays = _mdays[month]
    return (day1, ndays)


def monthcalendar(year, month):
    """Return a matrix representing a month's calendar.
    Each row represents a week; days outside the month are set to 0."""
    day1, ndays = monthrange(year, month)
    rows = []
    row = [0] * day1
    day = 1
    while day <= ndays:
        row.append(day)
        if len(row) == 7:
            rows.append(row)
            row = []
        day = day + 1
    if row:
        while len(row) < 7:
            row.append(0)
        rows.append(row)
    return rows


class TextCalendar:
    """A calendar that can produce formatted text output."""

    def __init__(self, firstweekday=0):
        self.firstweekday = firstweekday

    def formatmonthname(self, theyear, themonth, width, withyear=True):
        """Return a formatted month name."""
        s = month_name[themonth]
        if withyear:
            s = s + ' ' + str(theyear)
        return s.center(width)

    def formatweekday(self, day, width):
        """Return a formatted week day name."""
        names = day_abbr
        if width >= 9:
            names = day_name
        return names[day].center(width)

    def formatweekheader(self, width):
        """Return a header for a week."""
        header = []
        for i in range(7):
            day = (self.firstweekday + i) % 7
            header.append(self.formatweekday(day, width))
        return ' '.join(header)

    def formatday(self, day, width):
        """Format a single day."""
        if day == 0:
            return ' ' * width
        return str(day).rjust(width)

    def formatweek(self, theweek, width):
        """Format a single week row."""
        return ' '.join(self.formatday(d, width) for d in theweek)

    def formatmonth(self, theyear, themonth, w=2, l=1):
        """Return a month's calendar as a multi-line string."""
        w = max(2, w)
        l = max(1, l)
        lines = []
        lines.append(self.formatmonthname(theyear, themonth, 7 * (w + 1) - 1))
        lines.append('')
        lines.append(self.formatweekheader(w))
        for week in monthcalendar(theyear, themonth):
            lines.append(self.formatweek(week, w))
            for _ in range(l - 1):
                lines.append('')
        return '\n'.join(lines) + '\n'

    def formatyear(self, theyear, w=2, l=1, c=6, m=3):
        """Return a year's calendar as a multi-line string."""
        lines = []
        header = str(theyear).center(m * (7 * (w + 1) - 1 + c) - c)
        lines.append(header)
        lines.append('')
        for i in range(1, 13, m):
            month_strs = []
            for j in range(m):
                month_num = i + j
                if month_num <= 12:
                    month_strs.append(self.formatmonth(theyear, month_num, w, l))
            lines.append('  '.join(
                [ms.split('\n')[0] for ms in month_strs]
            ))
            # Add remaining lines
            max_lines = max(len(ms.split('\n')) for ms in month_strs)
            for k in range(1, max_lines):
                row_parts = []
                for ms in month_strs:
                    ms_lines = ms.split('\n')
                    if k < len(ms_lines):
                        row_parts.append(ms_lines[k])
                    else:
                        row_parts.append(' ' * (7 * (w + 1) - 1))
                lines.append('  '.join(row_parts))
            lines.append('')
        return '\n'.join(lines)


_default_calendar = TextCalendar()


def month(theyear, themonth, w=0, l=0):
    """Print a month's calendar."""
    return _default_calendar.formatmonth(theyear, themonth)


def calendar(theyear, w=2, l=1, c=6, m=3):
    """Return a year's calendar as a multi-line string."""
    return _default_calendar.formatyear(theyear, w, l, c, m)


def prmonth(theyear, themonth, w=0, l=0):
    """Print a month's calendar."""
    print(month(theyear, themonth, w, l))


def prcal(theyear, w=0, l=0, c=6, m=3):
    """Print a year's calendar."""
    print(calendar(theyear, w, l, c, m))
