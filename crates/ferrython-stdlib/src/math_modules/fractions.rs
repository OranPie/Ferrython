use super::number::{bigint_to_object, index_bigint};
use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    check_args, make_builtin, make_module, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::PyInt;
use ferrython_core::types::{float_as_integer_ratio, py_hash_rational};
use indexmap::IndexMap;
use num_bigint::{BigInt, Sign};
use num_integer::Integer;
use num_traits::{One, Signed, ToPrimitive, Zero};

// ── fractions module ─────────────────────────────────────────────────
pub fn create_fractions_module() -> PyObjectRef {
    use ferrython_core::object::{new_shared_fx, InstanceData};

    fn object_to_bigint(obj: &PyObjectRef) -> Option<BigInt> {
        match &obj.payload {
            PyObjectPayload::Int(PyInt::Small(n)) => Some(BigInt::from(*n)),
            PyObjectPayload::Int(PyInt::Big(n)) => Some(n.as_ref().clone()),
            PyObjectPayload::Bool(b) => Some(BigInt::from(if *b { 1 } else { 0 })),
            _ => None,
        }
    }

    fn get_frac_bigint_parts(obj: &PyObjectRef) -> Option<(BigInt, BigInt)> {
        if let PyObjectPayload::Instance(inst) = &obj.payload {
            let attrs = inst.attrs.read();
            if attrs.contains_key("__fraction__") {
                let n = attrs.get("numerator").and_then(object_to_bigint)?;
                let d = attrs.get("denominator").and_then(object_to_bigint)?;
                return Some((n, d));
            }
            if let (Some(n), Some(d)) = (
                attrs.get("numerator").and_then(object_to_bigint),
                attrs.get("denominator").and_then(object_to_bigint),
            ) {
                return Some((n, d));
            }
        }
        object_to_bigint(obj).map(|n| (n, BigInt::one()))
    }

    fn get_fraction_constructor_parts(obj: &PyObjectRef) -> Option<(PyObjectRef, BigInt)> {
        if matches!(&obj.payload, PyObjectPayload::Instance(_)) {
            let numerator = get_descriptor_aware_attr(obj, "numerator")?;
            let denominator =
                get_descriptor_aware_attr(obj, "denominator").and_then(|d| object_to_bigint(&d))?;
            return Some((numerator, denominator));
        }
        object_to_bigint(obj).map(|n| (bigint_to_object(n), BigInt::one()))
    }

    fn get_descriptor_aware_attr(obj: &PyObjectRef, name: &str) -> Option<PyObjectRef> {
        if let PyObjectPayload::Instance(inst) = &obj.payload {
            if let Some(descriptor) = ferrython_core::object::lookup_in_class_mro(&inst.class, name)
            {
                if ferrython_core::object::is_property_like(&descriptor) {
                    let getter = ferrython_core::object::property_field(&descriptor, "fget")?;
                    if matches!(&getter.payload, PyObjectPayload::None) {
                        return None;
                    }
                    return ferrython_core::object::call_callable(&getter, &[obj.clone()]).ok();
                }
            }
        }
        obj.get_attr(name)
    }

    fn is_decimal_instance(obj: &PyObjectRef) -> bool {
        obj.get_attr("__decimal__")
            .is_some_and(|marker| marker.is_truthy())
    }

    fn decimal_as_integer_ratio(obj: &PyObjectRef) -> PyResult<Option<(BigInt, BigInt)>> {
        if !is_decimal_instance(obj) {
            return Ok(None);
        }
        let method = obj
            .get_attr("as_integer_ratio")
            .ok_or_else(|| PyException::type_error("from_decimal requires a Decimal instance"))?;
        let ratio = ferrython_core::object::call_callable(&method, &[])?;
        if let PyObjectPayload::Tuple(items) = &ratio.payload {
            if items.len() == 2 {
                if let (Some(n), Some(d)) =
                    (object_to_bigint(&items[0]), object_to_bigint(&items[1]))
                {
                    return Ok(Some((n, d)));
                }
            }
        }
        Err(PyException::type_error(
            "Decimal.as_integer_ratio() returned invalid ratio",
        ))
    }

    fn get_frac_parts(obj: &PyObjectRef) -> Option<(i64, i64)> {
        let (n, d) = get_frac_bigint_parts(obj)?;
        Some((n.to_i64()?, d.to_i64()?))
    }

    fn is_fraction_instance(obj: &PyObjectRef) -> bool {
        matches!(&obj.payload, PyObjectPayload::Instance(inst) if inst.attrs.read().contains_key("__fraction__"))
    }

    fn fraction_to_f64(obj: &PyObjectRef) -> PyResult<f64> {
        let (n, d) =
            get_frac_bigint_parts(obj).ok_or_else(|| PyException::type_error("not a Fraction"))?;
        let nf = n.to_f64().unwrap_or_else(|| {
            if n.sign() == Sign::Minus {
                f64::NEG_INFINITY
            } else {
                f64::INFINITY
            }
        });
        let df = d.to_f64().unwrap_or(f64::INFINITY);
        Ok(nf / df)
    }

    fn fraction_other_float(obj: &PyObjectRef) -> Option<f64> {
        match &obj.payload {
            PyObjectPayload::Float(f) => Some(*f),
            _ => None,
        }
    }

    fn fraction_other_complex(obj: &PyObjectRef) -> Option<(f64, f64)> {
        match &obj.payload {
            PyObjectPayload::Complex { real, imag } => Some((*real, *imag)),
            _ => None,
        }
    }

    fn fraction_complex_value(obj: &PyObjectRef) -> Option<(f64, f64)> {
        if let Some(value) = fraction_other_complex(obj) {
            return Some(value);
        }
        fraction_other_float(obj).map(|real| (real, 0.0))
    }

    fn complex_div(ar: f64, ai: f64, br: f64, bi: f64) -> PyResult<PyObjectRef> {
        if br == 0.0 && bi == 0.0 {
            return Err(PyException::zero_division_error("complex division by zero"));
        }
        if bi == 0.0 {
            return Ok(PyObject::complex(ar / br, ai / br));
        }
        let denom = br * br + bi * bi;
        Ok(PyObject::complex(
            (ar * br + ai * bi) / denom,
            (ai * br - ar * bi) / denom,
        ))
    }

    fn fraction_zero_division(num: &BigInt) -> PyException {
        PyException::new(
            ferrython_core::error::ExceptionKind::ZeroDivisionError,
            format!("Fraction({}, 0)", num),
        )
    }

    fn bigint_floor_div(a: BigInt, b: BigInt) -> BigInt {
        a.div_floor(&b)
    }

    fn bigint_mod_fraction(
        an: BigInt,
        ad: BigInt,
        bn: BigInt,
        bd: BigInt,
    ) -> PyResult<PyObjectRef> {
        if bn.is_zero() {
            return Err(PyException::zero_division_error("Fraction modulo by zero"));
        }
        let q = bigint_floor_div(&an * &bd, &ad * &bn);
        let rn = an * bd.clone() - q * bn * ad.clone();
        let rd = ad * bd;
        Ok(make_frac_bigint_instance(rn, rd))
    }

    fn python_float_mod(a: f64, b: f64) -> PyResult<PyObjectRef> {
        if b == 0.0 {
            return Err(PyException::zero_division_error("float modulo"));
        }
        if b.is_infinite() && a.is_finite() {
            if b.is_sign_positive() {
                return Ok(PyObject::float(if a >= 0.0 { a } else { f64::INFINITY }));
            }
            return Ok(PyObject::float(if a > 0.0 { f64::NEG_INFINITY } else { a }));
        }
        let r = a % b;
        let r = if (r != 0.0) && ((r < 0.0) != (b < 0.0)) {
            r + b
        } else {
            r
        };
        Ok(PyObject::float(r))
    }

    fn float_to_bigint_fraction(f: f64) -> Option<(BigInt, BigInt)> {
        if !f.is_finite() {
            return None;
        }
        Some(float_as_integer_ratio(f))
    }

    fn get_frac_cmp_parts(obj: &PyObjectRef) -> Option<(BigInt, BigInt)> {
        if let Some(parts) = get_frac_bigint_parts(obj) {
            return Some(parts);
        }
        if let PyObjectPayload::Instance(inst) = &obj.payload {
            let attrs = inst.attrs.read();
            if attrs.contains_key("__decimal__") {
                let s = attrs.get("_value").and_then(|v| v.as_str())?;
                return decimal_string_ratio(s);
            }
        }
        if let Ok(Some(parts)) = decimal_as_integer_ratio(obj) {
            return Some(parts);
        }
        if let PyObjectPayload::Float(f) = &obj.payload {
            if f.is_infinite() {
                return Some((
                    if f.is_sign_negative() {
                        -BigInt::one()
                    } else {
                        BigInt::one()
                    },
                    BigInt::zero(),
                ));
            }
            return float_to_bigint_fraction(*f);
        }
        if let PyObjectPayload::Complex { real, imag } = &obj.payload {
            if *imag == 0.0 && real.is_finite() {
                return Some(float_as_integer_ratio(*real));
            }
        }
        None
    }

    fn decimal_string_ratio(s: &str) -> Option<(BigInt, BigInt)> {
        let s = s.trim();
        let check = s.trim_start_matches('+').trim_start_matches('-');
        if check.eq_ignore_ascii_case("nan") || check.eq_ignore_ascii_case("snan") {
            return None;
        }
        if s.eq_ignore_ascii_case("infinity")
            || s.eq_ignore_ascii_case("+infinity")
            || s.eq_ignore_ascii_case("inf")
            || s.eq_ignore_ascii_case("+inf")
        {
            return Some((BigInt::one(), BigInt::zero()));
        }
        if s.eq_ignore_ascii_case("-infinity") || s.eq_ignore_ascii_case("-inf") {
            return Some((-BigInt::one(), BigInt::zero()));
        }
        let (negative, body) = if let Some(rest) = s.strip_prefix('-') {
            (true, rest)
        } else if let Some(rest) = s.strip_prefix('+') {
            (false, rest)
        } else {
            (false, s)
        };
        if body.is_empty() || body == "." {
            return None;
        }
        let (mantissa, exp) =
            if let Some((m, e)) = body.split_once('e').or_else(|| body.split_once('E')) {
                (m, e.parse::<i64>().ok()?)
            } else {
                (body, 0)
            };
        let mut digits = String::new();
        let mut scale = 0i64;
        if let Some((int_part, frac_part)) = mantissa.split_once('.') {
            if int_part.is_empty() && frac_part.is_empty() {
                return None;
            }
            digits.push_str(int_part);
            digits.push_str(frac_part);
            scale = frac_part.len() as i64;
        } else {
            if mantissa.is_empty() {
                return None;
            }
            digits.push_str(mantissa);
        }
        if !digits.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        if digits.is_empty() || digits.chars().all(|c| c == '0') {
            return Some((BigInt::zero(), BigInt::one()));
        }
        let mut numerator = digits.parse::<BigInt>().ok()?;
        if negative {
            numerator = -numerator;
        }
        let power = scale - exp;
        if power.abs() > 10_000 {
            return None;
        }
        if power >= 0 {
            Some((numerator, BigInt::from(10u8).pow(power as u32)))
        } else {
            Some((
                numerator * BigInt::from(10u8).pow((-power) as u32),
                BigInt::one(),
            ))
        }
    }

    fn decimal_extreme_sign(obj: &PyObjectRef) -> Option<bool> {
        if let PyObjectPayload::Instance(inst) = &obj.payload {
            let attrs = inst.attrs.read();
            if attrs.contains_key("__decimal__") {
                let s = attrs.get("_value").and_then(|v| v.as_str())?;
                let check = s.trim().trim_start_matches('+').trim_start_matches('-');
                if check
                    .split_once('e')
                    .or_else(|| check.split_once('E'))
                    .and_then(|(_, e)| e.parse::<i64>().ok())
                    .map(|exp| exp.abs() > 10_000)
                    .unwrap_or(false)
                {
                    return Some(s.trim().starts_with('-'));
                }
            }
        }
        None
    }

    fn decimal_str_to_fraction(s: &str) -> PyResult<PyObjectRef> {
        let s = s.trim();
        if let Some((n, d)) = decimal_string_ratio(s) {
            if d.is_zero() {
                let check = s.trim_start_matches('+').trim_start_matches('-');
                if check.eq_ignore_ascii_case("nan") || check.eq_ignore_ascii_case("snan") {
                    return Err(PyException::value_error(
                        "cannot convert NaN to integer ratio",
                    ));
                }
                return Err(PyException::overflow_error(
                    "cannot convert Infinity to integer ratio",
                ));
            }
            return Ok(make_frac_bigint_instance(n, d));
        }
        Err(PyException::value_error(format!(
            "Invalid literal for Fraction: '{}'",
            s
        )))
    }

    fn make_frac_bigint_instance(num: BigInt, den: BigInt) -> PyObjectRef {
        let g = num.abs().gcd(&den.abs());
        let mut num = num / &g;
        let mut den = den / &g;
        if den.sign() == Sign::Minus {
            num = -num;
            den = -den;
        }
        make_frac_normalized_instance(num, den)
    }

    fn make_frac_bigint_instance_for_class(
        cls: PyObjectRef,
        num: BigInt,
        den: BigInt,
    ) -> PyObjectRef {
        let g = num.abs().gcd(&den.abs());
        let mut num = num / &g;
        let mut den = den / &g;
        if den.sign() == Sign::Minus {
            num = -num;
            den = -den;
        }
        let num_obj = bigint_to_object(num.clone());
        let den_obj = bigint_to_object(den.clone());
        let class_flags = InstanceData::compute_flags(&cls);
        let inst = PyObject::wrap(PyObjectPayload::Instance(std::mem::ManuallyDrop::new(
            Box::new(InstanceData {
                class: cls,
                attrs: new_shared_fx(),
                is_special: true,
                dict_storage: None,
                class_flags,
                finalizer_state: std::cell::Cell::new(0),
            }),
        )));
        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut w = inst_data.attrs.write();
            w.insert(
                CompactString::from("__fraction__"),
                PyObject::bool_val(true),
            );
            w.insert(CompactString::from("numerator"), num_obj);
            w.insert(CompactString::from("denominator"), den_obj);
            w.insert(CompactString::from("_numerator"), bigint_to_object(num));
            w.insert(CompactString::from("_denominator"), bigint_to_object(den));
        }
        inst
    }

    fn make_frac_instance(num: i64, den: i64) -> PyObjectRef {
        make_frac_bigint_instance(BigInt::from(num), BigInt::from(den))
    }

    fn make_frac_normalized_instance(num: BigInt, den: BigInt) -> PyObjectRef {
        let num_obj = bigint_to_object(num.clone());
        let den_obj = bigint_to_object(den.clone());
        let mut frac_ns = IndexMap::new();
        frac_ns.insert(CompactString::from("__add__"), make_builtin(frac_add));
        frac_ns.insert(CompactString::from("__radd__"), make_builtin(frac_add));
        frac_ns.insert(CompactString::from("__sub__"), make_builtin(frac_sub));
        frac_ns.insert(CompactString::from("__rsub__"), make_builtin(frac_rsub));
        frac_ns.insert(CompactString::from("__mul__"), make_builtin(frac_mul));
        frac_ns.insert(CompactString::from("__rmul__"), make_builtin(frac_mul));
        frac_ns.insert(CompactString::from("__truediv__"), make_builtin(frac_div));
        frac_ns.insert(CompactString::from("__rmod__"), make_builtin(frac_rmod));
        frac_ns.insert(CompactString::from("__divmod__"), make_builtin(frac_divmod));
        frac_ns.insert(
            CompactString::from("__rdivmod__"),
            make_builtin(frac_rdivmod),
        );
        frac_ns.insert(CompactString::from("__rpow__"), make_builtin(frac_rpow));
        frac_ns.insert(
            CompactString::from("__floordiv__"),
            make_builtin(frac_floordiv),
        );
        frac_ns.insert(CompactString::from("__neg__"), make_builtin(frac_neg));
        frac_ns.insert(CompactString::from("__pos__"), make_builtin(frac_pos));
        frac_ns.insert(CompactString::from("__abs__"), make_builtin(frac_abs));
        frac_ns.insert(
            CompactString::from("__init__"),
            make_builtin(|_args| Ok(PyObject::none())),
        );
        frac_ns.insert(
            CompactString::from("__setattr__"),
            make_builtin(frac_setattr),
        );
        frac_ns.insert(CompactString::from("__eq__"), make_builtin(frac_eq));
        frac_ns.insert(CompactString::from("__lt__"), make_builtin(frac_lt));
        frac_ns.insert(CompactString::from("__le__"), make_builtin(frac_le));
        frac_ns.insert(CompactString::from("__gt__"), make_builtin(frac_gt));
        frac_ns.insert(CompactString::from("__ge__"), make_builtin(frac_ge));
        frac_ns.insert(CompactString::from("__hash__"), make_builtin(frac_hash));
        frac_ns.insert(CompactString::from("__str__"), make_builtin(frac_str));
        frac_ns.insert(CompactString::from("__repr__"), make_builtin(frac_repr));
        frac_ns.insert(CompactString::from("__float__"), make_builtin(frac_float));
        frac_ns.insert(CompactString::from("__int__"), make_builtin(frac_int));
        frac_ns.insert(CompactString::from("__trunc__"), make_builtin(frac_int));
        frac_ns.insert(CompactString::from("__copy__"), make_builtin(frac_copy));
        frac_ns.insert(
            CompactString::from("__deepcopy__"),
            make_builtin(frac_deepcopy),
        );
        frac_ns.insert(CompactString::from("__floor__"), make_builtin(frac_floor));
        frac_ns.insert(CompactString::from("__ceil__"), make_builtin(frac_ceil));
        frac_ns.insert(CompactString::from("__round__"), make_builtin(frac_round));
        frac_ns.insert(CompactString::from("__bool__"), make_builtin(frac_bool));
        frac_ns.insert(
            CompactString::from("limit_denominator"),
            make_builtin(frac_limit_denominator),
        );
        frac_ns.insert(CompactString::from("__pow__"), make_builtin(frac_pow));
        frac_ns.insert(CompactString::from("__mod__"), make_builtin(frac_mod));
        frac_ns.insert(
            CompactString::from("__rtruediv__"),
            make_builtin(frac_rtruediv),
        );
        frac_ns.insert(
            CompactString::from("__rfloordiv__"),
            make_builtin(frac_rfloordiv),
        );
        frac_ns.insert(
            CompactString::from("__format__"),
            make_builtin(|args: &[PyObjectRef]| {
                if args.is_empty() {
                    return Ok(PyObject::str_val(CompactString::from("0")));
                }
                let (n, d) =
                    get_frac_bigint_parts(&args[0]).unwrap_or((BigInt::zero(), BigInt::one()));
                let spec = args.get(1).map(|a| a.py_to_string()).unwrap_or_default();
                if spec.is_empty() || spec == "s" {
                    if d == BigInt::one() {
                        return Ok(PyObject::str_val(CompactString::from(format!("{}", n))));
                    }
                    return Ok(PyObject::str_val(CompactString::from(format!(
                        "{}/{}",
                        n, d
                    ))));
                }
                // For numeric format specs, convert to float
                let f = fraction_to_f64(&args[0])?;
                Ok(PyObject::str_val(CompactString::from(format!("{}", f))))
            }),
        );
        frac_ns.insert(
            CompactString::from("as_integer_ratio"),
            make_builtin(|args: &[PyObjectRef]| {
                if args.is_empty() {
                    return Ok(PyObject::tuple(vec![PyObject::int(0), PyObject::int(1)]));
                }
                let (n, d) =
                    get_frac_bigint_parts(&args[0]).unwrap_or((BigInt::zero(), BigInt::one()));
                Ok(PyObject::tuple(vec![
                    bigint_to_object(n),
                    bigint_to_object(d),
                ]))
            }),
        );
        let class = PyObject::class(CompactString::from("Fraction"), vec![], frac_ns);
        let class_flags = InstanceData::compute_flags(&class);
        let inst = PyObject::wrap(PyObjectPayload::Instance(std::mem::ManuallyDrop::new(
            Box::new(InstanceData {
                class,
                attrs: new_shared_fx(),
                is_special: true,
                dict_storage: None,
                class_flags,
                finalizer_state: std::cell::Cell::new(0),
            }),
        )));
        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut w = inst_data.attrs.write();
            w.insert(
                CompactString::from("__fraction__"),
                PyObject::bool_val(true),
            );
            w.insert(CompactString::from("numerator"), num_obj);
            w.insert(CompactString::from("denominator"), den_obj);
            w.insert(CompactString::from("_numerator"), bigint_to_object(num));
            w.insert(CompactString::from("_denominator"), bigint_to_object(den));
        }
        inst
    }

    fn make_frac_raw_instance(numerator: PyObjectRef, denominator: BigInt) -> PyObjectRef {
        let frac = make_frac_normalized_instance(BigInt::zero(), denominator.clone());
        if let PyObjectPayload::Instance(ref inst_data) = frac.payload {
            let mut w = inst_data.attrs.write();
            w.insert(CompactString::from("numerator"), numerator.clone());
            w.insert(CompactString::from("_numerator"), numerator);
            w.insert(
                CompactString::from("denominator"),
                bigint_to_object(denominator.clone()),
            );
            w.insert(
                CompactString::from("_denominator"),
                bigint_to_object(denominator),
            );
        }
        frac
    }

    fn unsupported_fraction_operand(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let other = args.get(1).unwrap_or_else(|| &args[0]);
        if let PyObjectPayload::Instance(inst) = &other.payload {
            if inst.attrs.read().contains_key("__decimal__") {
                return Err(PyException::type_error(
                    "unsupported operand type(s) for Fraction and Decimal",
                ));
            }
        }
        Ok(PyObject::not_implemented())
    }

    fn frac_setattr(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 3 {
            return Err(PyException::type_error(
                "__setattr__ requires name and value",
            ));
        }
        let name = args[1].py_to_string();
        if let PyObjectPayload::Instance(inst) = &args[0].payload {
            match name.as_str() {
                "_numerator" => {
                    let mut attrs = inst.attrs.write();
                    attrs.insert(CompactString::from("_numerator"), args[2].clone());
                    attrs.insert(CompactString::from("numerator"), args[2].clone());
                    return Ok(PyObject::none());
                }
                "_denominator" => {
                    let mut attrs = inst.attrs.write();
                    attrs.insert(CompactString::from("_denominator"), args[2].clone());
                    attrs.insert(CompactString::from("denominator"), args[2].clone());
                    return Ok(PyObject::none());
                }
                _ => {}
            }
        }
        Err(PyException::attribute_error("can't set attribute"))
    }

    fn frac_add(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error("Fraction.__add__ requires 2 args"));
        }
        let (an, ad) = get_frac_bigint_parts(&args[0])
            .ok_or_else(|| PyException::type_error("not a Fraction"))?;
        if let Some(f) = fraction_other_float(&args[1]) {
            return Ok(PyObject::float(fraction_to_f64(&args[0])? + f));
        }
        if let Some((real, imag)) = fraction_other_complex(&args[1]) {
            return Ok(PyObject::complex(fraction_to_f64(&args[0])? + real, imag));
        }
        let Some((bn, bd)) = get_frac_bigint_parts(&args[1]) else {
            return unsupported_fraction_operand(args);
        };
        Ok(make_frac_bigint_instance(&an * &bd + &bn * &ad, ad * bd))
    }

    fn frac_sub(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error("Fraction.__sub__ requires 2 args"));
        }
        let (an, ad) = get_frac_bigint_parts(&args[0])
            .ok_or_else(|| PyException::type_error("not a Fraction"))?;
        if let Some(f) = fraction_other_float(&args[1]) {
            return Ok(PyObject::float(fraction_to_f64(&args[0])? - f));
        }
        if let Some((real, imag)) = fraction_other_complex(&args[1]) {
            return Ok(PyObject::complex(fraction_to_f64(&args[0])? - real, -imag));
        }
        let Some((bn, bd)) = get_frac_bigint_parts(&args[1]) else {
            return unsupported_fraction_operand(args);
        };
        Ok(make_frac_bigint_instance(&an * &bd - &bn * &ad, ad * bd))
    }

    fn frac_rsub(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error("requires 2 args"));
        }
        let (an, ad) = get_frac_bigint_parts(&args[0])
            .ok_or_else(|| PyException::type_error("not a Fraction"))?;
        if let Some(f) = fraction_other_float(&args[1]) {
            return Ok(PyObject::float(f - fraction_to_f64(&args[0])?));
        }
        if let Some((real, imag)) = fraction_complex_value(&args[1]) {
            return Ok(PyObject::complex(real - fraction_to_f64(&args[0])?, imag));
        }
        let Some((bn, bd)) = get_frac_bigint_parts(&args[1]) else {
            return unsupported_fraction_operand(args);
        };
        Ok(make_frac_bigint_instance(&bn * &ad - &an * &bd, ad * bd))
    }

    fn frac_mul(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error("Fraction.__mul__ requires 2 args"));
        }
        let (an, ad) = get_frac_bigint_parts(&args[0])
            .ok_or_else(|| PyException::type_error("not a Fraction"))?;
        if let Some(f) = fraction_other_float(&args[1]) {
            return Ok(PyObject::float(fraction_to_f64(&args[0])? * f));
        }
        if let Some((real, imag)) = fraction_other_complex(&args[1]) {
            let lhs = fraction_to_f64(&args[0])?;
            return Ok(PyObject::complex(lhs * real, lhs * imag));
        }
        let Some((bn, bd)) = get_frac_bigint_parts(&args[1]) else {
            return unsupported_fraction_operand(args);
        };
        Ok(make_frac_bigint_instance(an * bn, ad * bd))
    }

    fn frac_div(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error(
                "Fraction.__truediv__ requires 2 args",
            ));
        }
        let (an, ad) = get_frac_bigint_parts(&args[0])
            .ok_or_else(|| PyException::type_error("not a Fraction"))?;
        if let Some(f) = fraction_other_float(&args[1]) {
            if f == 0.0 {
                return Err(PyException::zero_division_error("float division by zero"));
            }
            return Ok(PyObject::float(fraction_to_f64(&args[0])? / f));
        }
        if let Some((real, imag)) = fraction_other_complex(&args[1]) {
            return complex_div(fraction_to_f64(&args[0])?, 0.0, real, imag);
        }
        let Some((bn, bd)) = get_frac_bigint_parts(&args[1]) else {
            return unsupported_fraction_operand(args);
        };
        if bn.is_zero() {
            return Err(PyException::zero_division_error(
                "Fraction division by zero",
            ));
        }
        Ok(make_frac_bigint_instance(an * bd, ad * bn))
    }

    fn frac_floordiv(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error("requires 2 args"));
        }
        if let Some(f) = fraction_other_float(&args[1]) {
            if f == 0.0 {
                return Err(PyException::zero_division_error(
                    "float floor division by zero",
                ));
            }
            return Ok(PyObject::float((fraction_to_f64(&args[0])? / f).floor()));
        }
        let (an, ad) = get_frac_bigint_parts(&args[0])
            .ok_or_else(|| PyException::type_error("not a Fraction"))?;
        let Some((bn, bd)) = get_frac_bigint_parts(&args[1]) else {
            return unsupported_fraction_operand(args);
        };
        if bn.is_zero() {
            return Err(PyException::zero_division_error(
                "Fraction division by zero",
            ));
        }
        Ok(bigint_to_object(bigint_floor_div(an * bd, ad * bn)))
    }

    fn frac_neg(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let (n, d) = get_frac_bigint_parts(&args[0])
            .ok_or_else(|| PyException::type_error("not a Fraction"))?;
        Ok(make_frac_bigint_instance(-n, d))
    }

    fn frac_abs(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let (n, d) = get_frac_bigint_parts(&args[0])
            .ok_or_else(|| PyException::type_error("not a Fraction"))?;
        Ok(make_frac_bigint_instance(n.abs(), d))
    }

    fn frac_pos(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Err(PyException::type_error("not a Fraction"));
        }
        Ok(args[0].clone())
    }

    fn frac_eq(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Ok(PyObject::bool_val(false));
        }
        let a = get_frac_cmp_parts(&args[0]);
        let b = get_frac_cmp_parts(&args[1]);
        match (a, b) {
            (Some((an, ad)), Some((bn, bd)))
                if is_fraction_instance(&args[0]) && is_fraction_instance(&args[1]) =>
            {
                Ok(PyObject::bool_val(an == bn && ad == bd))
            }
            (Some((an, ad)), Some((bn, bd))) => Ok(PyObject::bool_val(an * bd == bn * ad)),
            _ => Ok(PyObject::not_implemented()),
        }
    }

    fn frac_lt(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Ok(PyObject::bool_val(false));
        }
        if let Some(neg) = decimal_extreme_sign(&args[0]) {
            return Ok(PyObject::bool_val(neg));
        }
        if let Some(neg) = decimal_extreme_sign(&args[1]) {
            return Ok(PyObject::bool_val(!neg));
        }
        let Some((an, ad)) = get_frac_cmp_parts(&args[0]) else {
            return Ok(PyObject::not_implemented());
        };
        let Some((bn, bd)) = get_frac_cmp_parts(&args[1]) else {
            return Ok(PyObject::not_implemented());
        };
        Ok(PyObject::bool_val(an * bd < bn * ad))
    }

    fn frac_le(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Ok(PyObject::bool_val(false));
        }
        if let Some(neg) = decimal_extreme_sign(&args[0]) {
            return Ok(PyObject::bool_val(neg));
        }
        if let Some(neg) = decimal_extreme_sign(&args[1]) {
            return Ok(PyObject::bool_val(!neg));
        }
        let Some((an, ad)) = get_frac_cmp_parts(&args[0]) else {
            return Ok(PyObject::not_implemented());
        };
        let Some((bn, bd)) = get_frac_cmp_parts(&args[1]) else {
            return Ok(PyObject::not_implemented());
        };
        Ok(PyObject::bool_val(an * bd <= bn * ad))
    }

    fn frac_gt(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Ok(PyObject::bool_val(false));
        }
        if let Some(neg) = decimal_extreme_sign(&args[0]) {
            return Ok(PyObject::bool_val(!neg));
        }
        if let Some(neg) = decimal_extreme_sign(&args[1]) {
            return Ok(PyObject::bool_val(neg));
        }
        let Some((an, ad)) = get_frac_cmp_parts(&args[0]) else {
            return Ok(PyObject::not_implemented());
        };
        let Some((bn, bd)) = get_frac_cmp_parts(&args[1]) else {
            return Ok(PyObject::not_implemented());
        };
        Ok(PyObject::bool_val(an * bd > bn * ad))
    }

    fn frac_ge(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Ok(PyObject::bool_val(false));
        }
        if let Some(neg) = decimal_extreme_sign(&args[0]) {
            return Ok(PyObject::bool_val(!neg));
        }
        if let Some(neg) = decimal_extreme_sign(&args[1]) {
            return Ok(PyObject::bool_val(neg));
        }
        let Some((an, ad)) = get_frac_cmp_parts(&args[0]) else {
            return Ok(PyObject::not_implemented());
        };
        let Some((bn, bd)) = get_frac_cmp_parts(&args[1]) else {
            return Ok(PyObject::not_implemented());
        };
        Ok(PyObject::bool_val(an * bd >= bn * ad))
    }

    fn frac_hash(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let (n, d) = get_frac_bigint_parts(&args[0]).unwrap_or((BigInt::zero(), BigInt::one()));
        Ok(PyObject::int(py_hash_rational(&n, &d)))
    }

    fn frac_str(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let (n, d) = get_frac_bigint_parts(&args[0]).unwrap_or((BigInt::zero(), BigInt::one()));
        let s = if d == BigInt::one() {
            format!("{}", n)
        } else {
            format!("{}/{}", n, d)
        };
        Ok(PyObject::str_val(CompactString::from(s)))
    }

    fn frac_repr(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let (n, d) = get_frac_bigint_parts(&args[0]).unwrap_or((BigInt::zero(), BigInt::one()));
        Ok(PyObject::str_val(CompactString::from(format!(
            "Fraction({}, {})",
            n, d
        ))))
    }

    fn frac_float(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let (n, d) = get_frac_bigint_parts(&args[0]).unwrap_or((BigInt::zero(), BigInt::one()));
        if d.is_zero() {
            return Err(PyException::zero_division_error("division by zero"));
        }
        if let (Some(nf), Some(df)) = (n.to_f64(), d.to_f64()) {
            if nf.is_finite() && df.is_finite() {
                return Ok(PyObject::float(nf / df));
            }
        }
        let n_bits = n.bits() as i64;
        let d_bits = d.bits() as i64;
        let n_shift = (n_bits - 1020).max(0) as usize;
        let d_shift = (d_bits - 1020).max(0) as usize;
        let ns = if n_shift > 0 {
            &n >> n_shift
        } else {
            n.clone()
        };
        let ds = if d_shift > 0 {
            &d >> d_shift
        } else {
            d.clone()
        };
        let nf = ns
            .to_f64()
            .ok_or_else(|| PyException::overflow_error("int too large to convert to float"))?;
        let df = ds
            .to_f64()
            .ok_or_else(|| PyException::overflow_error("int too large to convert to float"))?;
        Ok(PyObject::float(
            (nf / df) * 2f64.powi((n_shift as i32) - (d_shift as i32)),
        ))
    }

    fn fraction_from_decimal_string_for_class(
        cls: Option<&PyObjectRef>,
        s: &str,
    ) -> PyResult<PyObjectRef> {
        let frac = decimal_str_to_fraction(s)?;
        if let Some(cls) = cls {
            if let Some((n, d)) = get_frac_bigint_parts(&frac) {
                return Ok(make_frac_bigint_instance_for_class(cls.clone(), n, d));
            }
        }
        Ok(frac)
    }

    fn fraction_base_type_name(obj: &PyObjectRef) -> bool {
        obj.type_name() == "Fraction"
    }

    fn frac_copy(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Err(PyException::type_error("__copy__ requires self"));
        }
        if fraction_base_type_name(&args[0]) {
            return Ok(args[0].clone());
        }
        let (n, d) = get_frac_bigint_parts(&args[0])
            .ok_or_else(|| PyException::type_error("not a Fraction"))?;
        if let PyObjectPayload::Instance(inst) = &args[0].payload {
            return Ok(make_frac_bigint_instance_for_class(
                inst.class.clone(),
                n,
                d,
            ));
        }
        Err(PyException::type_error("not a Fraction"))
    }

    fn frac_deepcopy(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        frac_copy(args)
    }

    fn frac_int(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let (n, d) = get_frac_bigint_parts(&args[0]).unwrap_or((BigInt::zero(), BigInt::one()));
        Ok(bigint_to_object(n / d))
    }

    fn frac_floor(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let (n, d) = get_frac_bigint_parts(&args[0])
            .ok_or_else(|| PyException::type_error("not a Fraction"))?;
        Ok(bigint_to_object(n.div_floor(&d)))
    }

    fn frac_ceil(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let (n, d) = get_frac_bigint_parts(&args[0])
            .ok_or_else(|| PyException::type_error("not a Fraction"))?;
        Ok(bigint_to_object(n.div_ceil(&d)))
    }

    fn round_div_half_even(n: BigInt, d: BigInt) -> BigInt {
        let q = n.div_floor(&d);
        let r = n - &q * &d;
        let twice = &r * 2;
        if twice < d {
            q
        } else if twice > d || q.is_odd() {
            q + 1
        } else {
            q
        }
    }

    fn frac_round(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let (n, d) = get_frac_bigint_parts(&args[0])
            .ok_or_else(|| PyException::type_error("not a Fraction"))?;
        if args.len() < 2 || matches!(&args[1].payload, PyObjectPayload::None) {
            return Ok(bigint_to_object(round_div_half_even(n, d)));
        }
        let ndigits = args[1].to_int()?;
        if ndigits >= 0 {
            let scale = BigInt::from(10u8).pow(ndigits as u32);
            let rounded = round_div_half_even(n * &scale, d);
            Ok(make_frac_bigint_instance(rounded, scale))
        } else {
            let scale = BigInt::from(10u8).pow((-ndigits) as u32);
            let rounded = round_div_half_even(n, d * &scale);
            Ok(make_frac_bigint_instance(rounded * scale, BigInt::one()))
        }
    }

    fn frac_bool(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if let PyObjectPayload::Instance(inst) = &args[0].payload {
            if let Some(numerator) = inst.attrs.read().get("numerator").cloned() {
                if !matches!(
                    &numerator.payload,
                    PyObjectPayload::Int(_) | PyObjectPayload::Bool(_)
                ) {
                    if let Some(method) = numerator.get_attr("__bool__") {
                        let result = ferrython_core::object::call_callable(&method, &[])?;
                        if let PyObjectPayload::Bool(value) = &result.payload {
                            return Ok(PyObject::bool_val(*value));
                        }
                    }
                    if let PyObjectPayload::Instance(num_inst) = &numerator.payload {
                        if let Some(value) = num_inst.attrs.read().get("value").cloned() {
                            return Ok(PyObject::bool_val(value.is_truthy()));
                        }
                    }
                    return Ok(PyObject::bool_val(numerator.is_truthy()));
                }
            }
        }
        let (n, _) = get_frac_bigint_parts(&args[0]).unwrap_or((BigInt::zero(), BigInt::one()));
        Ok(PyObject::bool_val(!n.is_zero()))
    }

    fn frac_limit_denominator(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let (n, d) = get_frac_parts(&args[0]).unwrap_or((0, 1));
        let max_den = if args.len() > 1 {
            args[1].to_int().unwrap_or(1_000_000)
        } else {
            1_000_000
        };
        if max_den < 1 {
            return Err(PyException::value_error(
                "max_denominator should be at least 1",
            ));
        }
        if d <= max_den {
            return Ok(make_frac_instance(n, d));
        }
        // CPython algorithm: continued fraction convergents (handles negative n)
        let mut p0: i64 = 0;
        let mut q0: i64 = 1;
        let mut p1: i64 = 1;
        let mut q1: i64 = 0;
        let mut nn = n;
        let mut dd = d;
        loop {
            let a = nn.div_euclid(dd);
            let q2 = q0 + a * q1;
            if q2 > max_den {
                break;
            }
            let new_p1 = p0 + a * p1;
            let new_q1 = q2;
            p0 = p1;
            q0 = q1;
            p1 = new_p1;
            q1 = new_q1;
            let tmp = nn - a * dd;
            nn = dd;
            dd = tmp;
            if dd == 0 {
                break;
            }
        }
        let k = if q1 != 0 { (max_den - q0) / q1 } else { 0 };
        let (bound1_n, bound1_d) = (p0 + k * p1, q0 + k * q1);
        // bound2 = p1/q1 (convergent), bound1 = semi-convergent
        // Return convergent if at least as close, matching CPython tie-breaking
        let err2 = (n as i128 * q1 as i128 - d as i128 * p1 as i128).unsigned_abs();
        let err1 = (n as i128 * bound1_d as i128 - d as i128 * bound1_n as i128).unsigned_abs();
        let (rn, rd) =
            if err2 * (bound1_d as i128).unsigned_abs() <= err1 * (q1 as i128).unsigned_abs() {
                (p1, q1)
            } else {
                (bound1_n, bound1_d)
            };
        Ok(make_frac_instance(rn, rd))
    }

    fn frac_pow(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error("Fraction.__pow__ requires 2 args"));
        }
        if let Some(f) = fraction_other_float(&args[1]) {
            return Ok(PyObject::float(fraction_to_f64(&args[0])?.powf(f)));
        }
        if let Some((real, imag)) = fraction_other_complex(&args[1]) {
            if imag == 0.0 {
                return Ok(PyObject::complex(
                    fraction_to_f64(&args[0])?.powf(real),
                    0.0,
                ));
            }
            return Ok(PyObject::complex(f64::NAN, f64::NAN));
        }
        let (n, d) = get_frac_bigint_parts(&args[0]).unwrap_or((BigInt::zero(), BigInt::one()));
        let Some((en, ed)) = get_frac_bigint_parts(&args[1]) else {
            return unsupported_fraction_operand(args);
        };
        if ed == BigInt::one() {
            let exp = en
                .to_i64()
                .ok_or_else(|| PyException::overflow_error("Fraction exponent too large"))?;
            let e = exp.unsigned_abs() as u32;
            if exp >= 0 {
                Ok(make_frac_bigint_instance(n.pow(e), d.pow(e)))
            } else {
                if n.is_zero() {
                    return Err(PyException::zero_division_error(
                        "Fraction division by zero",
                    ));
                }
                Ok(make_frac_bigint_instance(d.pow(e), n.pow(e)))
            }
        } else {
            let base = fraction_to_f64(&args[0])?;
            let exp = en.to_f64().unwrap_or(0.0) / ed.to_f64().unwrap_or(1.0);
            if base < 0.0 {
                let mag = (-base).powf(exp);
                Ok(PyObject::complex(
                    mag * (std::f64::consts::PI * exp).cos(),
                    mag * (std::f64::consts::PI * exp).sin(),
                ))
            } else {
                Ok(PyObject::float(base.powf(exp)))
            }
        }
    }

    fn frac_rpow(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error("requires 2 args"));
        }
        let Some((en, ed)) = get_frac_bigint_parts(&args[0]) else {
            return unsupported_fraction_operand(args);
        };
        if ed == BigInt::one() {
            let exp = en
                .to_i64()
                .ok_or_else(|| PyException::overflow_error("Fraction exponent too large"))?;
            if let Some(base) = object_to_bigint(&args[1]) {
                if exp >= 0 {
                    return Ok(bigint_to_object(base.pow(exp as u32)));
                }
                if base.is_zero() {
                    return Err(PyException::zero_division_error(
                        "Fraction division by zero",
                    ));
                }
                return Ok(make_frac_bigint_instance(
                    BigInt::one(),
                    base.pow((-exp) as u32),
                ));
            }
        }
        let exp = en.to_f64().unwrap_or(0.0) / ed.to_f64().unwrap_or(1.0);
        if let Some(f) = fraction_other_float(&args[1]) {
            return Ok(PyObject::float(f.powf(exp)));
        }
        if let Some((real, imag)) = fraction_other_complex(&args[1]) {
            if imag == 0.0 {
                return Ok(PyObject::complex(real.powf(exp), 0.0));
            }
            return Ok(PyObject::complex(f64::NAN, f64::NAN));
        }
        if let Some(base) = object_to_bigint(&args[1]) {
            let bf = base.to_f64().unwrap_or_else(|| {
                if base.sign() == Sign::Minus {
                    f64::NEG_INFINITY
                } else {
                    f64::INFINITY
                }
            });
            if bf < 0.0 && ed != BigInt::one() {
                let mag = (-bf).powf(exp);
                return Ok(PyObject::complex(
                    mag * (std::f64::consts::PI * exp).cos(),
                    mag * (std::f64::consts::PI * exp).sin(),
                ));
            }
            return Ok(PyObject::float(bf.powf(exp)));
        }
        unsupported_fraction_operand(args)
    }

    fn frac_mod(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error("Fraction.__mod__ requires 2 args"));
        }
        if let Some(f) = fraction_other_float(&args[1]) {
            return python_float_mod(fraction_to_f64(&args[0])?, f);
        }
        let (an, ad) = get_frac_bigint_parts(&args[0]).unwrap_or((BigInt::zero(), BigInt::one()));
        let Some((bn, bd)) = get_frac_bigint_parts(&args[1]) else {
            return unsupported_fraction_operand(args);
        };
        bigint_mod_fraction(an, ad, bn, bd)
    }

    fn frac_rmod(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error("requires 2 args"));
        }
        if let Some(f) = fraction_other_float(&args[1]) {
            return python_float_mod(f, fraction_to_f64(&args[0])?);
        }
        let (bn, bd) = get_frac_bigint_parts(&args[0]).unwrap_or((BigInt::zero(), BigInt::one()));
        let Some((an, ad)) = get_frac_bigint_parts(&args[1]) else {
            return unsupported_fraction_operand(args);
        };
        bigint_mod_fraction(an, ad, bn, bd)
    }

    fn frac_divmod(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error("requires 2 args"));
        }
        let q = frac_floordiv(args)?;
        let r = frac_mod(args)?;
        Ok(PyObject::tuple(vec![q, r]))
    }

    fn frac_rdivmod(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error("requires 2 args"));
        }
        let q = frac_rfloordiv(args)?;
        let r = frac_rmod(args)?;
        Ok(PyObject::tuple(vec![q, r]))
    }

    fn frac_rtruediv(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error("requires 2 args"));
        }
        let (an, ad) = get_frac_bigint_parts(&args[0]).unwrap_or((BigInt::zero(), BigInt::one()));
        if an.is_zero() {
            return Err(PyException::zero_division_error(
                "Fraction division by zero",
            ));
        }
        if let Some(f) = fraction_other_float(&args[1]) {
            return Ok(PyObject::float(f / fraction_to_f64(&args[0])?));
        }
        if let Some((real, imag)) = fraction_complex_value(&args[1]) {
            return complex_div(real, imag, fraction_to_f64(&args[0])?, 0.0);
        }
        let Some((bn, bd)) = get_frac_bigint_parts(&args[1]) else {
            return unsupported_fraction_operand(args);
        };
        Ok(make_frac_bigint_instance(bn * ad, bd * an))
    }

    fn frac_rfloordiv(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error("requires 2 args"));
        }
        let (an, ad) = get_frac_bigint_parts(&args[0]).unwrap_or((BigInt::zero(), BigInt::one()));
        if an.is_zero() {
            return Err(PyException::zero_division_error(
                "Fraction division by zero",
            ));
        }
        if let Some(f) = fraction_other_float(&args[1]) {
            return Ok(PyObject::float((f / fraction_to_f64(&args[0])?).floor()));
        }
        let Some((bn, bd)) = get_frac_bigint_parts(&args[1]) else {
            return unsupported_fraction_operand(args);
        };
        Ok(bigint_to_object(bigint_floor_div(bn * ad, bd * an)))
    }

    // Fraction as a module-like callable with class methods
    let fraction_from_float = make_builtin(|args| {
        if args.is_empty() {
            return Err(PyException::type_error("from_float requires 1 argument"));
        }
        if let Some(n) = object_to_bigint(&args[0]) {
            return Ok(make_frac_bigint_instance(n, BigInt::one()));
        }
        let f = args[0].to_float()?;
        if f.is_nan() {
            return Err(PyException::value_error(
                "cannot convert NaN to integer ratio",
            ));
        }
        if f.is_infinite() {
            return Err(PyException::overflow_error(
                "cannot convert Infinity to integer ratio",
            ));
        }
        let (n, d) = float_as_integer_ratio(f);
        Ok(make_frac_bigint_instance(n, d))
    });
    let fraction_from_decimal = make_builtin(|args| {
        if args.is_empty() {
            return Err(PyException::type_error("from_decimal requires 1 argument"));
        }
        if let Some(n) = object_to_bigint(&args[0]) {
            return Ok(make_frac_bigint_instance(n, BigInt::one()));
        }
        if let PyObjectPayload::Instance(inst) = &args[0].payload {
            let attrs = inst.attrs.read();
            if attrs.contains_key("__decimal__") {
                let s = attrs.get("_value").and_then(|v| v.as_str()).unwrap_or("0");
                let check = s.trim().trim_start_matches('+').trim_start_matches('-');
                if check.eq_ignore_ascii_case("nan") || check.eq_ignore_ascii_case("snan") {
                    return Err(PyException::value_error(
                        "cannot convert NaN to integer ratio",
                    ));
                }
                if check.eq_ignore_ascii_case("inf") || check.eq_ignore_ascii_case("infinity") {
                    return Err(PyException::overflow_error(
                        "cannot convert Infinity to integer ratio",
                    ));
                }
                if let Some((n, d)) = decimal_string_ratio(s) {
                    return Ok(make_frac_bigint_instance(n, d));
                }
            }
        }
        if let Some((n, d)) = decimal_as_integer_ratio(&args[0])? {
            return Ok(make_frac_bigint_instance(n, d));
        }
        Err(PyException::type_error(
            "from_decimal requires a Decimal instance",
        ))
    });

    let mut frac_class_ns = IndexMap::new();
    if let PyObjectPayload::Instance(inst) = &make_frac_instance(0, 1).payload {
        if let PyObjectPayload::Class(cd) = &inst.class.payload {
            for (key, value) in cd.namespace.read().iter() {
                frac_class_ns.insert(key.clone(), value.clone());
            }
        }
    }
    frac_class_ns.insert(CompactString::from("from_float"), fraction_from_float);
    frac_class_ns.insert(CompactString::from("from_decimal"), fraction_from_decimal);
    frac_class_ns.insert(
        CompactString::from("gcd"),
        make_builtin(|args| {
            check_args("gcd", args, 2)?;
            crate::introspection_modules::emit_deprecation_warning("fractions.gcd() is deprecated");
            let fraction_parts = |obj: &PyObjectRef| -> Option<(BigInt, BigInt)> {
                if let PyObjectPayload::Instance(inst) = &obj.payload {
                    let attrs = inst.attrs.read();
                    if attrs.contains_key("__fraction__") {
                        let n = attrs.get("numerator").and_then(object_to_bigint)?;
                        let d = attrs.get("denominator").and_then(object_to_bigint)?;
                        return Some((n, d));
                    }
                }
                None
            };
            if let (Some((an, ad)), Some((bn, bd))) =
                (fraction_parts(&args[0]), fraction_parts(&args[1]))
            {
                let numerator_gcd = an.abs().gcd(&bn.abs());
                let denominator_lcm = ad.abs().lcm(&bd.abs());
                let mut result_n = numerator_gcd;
                if bn.sign() == Sign::Minus || (bn.is_zero() && an.sign() == Sign::Minus) {
                    result_n = -result_n;
                }
                return Ok(make_frac_bigint_instance(result_n, denominator_lcm));
            }
            if matches!(&args[0].payload, PyObjectPayload::Float(_))
                || matches!(&args[1].payload, PyObjectPayload::Float(_))
            {
                let original_a = args[0].to_float()?;
                let original_b = args[1].to_float()?;
                let mut a = original_a.abs();
                let mut b = original_b.abs();
                while b != 0.0 {
                    let t = b;
                    b = a % b;
                    a = t;
                }
                if original_b < 0.0 || (original_b == 0.0 && original_a < 0.0) {
                    a = -a;
                }
                return Ok(PyObject::float(a));
            }
            let original_a = object_to_bigint(&args[0])
                .ok_or_else(|| PyException::type_error("gcd() arguments must be numbers"))?;
            let original_b = object_to_bigint(&args[1])
                .ok_or_else(|| PyException::type_error("gcd() arguments must be numbers"))?;
            let mut a = original_a.abs();
            let mut b = original_b.abs();
            while !b.is_zero() {
                let t = b.clone();
                b = a % b;
                a = t;
            }
            if original_b.sign() == Sign::Minus
                || (original_b.is_zero() && original_a.sign() == Sign::Minus)
            {
                a = -a;
            }
            Ok(bigint_to_object(a))
        }),
    );
    let frac_class = PyObject::class(CompactString::from("Fraction"), vec![], frac_class_ns);

    // Store new function on the class for instantiation
    if let PyObjectPayload::Class(ref cd) = frac_class.payload {
        cd.namespace.write().insert(
            CompactString::from("__abc_registered_name__"),
            PyObject::str_val(CompactString::from("Rational")),
        );
        cd.namespace.write().insert(
            CompactString::from("__new__"),
            make_builtin(|args| {
                if args.is_empty() {
                    return Ok(make_frac_instance(0, 1));
                }
                // Skip cls argument if present (class object)
                let real_args =
                    if !args.is_empty() && matches!(&args[0].payload, PyObjectPayload::Class(_)) {
                        &args[1..]
                    } else {
                        args
                    };
                if real_args.is_empty() {
                    if !args.is_empty() && matches!(&args[0].payload, PyObjectPayload::Class(_)) {
                        return Ok(make_frac_bigint_instance_for_class(
                            args[0].clone(),
                            BigInt::zero(),
                            BigInt::one(),
                        ));
                    }
                    return Ok(make_frac_instance(0, 1));
                }
                if real_args.len() == 1 {
                    match &real_args[0].payload {
                        PyObjectPayload::Int(n) => {
                            let n = match n {
                                PyInt::Small(v) => BigInt::from(*v),
                                PyInt::Big(v) => v.as_ref().clone(),
                            };
                            if !args.is_empty()
                                && matches!(&args[0].payload, PyObjectPayload::Class(_))
                            {
                                return Ok(make_frac_bigint_instance_for_class(
                                    args[0].clone(),
                                    n,
                                    BigInt::one(),
                                ));
                            }
                            return Ok(make_frac_bigint_instance(n, BigInt::one()));
                        }
                        PyObjectPayload::Float(f) => {
                            if f.is_nan() {
                                return Err(PyException::value_error(
                                    "cannot convert NaN to integer ratio",
                                ));
                            }
                            if f.is_infinite() {
                                return Err(PyException::overflow_error(
                                    "cannot convert Infinity to integer ratio",
                                ));
                            }
                            let (n, d) = float_as_integer_ratio(*f);
                            if !args.is_empty()
                                && matches!(&args[0].payload, PyObjectPayload::Class(_))
                            {
                                return Ok(make_frac_bigint_instance_for_class(
                                    args[0].clone(),
                                    n,
                                    d,
                                ));
                            }
                            return Ok(make_frac_bigint_instance(n, d));
                        }
                        PyObjectPayload::Str(s) => {
                            let stripped = s.trim();
                            if let Some((n_str, d_str)) = stripped.split_once('/') {
                                let n_text = n_str.trim();
                                let d_text = d_str.trim();
                                if n_text.is_empty()
                                    || d_text.is_empty()
                                    || n_str != n_text
                                    || d_str != d_text
                                    || n_text.contains(' ')
                                    || d_text.contains(' ')
                                    || d_text.starts_with('+')
                                    || n_text.contains('.')
                                    || d_text.contains('.')
                                {
                                    return Err(PyException::value_error(format!(
                                        "Invalid literal for Fraction: '{}'",
                                        stripped
                                    )));
                                }
                                let n: BigInt = n_text.parse().map_err(|_| {
                                    PyException::value_error(format!(
                                        "Invalid literal for Fraction: '{}'",
                                        stripped
                                    ))
                                })?;
                                let d: BigInt = d_text.parse().map_err(|_| {
                                    PyException::value_error(format!(
                                        "Invalid literal for Fraction: '{}'",
                                        stripped
                                    ))
                                })?;
                                if d.is_zero() {
                                    return Err(fraction_zero_division(&n));
                                }
                                if !args.is_empty()
                                    && matches!(&args[0].payload, PyObjectPayload::Class(_))
                                {
                                    return Ok(make_frac_bigint_instance_for_class(
                                        args[0].clone(),
                                        n,
                                        d,
                                    ));
                                }
                                return Ok(make_frac_bigint_instance(n, d));
                            } else if stripped.contains('.')
                                || stripped.contains('e')
                                || stripped.contains('E')
                            {
                                return fraction_from_decimal_string_for_class(
                                    if !args.is_empty()
                                        && matches!(&args[0].payload, PyObjectPayload::Class(_))
                                    {
                                        Some(&args[0])
                                    } else {
                                        None
                                    },
                                    stripped,
                                );
                            } else {
                                let n: BigInt = stripped.parse().map_err(|_| {
                                    PyException::value_error(format!(
                                        "Invalid literal for Fraction: '{}'",
                                        stripped
                                    ))
                                })?;
                                if !args.is_empty()
                                    && matches!(&args[0].payload, PyObjectPayload::Class(_))
                                {
                                    return Ok(make_frac_bigint_instance_for_class(
                                        args[0].clone(),
                                        n,
                                        BigInt::one(),
                                    ));
                                }
                                return Ok(make_frac_bigint_instance(n, BigInt::one()));
                            }
                        }
                        _ => {
                            if let Some((numerator, denominator)) =
                                get_fraction_constructor_parts(&real_args[0])
                            {
                                if !object_to_bigint(&numerator)
                                    .is_some_and(|n| n == BigInt::zero())
                                    || !matches!(&numerator.payload, PyObjectPayload::Int(_))
                                {
                                    return Ok(make_frac_raw_instance(numerator, denominator));
                                }
                            }
                            if let Some((n, d)) = get_frac_bigint_parts(&real_args[0]) {
                                if !args.is_empty()
                                    && matches!(&args[0].payload, PyObjectPayload::Class(_))
                                {
                                    return Ok(make_frac_bigint_instance_for_class(
                                        args[0].clone(),
                                        n,
                                        d,
                                    ));
                                }
                                return Ok(make_frac_bigint_instance(n, d));
                            }
                            if let Some((numerator, denominator)) =
                                get_fraction_constructor_parts(&real_args[0])
                            {
                                return Ok(make_frac_raw_instance(numerator, denominator));
                            }
                            // Handle Decimal instances by converting via string
                            if let PyObjectPayload::Instance(inst) = &real_args[0].payload {
                                let attrs = inst.attrs.read();
                                if attrs.contains_key("__decimal__") {
                                    let s = attrs
                                        .get("_value")
                                        .map(|v| v.py_to_string())
                                        .unwrap_or_else(|| "0".to_string());
                                    drop(attrs);
                                    let check =
                                        s.trim().trim_start_matches('+').trim_start_matches('-');
                                    if check.eq_ignore_ascii_case("nan")
                                        || check.eq_ignore_ascii_case("snan")
                                    {
                                        return Err(PyException::value_error(
                                            "cannot convert NaN to integer ratio",
                                        ));
                                    }
                                    if check.eq_ignore_ascii_case("inf")
                                        || check.eq_ignore_ascii_case("infinity")
                                    {
                                        return Err(PyException::overflow_error(
                                            "cannot convert Infinity to integer ratio",
                                        ));
                                    }
                                    return decimal_str_to_fraction(&s);
                                }
                            }
                            if let Some((n, d)) = decimal_as_integer_ratio(&real_args[0])? {
                                if !args.is_empty()
                                    && matches!(&args[0].payload, PyObjectPayload::Class(_))
                                {
                                    return Ok(make_frac_bigint_instance_for_class(
                                        args[0].clone(),
                                        n,
                                        d,
                                    ));
                                }
                                return Ok(make_frac_bigint_instance(n, d));
                            }
                            return Err(PyException::type_error(
                                "Fraction() argument must be int, float, str, or Decimal",
                            ));
                        }
                    }
                }
                if real_args.len() > 2 {
                    return Err(PyException::type_error(
                        "Fraction() takes at most 2 arguments",
                    ));
                }
                let (n_num, n_den) = if let Some(parts) = get_frac_bigint_parts(&real_args[0]) {
                    parts
                } else {
                    (index_bigint(&real_args[0], "Fraction")?, BigInt::one())
                };
                let (d_num, d_den) = if let Some(parts) = get_frac_bigint_parts(&real_args[1]) {
                    parts
                } else {
                    (index_bigint(&real_args[1], "Fraction")?, BigInt::one())
                };
                let n = n_num * d_den;
                let d = n_den * d_num;
                if d.is_zero() {
                    return Err(fraction_zero_division(&n));
                }
                Ok(make_frac_bigint_instance_for_class(args[0].clone(), n, d))
            }),
        );
        cd.method_vtable.write().insert(
            CompactString::from("__new__"),
            cd.namespace.read()["__new__"].clone(),
        );
        cd.invalidate_cache();
    }

    make_module(
        "fractions",
        vec![
            (
                "gcd",
                frac_class
                    .get_attr("gcd")
                    .unwrap_or_else(|| make_builtin(fraction_gcd)),
            ),
            ("Fraction", frac_class),
        ],
    )
}

fn fraction_gcd(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("gcd", args, 2)?;
    crate::introspection_modules::emit_deprecation_warning("fractions.gcd() is deprecated");
    if matches!(&args[0].payload, PyObjectPayload::Float(_))
        || matches!(&args[1].payload, PyObjectPayload::Float(_))
    {
        let original_a = args[0].to_float()?;
        let original_b = args[1].to_float()?;
        let mut a = original_a.abs();
        let mut b = original_b.abs();
        while b != 0.0 {
            let t = b;
            b = a % b;
            a = t;
        }
        if original_b < 0.0 || (original_b == 0.0 && original_a < 0.0) {
            a = -a;
        }
        return Ok(PyObject::float(a));
    }
    let original_a = match &args[0].payload {
        PyObjectPayload::Int(PyInt::Small(i)) => BigInt::from(*i),
        PyObjectPayload::Int(PyInt::Big(i)) => i.as_ref().clone(),
        PyObjectPayload::Bool(b) => BigInt::from(if *b { 1 } else { 0 }),
        _ => BigInt::from(args[0].to_int()?),
    };
    let original_b = match &args[1].payload {
        PyObjectPayload::Int(PyInt::Small(i)) => BigInt::from(*i),
        PyObjectPayload::Int(PyInt::Big(i)) => i.as_ref().clone(),
        PyObjectPayload::Bool(b) => BigInt::from(if *b { 1 } else { 0 }),
        _ => BigInt::from(args[1].to_int()?),
    };
    let mut a = original_a.abs();
    let mut b = original_b.abs();
    while !b.is_zero() {
        let t = b.clone();
        b = a % b;
        a = t;
    }
    if original_b.sign() == Sign::Minus
        || (original_b.is_zero() && original_a.sign() == Sign::Minus)
    {
        a = -a;
    }
    Ok(bigint_to_object(a))
}
