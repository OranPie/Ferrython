use super::super::number::math_number_to_float;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{check_args, PyObject, PyObjectMethods, PyObjectRef};
use num_bigint::{BigInt, Sign};
use num_traits::{One, Signed, ToPrimitive, Zero};

unsafe extern "C" {
    fn ldexp(x: libc::c_double, exp: libc::c_int) -> libc::c_double;
}

pub(super) fn math_erf(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.erf", args, 1)?;
    let x = math_number_to_float(&args[0])?;
    // Abramowitz and Stegun approximation (7.1.26)
    let t = 1.0 / (1.0 + 0.3275911 * x.abs());
    let poly = t
        * (0.254829592
            + t * (-0.284496736 + t * (1.421413741 + t * (-1.453152027 + t * 1.061405429))));
    let erf = 1.0 - poly * (-x * x).exp();
    Ok(PyObject::float(if x < 0.0 { -erf } else { erf }))
}
pub(super) fn math_erfc(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.erfc", args, 1)?;
    let x = math_number_to_float(&args[0])?;
    let t = 1.0 / (1.0 + 0.3275911 * x.abs());
    let poly = t
        * (0.254829592
            + t * (-0.284496736 + t * (1.421413741 + t * (-1.453152027 + t * 1.061405429))));
    let erf = 1.0 - poly * (-x * x).exp();
    Ok(PyObject::float(if x < 0.0 { 1.0 + erf } else { 1.0 - erf }))
}
pub(super) fn math_gamma(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.gamma", args, 1)?;
    let x = math_number_to_float(&args[0])?;
    if x <= 0.0 && x == x.floor() {
        return Err(PyException::value_error("math domain error"));
    }
    // Lanczos approximation
    Ok(PyObject::float(lanczos_gamma(x)))
}
pub(super) fn math_lgamma(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.lgamma", args, 1)?;
    let x = math_number_to_float(&args[0])?;
    if x <= 0.0 && x == x.floor() {
        return Err(PyException::value_error("math domain error"));
    }
    Ok(PyObject::float(lanczos_gamma(x).abs().ln()))
}

pub(super) fn math_fsum(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.fsum", args, 1)?;
    let items = args[0].to_list()?;
    let mut total = BigInt::zero();
    let mut total_exp = 0i32;
    let mut initialized = false;
    let mut pos_inf = false;
    let mut neg_inf = false;
    let mut has_nan = false;
    for item in &items {
        let x = math_number_to_float(item)?;
        if x.is_nan() {
            has_nan = true;
            continue;
        }
        if x.is_infinite() {
            if x.is_sign_positive() {
                pos_inf = true;
            } else {
                neg_inf = true;
            }
            continue;
        }
        if x == 0.0 {
            continue;
        }
        let bits = x.to_bits();
        let negative = (bits >> 63) != 0;
        let raw_exp = ((bits >> 52) & 0x7ff) as i32;
        let frac = bits & 0x000f_ffff_ffff_ffff;
        let (mantissa, exp) = if raw_exp == 0 {
            (frac, -1074)
        } else {
            ((1u64 << 52) | frac, raw_exp - 1075)
        };
        let mut mant = BigInt::from(mantissa);
        if negative {
            mant = -mant;
        }
        if !initialized {
            total = mant;
            total_exp = exp;
            initialized = true;
        } else if exp < total_exp {
            total <<= (total_exp - exp) as usize;
            total += mant;
            total_exp = exp;
        } else {
            mant <<= (exp - total_exp) as usize;
            total += mant;
        }
    }
    if pos_inf && neg_inf {
        return Err(PyException::value_error("-inf + inf in fsum"));
    }
    if pos_inf {
        return Ok(PyObject::float(f64::INFINITY));
    }
    if neg_inf {
        return Ok(PyObject::float(f64::NEG_INFINITY));
    }
    if has_nan {
        return Ok(PyObject::float(f64::NAN));
    }
    if total.is_zero() {
        return Ok(PyObject::float(0.0));
    }

    let sign = total.sign();
    let mut mant = total.abs();
    let bit_len = mant.bits() as i32;
    let tail = (bit_len - 53).max(-1074 - total_exp);
    if tail > 0 {
        let mask = (BigInt::one() << tail as usize) - 1u32;
        let remainder = &mant & &mask;
        mant >>= tail as usize;
        let half = BigInt::one() << (tail as usize - 1);
        if remainder > half || (remainder == half && (&mant & BigInt::one()).is_one()) {
            mant += 1u32;
        }
        total_exp += tail;
    }
    let mut rounded = mant
        .to_f64()
        .ok_or_else(|| PyException::overflow_error("intermediate overflow in fsum"))?;
    if sign == Sign::Minus {
        rounded = -rounded;
    }
    let result = unsafe { ldexp(rounded, total_exp as libc::c_int) };
    if result.is_infinite() {
        return Err(PyException::overflow_error("intermediate overflow in fsum"));
    }
    Ok(PyObject::float(result))
}

pub(super) fn math_dist(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.dist", args, 2)?;
    let p = args[0].to_list()?;
    let q = args[1].to_list()?;
    if p.len() != q.len() {
        return Err(PyException::value_error(
            "both points must have the same number of dimensions",
        ));
    }
    let mut diffs = Vec::with_capacity(p.len());
    let mut max = 0.0f64;
    let mut has_nan = false;
    for (a, b) in p.iter().zip(q.iter()) {
        let diff = (math_number_to_float(a)? - math_number_to_float(b)?).abs();
        if diff.is_infinite() {
            return Ok(PyObject::float(f64::INFINITY));
        }
        if diff.is_nan() {
            has_nan = true;
        } else if diff > max {
            max = diff;
        }
        diffs.push(diff);
    }
    if has_nan {
        return Ok(PyObject::float(f64::NAN));
    }
    if max == 0.0 {
        return Ok(PyObject::float(0.0));
    }
    let mut sum = 0.0f64;
    for diff in diffs {
        let scaled = diff / max;
        sum += scaled * scaled;
    }
    Ok(PyObject::float(max * sum.sqrt()))
}

fn lanczos_gamma(x: f64) -> f64 {
    if x < 0.5 {
        std::f64::consts::PI / ((std::f64::consts::PI * x).sin() * lanczos_gamma(1.0 - x))
    } else {
        let g = 7.0;
        let coefs = [
            0.99999999999980993,
            676.5203681218851,
            -1259.1392167224028,
            771.32342877765313,
            -176.61502916214059,
            12.507343278686905,
            -0.13857109526572012,
            9.9843695780195716e-6,
            1.5056327351493116e-7,
        ];
        let z = x - 1.0;
        let mut sum = coefs[0];
        for (i, &c) in coefs[1..].iter().enumerate() {
            sum += c / (z + i as f64 + 1.0);
        }
        let t = z + g + 0.5;
        (2.0 * std::f64::consts::PI).sqrt() * t.powf(z + 0.5) * (-t).exp() * sum
    }
}
