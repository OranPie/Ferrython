use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};
use ferrython_core::types::PyInt;
use num_bigint::{BigInt, Sign};
use num_traits::{FromPrimitive, One, ToPrimitive, Zero};

fn bigint_to_scaled_f64(n: &BigInt, d: &BigInt) -> PyResult<f64> {
    if d.is_zero() {
        return Err(PyException::zero_division_error("division by zero"));
    }
    if let (Some(nf), Some(df)) = (n.to_f64(), d.to_f64()) {
        if nf.is_finite() && df.is_finite() {
            return Ok(nf / df);
        }
    }
    let n_bits = n.bits() as i64;
    let d_bits = d.bits() as i64;
    let n_shift = (n_bits - 1020).max(0) as usize;
    let d_shift = (d_bits - 1020).max(0) as usize;
    let ns = if n_shift > 0 { n >> n_shift } else { n.clone() };
    let ds = if d_shift > 0 { d >> d_shift } else { d.clone() };
    let nf = ns
        .to_f64()
        .ok_or_else(|| PyException::overflow_error("int too large to convert to float"))?;
    let df = ds
        .to_f64()
        .ok_or_else(|| PyException::overflow_error("int too large to convert to float"))?;
    Ok((nf / df) * 2f64.powi((n_shift as i32) - (d_shift as i32)))
}

fn object_to_bigint(obj: &PyObjectRef) -> Option<BigInt> {
    match &obj.payload {
        PyObjectPayload::Int(PyInt::Small(n)) => Some(BigInt::from(*n)),
        PyObjectPayload::Int(PyInt::Big(n)) => Some(n.as_ref().clone()),
        PyObjectPayload::Bool(b) => Some(BigInt::from(if *b { 1 } else { 0 })),
        _ => None,
    }
}

pub(super) fn math_number_to_float(obj: &PyObjectRef) -> PyResult<f64> {
    match &obj.payload {
        PyObjectPayload::Float(f) => Ok(*f),
        PyObjectPayload::Int(PyInt::Small(n)) => Ok(*n as f64),
        PyObjectPayload::Int(PyInt::Big(n)) => {
            let value = n
                .to_f64()
                .ok_or_else(|| PyException::overflow_error("int too large to convert to float"))?;
            if value.is_finite() {
                Ok(value)
            } else {
                Err(PyException::overflow_error(
                    "int too large to convert to float",
                ))
            }
        }
        PyObjectPayload::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
        PyObjectPayload::Str(_) => Err(PyException::type_error("must be real number, not str")),
        PyObjectPayload::Instance(inst) => {
            {
                let attrs = inst.attrs.read();
                if attrs.contains_key("__decimal__") {
                    if let Some(v) = attrs.get("_value") {
                        if let Some(s) = v.as_str() {
                            return s.parse::<f64>().map_err(|_| {
                                PyException::value_error(format!(
                                    "could not convert string to float: '{}'",
                                    s
                                ))
                            });
                        }
                    }
                }
                if let Some(v) = attrs.get("__builtin_value__") {
                    if matches!(
                        &v.payload,
                        PyObjectPayload::Float(_)
                            | PyObjectPayload::Int(_)
                            | PyObjectPayload::Bool(_)
                    ) {
                        return math_number_to_float(v);
                    }
                }
                if let Some(v) = attrs.get("_value") {
                    if let Some(s) = v.as_str() {
                        return s.parse::<f64>().map_err(|_| {
                            PyException::value_error(format!(
                                "could not convert string to float: '{}'",
                                s
                            ))
                        });
                    }
                }
                if attrs.contains_key("__fraction__") {
                    if let (Some(n), Some(d)) = (attrs.get("numerator"), attrs.get("denominator")) {
                        if let (Some(n), Some(d)) = (object_to_bigint(n), object_to_bigint(d)) {
                            return bigint_to_scaled_f64(&n, &d);
                        }
                        return Ok(math_number_to_float(n)? / math_number_to_float(d)?);
                    }
                }
            }
            if let Some(method) = obj.get_attr("__float__") {
                let result = ferrython_core::object::call_callable(&method, &[])?;
                if let PyObjectPayload::Float(f) = &result.payload {
                    return Ok(*f);
                }
                return Err(PyException::type_error("__float__ returned non-float"));
            }
            obj.to_float()
        }
        _ => Err(PyException::type_error(format!(
            "must be real number, not {}",
            obj.type_name()
        ))),
    }
}

pub(super) fn index_bigint(obj: &PyObjectRef, func_name: &str) -> PyResult<BigInt> {
    match &obj.payload {
        PyObjectPayload::Int(n) => Ok(match n {
            PyInt::Small(v) => BigInt::from(*v),
            PyInt::Big(v) => v.as_ref().clone(),
        }),
        PyObjectPayload::Bool(b) => Ok(BigInt::from(if *b { 1 } else { 0 })),
        PyObjectPayload::Instance(inst) => {
            if let Some(value) = inst.attrs.read().get("__builtin_value__").cloned() {
                if matches!(
                    &value.payload,
                    PyObjectPayload::Int(_) | PyObjectPayload::Bool(_)
                ) {
                    return index_bigint(&value, func_name);
                }
            }
            if let Some(method) = obj.get_attr("__index__") {
                let result = ferrython_core::object::call_callable(&method, &[])?;
                return index_bigint(&result, func_name);
            }
            Err(PyException::type_error(format!(
                "{}() argument must be an integer",
                func_name
            )))
        }
        _ => Err(PyException::type_error(format!(
            "{}() argument must be an integer",
            func_name
        ))),
    }
}

pub(super) fn bigint_to_object(value: BigInt) -> PyObjectRef {
    if let Some(v) = value.to_i64() {
        PyObject::int(v)
    } else {
        PyObject::big_int(value)
    }
}

pub(super) fn isqrt_bigint(n: &BigInt) -> BigInt {
    if n.is_zero() {
        return BigInt::zero();
    }
    let mut x = BigInt::one() << ((n.bits() + 1) / 2);
    loop {
        let y = (&x + n / &x) >> 1usize;
        if y >= x {
            return x;
        }
        x = y;
    }
}

pub(super) fn float_to_integral_object(
    x: f64,
    func_name: &str,
    op: fn(f64) -> f64,
) -> PyResult<PyObjectRef> {
    if x.is_nan() {
        return Err(PyException::value_error(format!(
            "cannot convert float NaN to integer in {}",
            func_name
        )));
    }
    if x.is_infinite() {
        return Err(PyException::overflow_error(format!(
            "cannot convert float infinity to integer in {}",
            func_name
        )));
    }
    let value = op(x);
    if value >= i64::MIN as f64 && value <= i64::MAX as f64 {
        Ok(PyObject::int(value as i64))
    } else {
        BigInt::from_f64(value)
            .map(PyObject::big_int)
            .ok_or_else(|| PyException::overflow_error("float too large to convert to integer"))
    }
}

pub(super) fn pyint_ln(n: &PyInt) -> PyResult<f64> {
    match n {
        PyInt::Small(v) => {
            if *v <= 0 {
                Err(PyException::value_error("math domain error"))
            } else {
                Ok((*v as f64).ln())
            }
        }
        PyInt::Big(v) => {
            if v.sign() != Sign::Plus {
                return Err(PyException::value_error("math domain error"));
            }
            let bits = v.bits();
            if bits <= 1023 {
                return v
                    .to_f64()
                    .map(|f| f.ln())
                    .ok_or_else(|| PyException::overflow_error("int too large to convert"));
            }
            let shift = bits.saturating_sub(53) as usize;
            let top = (v.as_ref() >> shift)
                .to_u64()
                .ok_or_else(|| PyException::overflow_error("int too large to convert"))?;
            Ok((top as f64).ln() + (shift as f64) * std::f64::consts::LN_2)
        }
    }
}

pub(super) fn pyint_log2(n: &PyInt) -> PyResult<f64> {
    match n {
        PyInt::Small(v) => {
            if *v <= 0 {
                Err(PyException::value_error("math domain error"))
            } else if (*v & (*v - 1)) == 0 {
                Ok((63 - v.leading_zeros()) as f64)
            } else {
                Ok((*v as f64).log2())
            }
        }
        PyInt::Big(v) => {
            if v.sign() != Sign::Plus {
                return Err(PyException::value_error("math domain error"));
            }
            let bits = v.bits();
            let one = BigInt::one();
            if (v.as_ref() & (v.as_ref() - &one)).is_zero() {
                return Ok((bits - 1) as f64);
            }
            Ok(pyint_ln(n)? / std::f64::consts::LN_2)
        }
    }
}

pub(super) fn float_log2_exact_power(x: f64) -> Option<f64> {
    if !(x > 0.0 && x.is_finite()) {
        return None;
    }
    let bits = x.to_bits();
    let exp = ((bits >> 52) & 0x7ff) as i32;
    let frac = bits & 0x000f_ffff_ffff_ffff;
    if exp == 0 {
        if frac != 0 && (frac & (frac - 1)) == 0 {
            return Some((frac.trailing_zeros() as i32 - 1074) as f64);
        }
    } else if frac == 0 {
        return Some((exp - 1023) as f64);
    }
    None
}

pub(super) fn is_odd_integer_float(x: f64) -> bool {
    x.is_finite() && x.fract() == 0.0 && x.abs() <= u64::MAX as f64 && ((x.abs() as u64) & 1) == 1
}

pub(super) fn math_ln_arg(obj: &PyObjectRef) -> PyResult<f64> {
    match &obj.payload {
        PyObjectPayload::Int(n) => pyint_ln(n),
        _ => {
            let x = math_number_to_float(obj)?;
            if x <= 0.0 {
                return Err(PyException::value_error("math domain error"));
            }
            Ok(x.ln())
        }
    }
}
