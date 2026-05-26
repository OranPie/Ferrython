use super::*;

// ── token module ──

pub fn create_token_module() -> PyObjectRef {
    make_module(
        "token",
        vec![
            ("ENDMARKER", PyObject::int(0)),
            ("NAME", PyObject::int(1)),
            ("NUMBER", PyObject::int(2)),
            ("STRING", PyObject::int(3)),
            ("NEWLINE", PyObject::int(4)),
            ("INDENT", PyObject::int(5)),
            ("DEDENT", PyObject::int(6)),
            ("LPAR", PyObject::int(7)),
            ("RPAR", PyObject::int(8)),
            ("LSQB", PyObject::int(9)),
            ("RSQB", PyObject::int(10)),
            ("COLON", PyObject::int(11)),
            ("COMMA", PyObject::int(12)),
            ("SEMI", PyObject::int(13)),
            ("PLUS", PyObject::int(14)),
            ("MINUS", PyObject::int(15)),
            ("STAR", PyObject::int(16)),
            ("SLASH", PyObject::int(17)),
            ("VBAR", PyObject::int(18)),
            ("AMPER", PyObject::int(19)),
            ("LESS", PyObject::int(20)),
            ("GREATER", PyObject::int(21)),
            ("EQUAL", PyObject::int(22)),
            ("DOT", PyObject::int(23)),
            ("PERCENT", PyObject::int(24)),
            ("LBRACE", PyObject::int(25)),
            ("RBRACE", PyObject::int(26)),
            ("EQEQUAL", PyObject::int(27)),
            ("NOTEQUAL", PyObject::int(28)),
            ("LESSEQUAL", PyObject::int(29)),
            ("GREATEREQUAL", PyObject::int(30)),
            ("TILDE", PyObject::int(31)),
            ("CIRCUMFLEX", PyObject::int(32)),
            ("LEFTSHIFT", PyObject::int(33)),
            ("RIGHTSHIFT", PyObject::int(34)),
            ("DOUBLESTAR", PyObject::int(35)),
            ("PLUSEQUAL", PyObject::int(36)),
            ("MINEQUAL", PyObject::int(37)),
            ("STAREQUAL", PyObject::int(38)),
            ("SLASHEQUAL", PyObject::int(39)),
            ("PERCENTEQUAL", PyObject::int(40)),
            ("AMPEREQUAL", PyObject::int(41)),
            ("VBAREQUAL", PyObject::int(42)),
            ("CIRCUMFLEXEQUAL", PyObject::int(43)),
            ("LEFTSHIFTEQUAL", PyObject::int(44)),
            ("RIGHTSHIFTEQUAL", PyObject::int(45)),
            ("DOUBLESTAREQUAL", PyObject::int(46)),
            ("DOUBLESLASH", PyObject::int(47)),
            ("DOUBLESLASHEQUAL", PyObject::int(48)),
            ("AT", PyObject::int(49)),
            ("ATEQUAL", PyObject::int(50)),
            ("RARROW", PyObject::int(51)),
            ("ELLIPSIS", PyObject::int(52)),
            ("COLONEQUAL", PyObject::int(53)),
            ("OP", PyObject::int(54)),
            ("COMMENT", PyObject::int(55)),
            ("NL", PyObject::int(56)),
            ("ERRORTOKEN", PyObject::int(57)),
            ("ENCODING", PyObject::int(62)),
            ("NT_OFFSET", PyObject::int(256)),
            ("tok_name", {
                let mut map = IndexMap::new();
                for (i, name) in [
                    (0, "ENDMARKER"),
                    (1, "NAME"),
                    (2, "NUMBER"),
                    (3, "STRING"),
                    (4, "NEWLINE"),
                    (5, "INDENT"),
                    (6, "DEDENT"),
                    (54, "OP"),
                    (55, "COMMENT"),
                    (56, "NL"),
                    (57, "ERRORTOKEN"),
                    (62, "ENCODING"),
                ]
                .iter()
                {
                    map.insert(
                        ferrython_core::types::HashableKey::Int(
                            ferrython_core::types::PyInt::Small(*i),
                        ),
                        PyObject::str_val(CompactString::from(*name)),
                    );
                }
                PyObject::dict(map)
            }),
        ],
    )
}
