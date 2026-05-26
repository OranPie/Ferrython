use compact_str::CompactString;
use ferrython_core::error::PyException;
use ferrython_core::object::{make_builtin, make_module, PyObject, PyObjectMethods, PyObjectRef};
use indexmap::IndexMap;

// ── numbers module (stub) ──

pub fn create_numbers_module() -> PyObjectRef {
    // Abstract method that raises NotImplementedError
    fn make_abstract(name: &str) -> PyObjectRef {
        let n = CompactString::from(name);
        PyObject::native_closure(name, move |_args: &[PyObjectRef]| {
            Err(PyException::type_error(format!("{} is abstract", n)))
        })
    }

    // Number — root of the numeric tower
    let mut number_ns = IndexMap::new();
    number_ns.insert(
        CompactString::from("__hash__"),
        make_abstract("Number.__hash__"),
    );
    let number_class = PyObject::class(CompactString::from("Number"), vec![], number_ns);

    // Complex — adds complex arithmetic operations
    let mut complex_ns = IndexMap::new();
    for op in &[
        "__add__",
        "__radd__",
        "__sub__",
        "__rsub__",
        "__mul__",
        "__rmul__",
        "__truediv__",
        "__rtruediv__",
        "__pow__",
        "__rpow__",
        "__neg__",
        "__pos__",
        "__abs__",
        "__complex__",
        "__eq__",
        "__hash__",
        "real",
        "imag",
        "conjugate",
    ] {
        complex_ns.insert(
            CompactString::from(*op),
            make_abstract(&format!("Complex.{}", op)),
        );
    }
    complex_ns.insert(
        CompactString::from("__bool__"),
        make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::bool_val(true))),
    );
    let complex_class = PyObject::class(
        CompactString::from("Complex"),
        vec![number_class.clone()],
        complex_ns,
    );

    // Real — adds ordering and real-valued operations
    let mut real_ns = IndexMap::new();
    for op in &[
        "__float__",
        "__trunc__",
        "__floor__",
        "__ceil__",
        "__round__",
        "__floordiv__",
        "__rfloordiv__",
        "__mod__",
        "__rmod__",
        "__lt__",
        "__le__",
    ] {
        real_ns.insert(
            CompactString::from(*op),
            make_abstract(&format!("Real.{}", op)),
        );
    }
    real_ns.insert(
        CompactString::from("real"),
        make_builtin(|args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::float(0.0));
            }
            Ok(args[0].clone())
        }),
    );
    real_ns.insert(
        CompactString::from("imag"),
        make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::int(0))),
    );
    real_ns.insert(
        CompactString::from("conjugate"),
        make_builtin(|args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::float(0.0));
            }
            Ok(args[0].clone())
        }),
    );
    let real_class = PyObject::class(
        CompactString::from("Real"),
        vec![complex_class.clone()],
        real_ns,
    );

    // Rational — adds numerator/denominator
    let mut rational_ns = IndexMap::new();
    rational_ns.insert(
        CompactString::from("numerator"),
        make_abstract("Rational.numerator"),
    );
    rational_ns.insert(
        CompactString::from("denominator"),
        make_abstract("Rational.denominator"),
    );
    rational_ns.insert(
        CompactString::from("__float__"),
        make_builtin(|args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::float(0.0));
            }
            let self_obj = &args[0];
            if let (Some(num), Some(den)) = (
                self_obj.get_attr("numerator"),
                self_obj.get_attr("denominator"),
            ) {
                let n = num.to_int().unwrap_or(0) as f64;
                let d = den.to_int().unwrap_or(1) as f64;
                return Ok(PyObject::float(if d != 0.0 { n / d } else { f64::NAN }));
            }
            Ok(PyObject::float(0.0))
        }),
    );
    let rational_class = PyObject::class(
        CompactString::from("Rational"),
        vec![real_class.clone()],
        rational_ns,
    );

    // Integral — adds integer-specific operations
    let mut integral_ns = IndexMap::new();
    for op in &[
        "__int__",
        "__index__",
        "__lshift__",
        "__rlshift__",
        "__rshift__",
        "__rrshift__",
        "__and__",
        "__rand__",
        "__xor__",
        "__rxor__",
        "__or__",
        "__ror__",
        "__invert__",
    ] {
        integral_ns.insert(
            CompactString::from(*op),
            make_abstract(&format!("Integral.{}", op)),
        );
    }
    integral_ns.insert(
        CompactString::from("__float__"),
        make_builtin(|args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::float(0.0));
            }
            let v = args[0].to_int().unwrap_or(0);
            Ok(PyObject::float(v as f64))
        }),
    );
    integral_ns.insert(
        CompactString::from("numerator"),
        make_builtin(|args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::int(0));
            }
            Ok(args[0].clone())
        }),
    );
    integral_ns.insert(
        CompactString::from("denominator"),
        make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::int(1))),
    );
    let integral_class = PyObject::class(
        CompactString::from("Integral"),
        vec![rational_class.clone()],
        integral_ns,
    );

    make_module(
        "numbers",
        vec![
            ("Number", number_class),
            ("Complex", complex_class),
            ("Real", real_class),
            ("Rational", rational_class),
            ("Integral", integral_class),
        ],
    )
}
