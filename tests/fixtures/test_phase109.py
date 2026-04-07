# test_phase109.py — xml.etree iter() fix, curses stub, stdlib deepening

# ── xml.etree.ElementTree iter() includes root ──
import xml.etree.ElementTree as ET

xml_str = "<root><child1><sub1/></child1><child2/></root>"
root = ET.fromstring(xml_str)

# iter() should include the root element itself (CPython behavior)
all_tags = [e.tag for e in root.iter()]
assert "root" in all_tags, f"root should be in iter(), got {all_tags}"
assert "child1" in all_tags, f"child1 should be in iter(), got {all_tags}"
assert "sub1" in all_tags, f"sub1 should be in iter(), got {all_tags}"
assert "child2" in all_tags, f"child2 should be in iter(), got {all_tags}"
assert all_tags[0] == "root", f"root should be first in iter(), got {all_tags}"

# iter() with tag filter
child_tags = [e.tag for e in root.iter("child1")]
assert child_tags == ["child1"], f"Expected ['child1'], got {child_tags}"

# ── curses stub ──
import curses

assert hasattr(curses, "initscr")
assert hasattr(curses, "endwin")
assert hasattr(curses, "wrapper")
assert hasattr(curses, "start_color")
assert hasattr(curses, "cbreak")
assert hasattr(curses, "noecho")
assert hasattr(curses, "curs_set")

# Color constants
assert curses.COLOR_BLACK == 0
assert curses.COLOR_RED == 1
assert curses.COLOR_WHITE == 7

# Key constants
assert curses.KEY_UP == 259
assert curses.KEY_DOWN == 258

# Attribute constants
assert curses.A_NORMAL == 0
assert curses.A_BOLD == (1 << 21)

# has_colors returns False in stub
assert curses.has_colors() == False

# initscr returns a window
win = curses.initscr()
assert hasattr(win, "addstr")
assert hasattr(win, "refresh")
assert hasattr(win, "getmaxyx")
maxyx = win.getmaxyx()
assert maxyx[0] == 24 and maxyx[1] == 80

# endwin doesn't crash
curses.endwin()

# error class exists
assert hasattr(curses, "error")

print("phase109: all tests passed")
