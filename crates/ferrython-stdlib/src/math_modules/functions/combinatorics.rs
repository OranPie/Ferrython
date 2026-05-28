use super::super::number::{bigint_to_object, index_bigint, isqrt_bigint};
use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{check_args, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};
use ferrython_core::types::{HashableKey, PyInt};
use num_bigint::{BigInt, Sign};
use num_integer::Integer;
use num_traits::{One, Signed, ToPrimitive, Zero};

pub(super) fn math_gcd(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::int(0));
    }
    if args.len() == 1 {
        return Ok(bigint_to_object(index_bigint(&args[0], "gcd")?.abs()));
    }
    let mut result = index_bigint(&args[0], "gcd")?.abs();
    for arg in &args[1..] {
        result = result.gcd(&index_bigint(arg, "gcd")?.abs());
    }
    Ok(bigint_to_object(result))
}
pub(super) fn math_factorial(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.factorial", args, 1)?;
    let n = match &args[0].payload {
        PyObjectPayload::Int(PyInt::Small(v)) => *v,
        PyObjectPayload::Int(PyInt::Big(v)) => {
            if v.sign() == Sign::Minus {
                return Err(PyException::value_error(
                    "factorial() not defined for negative values",
                ));
            }
            v.to_i64()
                .ok_or_else(|| PyException::value_error("factorial() argument too large"))?
        }
        PyObjectPayload::Bool(b) => {
            if *b {
                1
            } else {
                0
            }
        }
        PyObjectPayload::Float(f) => {
            if *f < 0.0 {
                return Err(PyException::value_error(
                    "factorial() not defined for negative values",
                ));
            }
            if !f.is_finite() || f.fract() != 0.0 {
                return Err(PyException::value_error(
                    "factorial() only accepts integral values",
                ));
            }
            if *f > i64::MAX as f64 || *f < i64::MIN as f64 {
                return Err(PyException::overflow_error(
                    "factorial() argument too large",
                ));
            }
            *f as i64
        }
        _ => {
            return Err(PyException::type_error(
                "factorial() argument must be an integer",
            ))
        }
    };
    if n < 0 {
        return Err(PyException::value_error(
            "factorial() not defined for negative values",
        ));
    }
    let mut result = BigInt::one();
    for i in 2..=n {
        result *= i;
    }
    Ok(PyObject::big_int(result))
}

pub(super) fn math_isqrt(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("isqrt", args, 1)?;
    let n = index_bigint(&args[0], "isqrt")?;
    if n.sign() == Sign::Minus {
        return Err(PyException::value_error("isqrt() argument must be >= 0"));
    }
    Ok(bigint_to_object(isqrt_bigint(&n)))
}
pub(super) fn math_comb(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.comb", args, 2)?;
    let n = index_bigint(&args[0], "comb")?;
    let k = index_bigint(&args[1], "comb")?;
    if n.sign() == Sign::Minus {
        return Err(PyException::value_error("n must be a non-negative integer"));
    }
    if k.sign() == Sign::Minus {
        return Err(PyException::value_error("k must be a non-negative integer"));
    }
    if k > n {
        return Ok(PyObject::int(0));
    }
    let n_minus_k = &n - &k;
    let k = if k > n_minus_k { n_minus_k } else { k };
    if k.is_zero() {
        return Ok(PyObject::int(1));
    }
    if k.is_one() {
        return Ok(bigint_to_object(n));
    }
    if k == BigInt::from(2) {
        return Ok(bigint_to_object((&n * (&n - 1u32)) / 2u32));
    }
    let Some(k_u64) = k.to_u64() else {
        return Err(PyException::overflow_error("comb() argument too large"));
    };
    if k_u64 > 1_000_000 {
        return Err(PyException::overflow_error("comb() argument too large"));
    }
    let mut result = BigInt::one();
    for i in 1..=k_u64 {
        let i_big = BigInt::from(i);
        result *= &n - &k + &i_big;
        result /= i_big;
    }
    Ok(bigint_to_object(result))
}

pub(super) fn math_perm(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() || args.len() > 2 {
        return Err(PyException::type_error("perm() requires 1 or 2 arguments"));
    }
    let n = index_bigint(&args[0], "perm")?;
    let k = if args.len() == 2 && !matches!(&args[1].payload, PyObjectPayload::None) {
        index_bigint(&args[1], "perm")?
    } else {
        n.clone()
    };
    if n.sign() == Sign::Minus {
        return Err(PyException::value_error("n must be a non-negative integer"));
    }
    if k.sign() == Sign::Minus {
        return Err(PyException::value_error("k must be a non-negative integer"));
    }
    if k > n {
        return Ok(PyObject::int(0));
    }
    if k.is_zero() {
        return Ok(PyObject::int(1));
    }
    if k.is_one() {
        return Ok(bigint_to_object(n));
    }
    if k == BigInt::from(2) {
        return Ok(bigint_to_object(&n * (&n - 1u32)));
    }
    let Some(k_u64) = k.to_u64() else {
        return Err(PyException::overflow_error("perm() argument too large"));
    };
    if k_u64 > 1_000_000 {
        return Err(PyException::overflow_error("perm() argument too large"));
    }
    let mut result = BigInt::one();
    for i in 0..k_u64 {
        result *= &n - i;
    }
    Ok(bigint_to_object(result))
}

pub(super) fn math_prod(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "prod() requires at least 1 argument",
        ));
    }
    let mut positional_end = args.len();
    let mut start = PyObject::int(1);
    if args.len() > 1 {
        if let Some(PyObjectPayload::Dict(d)) = args.last().map(|a| &a.payload) {
            let map = d.read();
            if let Some(v) = map.get(&HashableKey::str_key(CompactString::from("start"))) {
                start = v.clone();
            }
            positional_end -= 1;
        }
    }
    if positional_end != 1 {
        return Err(PyException::type_error(
            "prod() takes exactly 1 positional argument",
        ));
    }
    let items = args[0].to_list()?;
    let mut int_product = match &start.payload {
        PyObjectPayload::Int(PyInt::Small(v)) => Some(BigInt::from(*v)),
        PyObjectPayload::Int(PyInt::Big(v)) => Some(v.as_ref().clone()),
        PyObjectPayload::Bool(b) => Some(BigInt::from(if *b { 1 } else { 0 })),
        _ => None,
    };
    let mut product = start;
    for item in &items {
        if let Some(acc) = int_product.as_mut() {
            match &item.payload {
                PyObjectPayload::Int(PyInt::Small(v)) => {
                    *acc *= *v;
                    continue;
                }
                PyObjectPayload::Int(PyInt::Big(v)) => {
                    *acc *= v.as_ref();
                    continue;
                }
                PyObjectPayload::Bool(b) => {
                    *acc *= if *b { 1 } else { 0 };
                    continue;
                }
                _ => {
                    product = bigint_to_object(acc.clone());
                    int_product = None;
                }
            }
        }
        product = prod_multiply(&product, item)?;
    }
    if let Some(product) = int_product {
        Ok(bigint_to_object(product))
    } else {
        Ok(product)
    }
}

fn prod_multiply(a: &PyObjectRef, b: &PyObjectRef) -> PyResult<PyObjectRef> {
    if matches!(&a.payload, PyObjectPayload::Instance(_)) {
        if let Some(method) = a.get_attr("__mul__") {
            let result = ferrython_core::object::call_callable(&method, std::slice::from_ref(b))?;
            if !matches!(&result.payload, PyObjectPayload::NotImplemented) {
                return Ok(result);
            }
        }
    }
    if matches!(&b.payload, PyObjectPayload::Instance(_)) {
        if let Some(method) = b.get_attr("__rmul__").or_else(|| b.get_attr("__mul__")) {
            let result = ferrython_core::object::call_callable(&method, std::slice::from_ref(a))?;
            if !matches!(&result.payload, PyObjectPayload::NotImplemented) {
                return Ok(result);
            }
        }
    }
    let result = a.mul(b)?;
    if matches!(&result.payload, PyObjectPayload::NotImplemented) {
        Err(PyException::type_error(format!(
            "unsupported operand type(s) for *: '{}' and '{}'",
            a.type_name(),
            b.type_name()
        )))
    } else {
        Ok(result)
    }
}

pub(super) fn math_lcm(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::int(0));
    }
    fn gcd(a: i64, b: i64) -> i64 {
        if b == 0 {
            a.abs()
        } else {
            gcd(b, a % b)
        }
    }
    let mut result = args[0].to_int()?.abs();
    for arg in &args[1..] {
        let b = arg.to_int()?.abs();
        if b == 0 {
            return Ok(PyObject::int(0));
        }
        result = result / gcd(result, b) * b;
    }
    Ok(PyObject::int(result))
}
