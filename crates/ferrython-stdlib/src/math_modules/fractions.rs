use super::{bigint_to_object, index_bigint};
use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    check_args, make_builtin, make_module, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::PyInt;
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
        }
        object_to_bigint(obj).map(|n| (n, BigInt::one()))
    }

    fn get_frac_parts(obj: &PyObjectRef) -> Option<(i64, i64)> {
        let (n, d) = get_frac_bigint_parts(obj)?;
        Some((n.to_i64()?, d.to_i64()?))
    }

    fn float_to_bigint_fraction(f: f64) -> Option<(BigInt, BigInt)> {
        if !f.is_finite() {
            return None;
        }
        if f == 0.0 {
            return Some((BigInt::zero(), BigInt::one()));
        }
        let bits = f.to_bits();
        let negative = (bits >> 63) != 0;
        let raw_exp = ((bits >> 52) & 0x7ff) as i32;
        let frac = bits & 0x000f_ffff_ffff_ffff;
        let (mantissa, exp) = if raw_exp == 0 {
            (frac, -1074)
        } else {
            ((1u64 << 52) | frac, raw_exp - 1075)
        };
        let mut numer = BigInt::from(mantissa);
        let mut denom = BigInt::one();
        if exp >= 0 {
            numer <<= exp as usize;
        } else {
            denom <<= (-exp) as usize;
        }
        if negative {
            numer = -numer;
        }
        let g = numer.abs().gcd(&denom);
        Some((numer / &g, denom / g))
    }

    fn get_frac_cmp_parts(obj: &PyObjectRef) -> Option<(BigInt, BigInt)> {
        if let Some(parts) = get_frac_bigint_parts(obj) {
            return Some(parts);
        }
        if let PyObjectPayload::Float(f) = &obj.payload {
            return float_to_bigint_fraction(*f);
        }
        None
    }

    fn decimal_str_to_fraction(s: &str) -> PyResult<PyObjectRef> {
        let s = s.trim();
        let (sign, s) = if let Some(rest) = s.strip_prefix('-') {
            (-1i64, rest)
        } else {
            (1i64, s)
        };
        if let Some((int_part, frac_part)) = s.split_once('.') {
            let int_part = if int_part.is_empty() { "0" } else { int_part };
            let frac_digits = frac_part.len() as u32;
            let denom = 10i64.checked_pow(frac_digits).unwrap_or(1);
            let int_val: i64 = int_part.parse().unwrap_or(0);
            let frac_val: i64 = frac_part.parse().unwrap_or(0);
            let numer = sign * (int_val * denom + frac_val);
            Ok(make_frac_instance(numer, denom))
        } else {
            let n: i64 = s.parse().unwrap_or(0);
            Ok(make_frac_instance(sign * n, 1))
        }
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

    fn make_frac_instance(num: i64, den: i64) -> PyObjectRef {
        make_frac_bigint_instance(BigInt::from(num), BigInt::from(den))
    }

    fn make_frac_normalized_instance(num: BigInt, den: BigInt) -> PyObjectRef {
        let num_obj = bigint_to_object(num);
        let den_obj = bigint_to_object(den);
        let mut frac_ns = IndexMap::new();
        frac_ns.insert(CompactString::from("__add__"), make_builtin(frac_add));
        frac_ns.insert(CompactString::from("__radd__"), make_builtin(frac_add));
        frac_ns.insert(CompactString::from("__sub__"), make_builtin(frac_sub));
        frac_ns.insert(CompactString::from("__rsub__"), make_builtin(frac_rsub));
        frac_ns.insert(CompactString::from("__mul__"), make_builtin(frac_mul));
        frac_ns.insert(CompactString::from("__rmul__"), make_builtin(frac_mul));
        frac_ns.insert(CompactString::from("__truediv__"), make_builtin(frac_div));
        frac_ns.insert(
            CompactString::from("__floordiv__"),
            make_builtin(frac_floordiv),
        );
        frac_ns.insert(CompactString::from("__neg__"), make_builtin(frac_neg));
        frac_ns.insert(CompactString::from("__abs__"), make_builtin(frac_abs));
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
                let (n, d) = get_frac_parts(&args[0]).unwrap_or((0, 1));
                let spec = args.get(1).map(|a| a.py_to_string()).unwrap_or_default();
                if spec.is_empty() || spec == "s" {
                    if d == 1 {
                        return Ok(PyObject::str_val(CompactString::from(format!("{}", n))));
                    }
                    return Ok(PyObject::str_val(CompactString::from(format!(
                        "{}/{}",
                        n, d
                    ))));
                }
                // For numeric format specs, convert to float
                let f = n as f64 / d as f64;
                Ok(PyObject::str_val(CompactString::from(format!("{}", f))))
            }),
        );
        frac_ns.insert(
            CompactString::from("as_integer_ratio"),
            make_builtin(|args: &[PyObjectRef]| {
                if args.is_empty() {
                    return Ok(PyObject::tuple(vec![PyObject::int(0), PyObject::int(1)]));
                }
                let (n, d) = get_frac_parts(&args[0]).unwrap_or((0, 1));
                Ok(PyObject::tuple(vec![PyObject::int(n), PyObject::int(d)]))
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
        }
        inst
    }

    fn frac_add(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error("Fraction.__add__ requires 2 args"));
        }
        let (an, ad) = get_frac_bigint_parts(&args[0])
            .ok_or_else(|| PyException::type_error("not a Fraction"))?;
        let (bn, bd) = get_frac_bigint_parts(&args[1])
            .ok_or_else(|| PyException::type_error("not a Fraction"))?;
        Ok(make_frac_bigint_instance(&an * &bd + &bn * &ad, ad * bd))
    }

    fn frac_sub(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error("Fraction.__sub__ requires 2 args"));
        }
        let (an, ad) = get_frac_bigint_parts(&args[0])
            .ok_or_else(|| PyException::type_error("not a Fraction"))?;
        let (bn, bd) = get_frac_bigint_parts(&args[1])
            .ok_or_else(|| PyException::type_error("not a Fraction"))?;
        Ok(make_frac_bigint_instance(&an * &bd - &bn * &ad, ad * bd))
    }

    fn frac_rsub(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error("requires 2 args"));
        }
        let (an, ad) = get_frac_bigint_parts(&args[0])
            .ok_or_else(|| PyException::type_error("not a Fraction"))?;
        let (bn, bd) = get_frac_bigint_parts(&args[1])
            .ok_or_else(|| PyException::type_error("not a Fraction"))?;
        Ok(make_frac_bigint_instance(&bn * &ad - &an * &bd, ad * bd))
    }

    fn frac_mul(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error("Fraction.__mul__ requires 2 args"));
        }
        let (an, ad) = get_frac_bigint_parts(&args[0])
            .ok_or_else(|| PyException::type_error("not a Fraction"))?;
        let (bn, bd) = get_frac_bigint_parts(&args[1])
            .ok_or_else(|| PyException::type_error("not a Fraction"))?;
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
        let (bn, bd) = get_frac_bigint_parts(&args[1])
            .ok_or_else(|| PyException::type_error("not a Fraction"))?;
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
        let (an, ad) =
            get_frac_parts(&args[0]).ok_or_else(|| PyException::type_error("not a Fraction"))?;
        let (bn, bd) =
            get_frac_parts(&args[1]).ok_or_else(|| PyException::type_error("not a Fraction"))?;
        if bn == 0 {
            return Err(PyException::zero_division_error(
                "Fraction division by zero",
            ));
        }
        let result = (an * bd).div_euclid(ad * bn);
        Ok(PyObject::int(result))
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

    fn frac_eq(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Ok(PyObject::bool_val(false));
        }
        let a = get_frac_cmp_parts(&args[0]);
        let b = get_frac_cmp_parts(&args[1]);
        match (a, b) {
            (Some((an, ad)), Some((bn, bd))) => Ok(PyObject::bool_val(an * bd == bn * ad)),
            _ => Ok(PyObject::bool_val(false)),
        }
    }

    fn frac_lt(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Ok(PyObject::bool_val(false));
        }
        let (an, ad) = get_frac_cmp_parts(&args[0]).unwrap_or((BigInt::zero(), BigInt::one()));
        let (bn, bd) = get_frac_cmp_parts(&args[1]).unwrap_or((BigInt::zero(), BigInt::one()));
        Ok(PyObject::bool_val(an * bd < bn * ad))
    }

    fn frac_le(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Ok(PyObject::bool_val(false));
        }
        let (an, ad) = get_frac_cmp_parts(&args[0]).unwrap_or((BigInt::zero(), BigInt::one()));
        let (bn, bd) = get_frac_cmp_parts(&args[1]).unwrap_or((BigInt::zero(), BigInt::one()));
        Ok(PyObject::bool_val(an * bd <= bn * ad))
    }

    fn frac_gt(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Ok(PyObject::bool_val(false));
        }
        let (an, ad) = get_frac_cmp_parts(&args[0]).unwrap_or((BigInt::zero(), BigInt::one()));
        let (bn, bd) = get_frac_cmp_parts(&args[1]).unwrap_or((BigInt::zero(), BigInt::one()));
        Ok(PyObject::bool_val(an * bd > bn * ad))
    }

    fn frac_ge(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Ok(PyObject::bool_val(false));
        }
        let (an, ad) = get_frac_cmp_parts(&args[0]).unwrap_or((BigInt::zero(), BigInt::one()));
        let (bn, bd) = get_frac_cmp_parts(&args[1]).unwrap_or((BigInt::zero(), BigInt::one()));
        Ok(PyObject::bool_val(an * bd >= bn * ad))
    }

    fn frac_hash(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let (n, d) = get_frac_parts(&args[0]).unwrap_or((0, 1));
        Ok(PyObject::int(n.wrapping_mul(31).wrapping_add(d)))
    }

    fn frac_str(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let (n, d) = get_frac_parts(&args[0]).unwrap_or((0, 1));
        let s = if d == 1 {
            format!("{}", n)
        } else {
            format!("{}/{}", n, d)
        };
        Ok(PyObject::str_val(CompactString::from(s)))
    }

    fn frac_repr(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let (n, d) = get_frac_parts(&args[0]).unwrap_or((0, 1));
        Ok(PyObject::str_val(CompactString::from(format!(
            "Fraction({}, {})",
            n, d
        ))))
    }

    fn frac_float(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let (n, d) = get_frac_bigint_parts(&args[0]).unwrap_or((BigInt::zero(), BigInt::one()));
        let n = n
            .to_f64()
            .ok_or_else(|| PyException::overflow_error("int too large to convert to float"))?;
        let d = d
            .to_f64()
            .ok_or_else(|| PyException::overflow_error("int too large to convert to float"))?;
        Ok(PyObject::float(n / d))
    }

    fn frac_int(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let (n, d) = get_frac_parts(&args[0]).unwrap_or((0, 1));
        Ok(PyObject::int(n / d))
    }

    fn frac_bool(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let (n, _) = get_frac_parts(&args[0]).unwrap_or((0, 1));
        Ok(PyObject::bool_val(n != 0))
    }

    fn frac_limit_denominator(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let (n, d) = get_frac_parts(&args[0]).unwrap_or((0, 1));
        let max_den = if args.len() > 1 {
            args[1].to_int().unwrap_or(1_000_000)
        } else {
            1_000_000
        };
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
        let (n, d) = get_frac_parts(&args[0]).unwrap_or((0, 1));
        let exp = args[1].to_int().unwrap_or(1);
        if exp >= 0 {
            let e = exp as u32;
            Ok(make_frac_instance(n.pow(e), d.pow(e)))
        } else {
            let e = (-exp) as u32;
            if n == 0 {
                return Err(PyException::zero_division_error(
                    "Fraction division by zero",
                ));
            }
            Ok(make_frac_instance(d.pow(e), n.pow(e)))
        }
    }

    fn frac_mod(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error("Fraction.__mod__ requires 2 args"));
        }
        let (an, ad) = get_frac_parts(&args[0]).unwrap_or((0, 1));
        let (bn, bd) = get_frac_parts(&args[1]).unwrap_or((1, 1));
        if bn == 0 {
            return Err(PyException::zero_division_error("Fraction modulo by zero"));
        }
        // a % b = a - b * floor(a/b)
        let num = an * bd;
        let den = ad * bn;
        let floor_div = if den > 0 {
            num.div_euclid(den)
        } else {
            -((-num).div_euclid(-den))
        };
        let result_n = an * bd * bd - floor_div * bn * ad * bd;
        let result_d = ad * bd * bd;
        Ok(make_frac_instance(result_n, result_d))
    }

    fn frac_rtruediv(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error("requires 2 args"));
        }
        let (an, ad) = get_frac_parts(&args[0]).unwrap_or((0, 1));
        let (bn, bd) = get_frac_parts(&args[1]).unwrap_or((1, 1));
        if an == 0 {
            return Err(PyException::zero_division_error(
                "Fraction division by zero",
            ));
        }
        Ok(make_frac_instance(bn * ad, bd * an))
    }

    fn frac_rfloordiv(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error("requires 2 args"));
        }
        let (an, ad) = get_frac_parts(&args[0]).unwrap_or((0, 1));
        let (bn, bd) = get_frac_parts(&args[1]).unwrap_or((1, 1));
        if an == 0 {
            return Err(PyException::zero_division_error(
                "Fraction division by zero",
            ));
        }
        let num = bn * ad;
        let den = bd * an;
        let result = if (num < 0) ^ (den < 0) {
            -((-num).abs() / den.abs()) - if num.abs() % den.abs() != 0 { 1 } else { 0 }
        } else {
            num / den
        };
        Ok(make_frac_instance(result, 1))
    }

    // Fraction as a module-like callable with class methods
    let fraction_from_float = make_builtin(|args| {
        if args.is_empty() {
            return Err(PyException::type_error("from_float requires 1 argument"));
        }
        let f = args[0].to_float()?;
        let (n, d) = float_to_fraction(f);
        Ok(make_frac_instance(n, d))
    });
    let fraction_from_decimal = make_builtin(|args| {
        if args.is_empty() {
            return Err(PyException::type_error("from_decimal requires 1 argument"));
        }
        let f = args[0].to_float()?;
        let (n, d) = float_to_fraction(f);
        Ok(make_frac_instance(n, d))
    });

    let frac_class_ns = IndexMap::from([
        (CompactString::from("from_float"), fraction_from_float),
        (CompactString::from("from_decimal"), fraction_from_decimal),
    ]);
    let frac_class = PyObject::class(CompactString::from("Fraction"), vec![], frac_class_ns);

    // Store new function on the class for instantiation
    if let PyObjectPayload::Class(ref cd) = frac_class.payload {
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
                    return Ok(make_frac_instance(0, 1));
                }
                if real_args.len() == 1 {
                    match &real_args[0].payload {
                        PyObjectPayload::Int(n) => {
                            let n = match n {
                                PyInt::Small(v) => BigInt::from(*v),
                                PyInt::Big(v) => v.as_ref().clone(),
                            };
                            return Ok(make_frac_bigint_instance(n, BigInt::one()));
                        }
                        PyObjectPayload::Float(f) => {
                            let (n, d) = float_to_fraction(*f);
                            return Ok(make_frac_instance(n, d));
                        }
                        PyObjectPayload::Str(s) => {
                            if let Some((n_str, d_str)) = s.split_once('/') {
                                let n: i64 = n_str.trim().parse().map_err(|_| {
                                    PyException::value_error("Invalid fraction string")
                                })?;
                                let d: i64 = d_str.trim().parse().map_err(|_| {
                                    PyException::value_error("Invalid fraction string")
                                })?;
                                if d == 0 {
                                    return Err(PyException::new(
                                        ferrython_core::error::ExceptionKind::ZeroDivisionError,
                                        "Fraction(_, 0)",
                                    ));
                                }
                                return Ok(make_frac_instance(n, d));
                            } else if s.contains('.') || s.contains('e') || s.contains('E') {
                                return decimal_str_to_fraction(s.trim());
                            } else {
                                let n: i64 = s.trim().parse().map_err(|_| {
                                    PyException::value_error("Invalid fraction string")
                                })?;
                                return Ok(make_frac_instance(n, 1));
                            }
                        }
                        _ => {
                            if let Some((n, d)) = get_frac_parts(&real_args[0]) {
                                return Ok(make_frac_instance(n, d));
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
                                    return decimal_str_to_fraction(&s);
                                }
                            }
                            return Err(PyException::type_error(
                                "Fraction() argument must be int, float, str, or Decimal",
                            ));
                        }
                    }
                }
                let n = index_bigint(&real_args[0], "Fraction")?;
                let d = index_bigint(&real_args[1], "Fraction")?;
                if d.is_zero() {
                    return Err(PyException::new(
                        ferrython_core::error::ExceptionKind::ZeroDivisionError,
                        "Fraction(_, 0)",
                    ));
                }
                Ok(make_frac_bigint_instance(n, d))
            }),
        );
        cd.invalidate_cache();
    }

    make_module(
        "fractions",
        vec![
            ("Fraction", frac_class),
            ("gcd", make_builtin(fraction_gcd)),
        ],
    )
}

fn float_to_fraction(f: f64) -> (i64, i64) {
    if f == 0.0 {
        return (0, 1);
    }
    if f.is_infinite() || f.is_nan() {
        return (0, 1);
    }
    // Exact IEEE 754 decomposition matching CPython's float.as_integer_ratio()
    let bits = f.to_bits();
    let sign: i64 = if (bits >> 63) != 0 { -1 } else { 1 };
    let raw_exp = ((bits >> 52) & 0x7FF) as i64;
    let mantissa = (bits & 0x000F_FFFF_FFFF_FFFF) as i64;

    let (mut numer, exp) = if raw_exp == 0 {
        (mantissa, -1074i64) // subnormal
    } else {
        ((1i64 << 52) | mantissa, raw_exp - 1075)
    };

    // Remove trailing zero bits to simplify
    if numer != 0 {
        let tz = numer.trailing_zeros();
        numer >>= tz;
        // exp += tz as i64; (already accounted for since we shift numer)
        let adjusted_exp = exp + tz as i64;
        if adjusted_exp >= 0 {
            let shift = adjusted_exp.min(62) as u32;
            return (sign * numer.checked_shl(shift).unwrap_or(numer), 1);
        } else {
            let shift = (-adjusted_exp).min(62) as u32;
            return (sign * numer, 1i64.checked_shl(shift).unwrap_or(i64::MAX));
        }
    }
    (0, 1)
}

fn fraction_gcd(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("gcd", args, 2)?;
    let mut a = args[0].to_int()?.abs();
    let mut b = args[1].to_int()?.abs();
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    Ok(PyObject::int(a))
}
