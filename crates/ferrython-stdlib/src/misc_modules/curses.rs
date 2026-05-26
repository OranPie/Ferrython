use compact_str::CompactString;
use ferrython_core::error::PyException;
use ferrython_core::object::{
    make_builtin, make_module, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use indexmap::IndexMap;

// ── curses module (stub) ──

/// Helper to create a curses window object with standard methods
fn make_curses_window(nlines: i64, ncols: i64, begin_y: i64, begin_x: i64) -> PyObjectRef {
    let cls = PyObject::class(CompactString::from("Window"), vec![], IndexMap::new());
    let win = PyObject::instance(cls);
    if let PyObjectPayload::Instance(ref d) = win.payload {
        let mut w = d.attrs.write();
        w.insert(CompactString::from("_nlines"), PyObject::int(nlines));
        w.insert(CompactString::from("_ncols"), PyObject::int(ncols));
        w.insert(CompactString::from("_begin_y"), PyObject::int(begin_y));
        w.insert(CompactString::from("_begin_x"), PyObject::int(begin_x));
        w.insert(CompactString::from("_cur_y"), PyObject::int(0));
        w.insert(CompactString::from("_cur_x"), PyObject::int(0));

        let self_ref = win.clone();
        let s = self_ref.clone();
        w.insert(
            CompactString::from("addstr"),
            PyObject::native_closure("addstr", move |_| Ok(s.clone())),
        );
        let s = self_ref.clone();
        w.insert(
            CompactString::from("addnstr"),
            PyObject::native_closure("addnstr", move |_| Ok(s.clone())),
        );
        let s = self_ref.clone();
        w.insert(
            CompactString::from("refresh"),
            PyObject::native_closure("refresh", move |_| Ok(s.clone())),
        );
        let s = self_ref.clone();
        w.insert(
            CompactString::from("clear"),
            PyObject::native_closure("clear", move |_| Ok(s.clone())),
        );
        let s = self_ref.clone();
        w.insert(
            CompactString::from("erase"),
            PyObject::native_closure("erase", move |_| Ok(s.clone())),
        );
        w.insert(
            CompactString::from("getch"),
            make_builtin(|_| Ok(PyObject::int(-1))),
        );
        w.insert(
            CompactString::from("getkey"),
            make_builtin(|_| Ok(PyObject::str_val(CompactString::from("")))),
        );
        let nl = nlines;
        let nc = ncols;
        w.insert(
            CompactString::from("getmaxyx"),
            PyObject::native_closure("getmaxyx", move |_| {
                Ok(PyObject::tuple(vec![PyObject::int(nl), PyObject::int(nc)]))
            }),
        );
        w.insert(
            CompactString::from("getyx"),
            make_builtin(|_| Ok(PyObject::tuple(vec![PyObject::int(0), PyObject::int(0)]))),
        );
        w.insert(CompactString::from("getbegyx"), {
            let by = begin_y;
            let bx = begin_x;
            PyObject::native_closure("getbegyx", move |_| {
                Ok(PyObject::tuple(vec![PyObject::int(by), PyObject::int(bx)]))
            })
        });
        let s = self_ref.clone();
        w.insert(
            CompactString::from("move"),
            PyObject::native_closure("move", move |_| Ok(s.clone())),
        );
        let s = self_ref.clone();
        w.insert(
            CompactString::from("clrtoeol"),
            PyObject::native_closure("clrtoeol", move |_| Ok(s.clone())),
        );
        let s = self_ref.clone();
        w.insert(
            CompactString::from("clrtobot"),
            PyObject::native_closure("clrtobot", move |_| Ok(s.clone())),
        );
        let s = self_ref.clone();
        w.insert(
            CompactString::from("keypad"),
            PyObject::native_closure("keypad", move |_| Ok(s.clone())),
        );
        let s = self_ref.clone();
        w.insert(
            CompactString::from("nodelay"),
            PyObject::native_closure("nodelay", move |_| Ok(s.clone())),
        );
        let s = self_ref.clone();
        w.insert(
            CompactString::from("timeout"),
            PyObject::native_closure("timeout", move |_| Ok(s.clone())),
        );
        let s = self_ref.clone();
        w.insert(
            CompactString::from("scrollok"),
            PyObject::native_closure("scrollok", move |_| Ok(s.clone())),
        );
        let s = self_ref.clone();
        w.insert(
            CompactString::from("idlok"),
            PyObject::native_closure("idlok", move |_| Ok(s.clone())),
        );
        let s = self_ref.clone();
        w.insert(
            CompactString::from("border"),
            PyObject::native_closure("border", move |_| Ok(s.clone())),
        );
        let s = self_ref.clone();
        w.insert(
            CompactString::from("box"),
            PyObject::native_closure("box", move |_| Ok(s.clone())),
        );
        w.insert(
            CompactString::from("subwin"),
            make_builtin(|args: &[PyObjectRef]| {
                let (nl, nc, by, bx) = match args.len() {
                    4 => (
                        args[0].as_int().unwrap_or(24),
                        args[1].as_int().unwrap_or(80),
                        args[2].as_int().unwrap_or(0),
                        args[3].as_int().unwrap_or(0),
                    ),
                    2 => (
                        args[0].as_int().unwrap_or(24),
                        args[1].as_int().unwrap_or(80),
                        0,
                        0,
                    ),
                    _ => (24, 80, 0, 0),
                };
                Ok(make_curses_window(nl, nc, by, bx))
            }),
        );
        w.insert(
            CompactString::from("derwin"),
            make_builtin(|args: &[PyObjectRef]| {
                let (nl, nc, by, bx) = match args.len() {
                    4 => (
                        args[0].as_int().unwrap_or(24),
                        args[1].as_int().unwrap_or(80),
                        args[2].as_int().unwrap_or(0),
                        args[3].as_int().unwrap_or(0),
                    ),
                    2 => (
                        args[0].as_int().unwrap_or(24),
                        args[1].as_int().unwrap_or(80),
                        0,
                        0,
                    ),
                    _ => (24, 80, 0, 0),
                };
                Ok(make_curses_window(nl, nc, by, bx))
            }),
        );
        let s = self_ref.clone();
        w.insert(
            CompactString::from("mvaddstr"),
            PyObject::native_closure("mvaddstr", move |_| Ok(s.clone())),
        );
        let s = self_ref.clone();
        w.insert(
            CompactString::from("attron"),
            PyObject::native_closure("attron", move |_| Ok(s.clone())),
        );
        let s = self_ref.clone();
        w.insert(
            CompactString::from("attroff"),
            PyObject::native_closure("attroff", move |_| Ok(s.clone())),
        );
        let s = self_ref.clone();
        w.insert(
            CompactString::from("attrset"),
            PyObject::native_closure("attrset", move |_| Ok(s.clone())),
        );
        let s = self_ref.clone();
        w.insert(
            CompactString::from("bkgd"),
            PyObject::native_closure("bkgd", move |_| Ok(s.clone())),
        );
        let s = self_ref.clone();
        w.insert(
            CompactString::from("noutrefresh"),
            PyObject::native_closure("noutrefresh", move |_| Ok(s.clone())),
        );
        let s = self_ref.clone();
        w.insert(
            CompactString::from("insstr"),
            PyObject::native_closure("insstr", move |_| Ok(s.clone())),
        );
        let s = self_ref.clone();
        w.insert(
            CompactString::from("deleteln"),
            PyObject::native_closure("deleteln", move |_| Ok(s.clone())),
        );
        let s = self_ref.clone();
        w.insert(
            CompactString::from("insertln"),
            PyObject::native_closure("insertln", move |_| Ok(s.clone())),
        );
        let s = self_ref.clone();
        w.insert(
            CompactString::from("scroll"),
            PyObject::native_closure("scroll", move |_| Ok(s.clone())),
        );
        let s = self_ref.clone();
        w.insert(
            CompactString::from("setscrreg"),
            PyObject::native_closure("setscrreg", move |_| Ok(s.clone())),
        );
        w.insert(
            CompactString::from("inch"),
            make_builtin(|_| Ok(PyObject::int(32))),
        ); // space char
        w.insert(
            CompactString::from("instr"),
            make_builtin(|_| Ok(PyObject::str_val(CompactString::from("")))),
        );
    }
    win
}

pub fn create_curses_module() -> PyObjectRef {
    // Stub curses module — provides constants and no-op functions
    // so that programs that conditionally import curses don't crash.

    let initscr_fn = make_builtin(|_: &[PyObjectRef]| Ok(make_curses_window(24, 80, 0, 0)));

    let wrapper_fn = make_builtin(|args: &[PyObjectRef]| {
        // curses.wrapper(func) — calls func(stdscr)
        if args.is_empty() {
            return Err(PyException::type_error("wrapper() requires a callable"));
        }
        let stdscr = make_curses_window(24, 80, 0, 0);
        ferrython_core::error::request_vm_call(args[0].clone(), vec![stdscr]);
        Ok(PyObject::none())
    });

    make_module(
        "curses",
        vec![
            ("initscr", initscr_fn.clone()),
            ("endwin", make_builtin(|_| Ok(PyObject::none()))),
            ("wrapper", wrapper_fn),
            ("start_color", make_builtin(|_| Ok(PyObject::none()))),
            ("init_pair", make_builtin(|_| Ok(PyObject::none()))),
            (
                "color_pair",
                make_builtin(|args| {
                    Ok(PyObject::int(
                        args.first().and_then(|a| a.as_int()).unwrap_or(0),
                    ))
                }),
            ),
            ("cbreak", make_builtin(|_| Ok(PyObject::none()))),
            ("nocbreak", make_builtin(|_| Ok(PyObject::none()))),
            ("echo", make_builtin(|_| Ok(PyObject::none()))),
            ("noecho", make_builtin(|_| Ok(PyObject::none()))),
            ("raw", make_builtin(|_| Ok(PyObject::none()))),
            ("noraw", make_builtin(|_| Ok(PyObject::none()))),
            (
                "curs_set",
                make_builtin(|args| {
                    // Returns previous cursor visibility (0, 1, or 2)
                    let _new = args.first().and_then(|a| a.as_int()).unwrap_or(1);
                    Ok(PyObject::int(1))
                }),
            ),
            (
                "newwin",
                make_builtin(|args: &[PyObjectRef]| {
                    let (nl, nc, by, bx) = match args.len() {
                        4 => (
                            args[0].as_int().unwrap_or(24),
                            args[1].as_int().unwrap_or(80),
                            args[2].as_int().unwrap_or(0),
                            args[3].as_int().unwrap_or(0),
                        ),
                        2 => (
                            args[0].as_int().unwrap_or(24),
                            args[1].as_int().unwrap_or(80),
                            0,
                            0,
                        ),
                        _ => (24, 80, 0, 0),
                    };
                    Ok(make_curses_window(nl, nc, by, bx))
                }),
            ),
            (
                "newpad",
                make_builtin(|args: &[PyObjectRef]| {
                    let nl = args.first().and_then(|a| a.as_int()).unwrap_or(100);
                    let nc = args.get(1).and_then(|a| a.as_int()).unwrap_or(100);
                    Ok(make_curses_window(nl, nc, 0, 0))
                }),
            ),
            (
                "napms",
                make_builtin(|args| {
                    let ms = args.first().and_then(|a| a.as_int()).unwrap_or(0);
                    if ms > 0 {
                        std::thread::sleep(std::time::Duration::from_millis(ms as u64));
                    }
                    Ok(PyObject::none())
                }),
            ),
            ("beep", make_builtin(|_| Ok(PyObject::none()))),
            ("flash", make_builtin(|_| Ok(PyObject::none()))),
            ("doupdate", make_builtin(|_| Ok(PyObject::none()))),
            (
                "has_colors",
                make_builtin(|_| Ok(PyObject::bool_val(false))),
            ),
            (
                "can_change_color",
                make_builtin(|_| Ok(PyObject::bool_val(false))),
            ),
            ("use_default_colors", make_builtin(|_| Ok(PyObject::none()))),
            ("use_env", make_builtin(|_| Ok(PyObject::none()))),
            ("isendwin", make_builtin(|_| Ok(PyObject::bool_val(false)))),
            (
                "erasechar",
                make_builtin(|_| Ok(PyObject::str_val(CompactString::from("\x08")))),
            ),
            (
                "killchar",
                make_builtin(|_| Ok(PyObject::str_val(CompactString::from("\x15")))),
            ),
            ("LINES", PyObject::int(24)),
            ("COLS", PyObject::int(80)),
            ("COLORS", PyObject::int(256)),
            ("COLOR_PAIRS", PyObject::int(256)),
            // Color constants
            ("COLOR_BLACK", PyObject::int(0)),
            ("COLOR_RED", PyObject::int(1)),
            ("COLOR_GREEN", PyObject::int(2)),
            ("COLOR_YELLOW", PyObject::int(3)),
            ("COLOR_BLUE", PyObject::int(4)),
            ("COLOR_MAGENTA", PyObject::int(5)),
            ("COLOR_CYAN", PyObject::int(6)),
            ("COLOR_WHITE", PyObject::int(7)),
            // Attribute constants
            ("A_NORMAL", PyObject::int(0)),
            ("A_STANDOUT", PyObject::int(1 << 16)),
            ("A_UNDERLINE", PyObject::int(1 << 17)),
            ("A_REVERSE", PyObject::int(1 << 18)),
            ("A_BLINK", PyObject::int(1 << 19)),
            ("A_DIM", PyObject::int(1 << 20)),
            ("A_BOLD", PyObject::int(1 << 21)),
            ("A_PROTECT", PyObject::int(1 << 24)),
            ("A_INVIS", PyObject::int(1 << 23)),
            ("A_ALTCHARSET", PyObject::int(1 << 22)),
            // Key constants
            ("KEY_UP", PyObject::int(259)),
            ("KEY_DOWN", PyObject::int(258)),
            ("KEY_LEFT", PyObject::int(260)),
            ("KEY_RIGHT", PyObject::int(261)),
            ("KEY_HOME", PyObject::int(262)),
            ("KEY_BACKSPACE", PyObject::int(263)),
            ("KEY_F0", PyObject::int(264)),
            ("KEY_DC", PyObject::int(330)),
            ("KEY_IC", PyObject::int(331)),
            ("KEY_NPAGE", PyObject::int(338)),
            ("KEY_PPAGE", PyObject::int(339)),
            ("KEY_ENTER", PyObject::int(343)),
            ("KEY_RESIZE", PyObject::int(410)),
            // Error class
            (
                "error",
                PyObject::class(CompactString::from("error"), vec![], IndexMap::new()),
            ),
        ],
    )
}
