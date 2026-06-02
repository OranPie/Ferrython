use ferrython_core::error::PyResult;
use ferrython_core::object::{
    check_args, make_builtin, make_module, PyObject, PyObjectMethods, PyObjectRef,
};

const ONE_THIRD: f64 = 1.0 / 3.0;
const ONE_SIXTH: f64 = 1.0 / 6.0;
const TWO_THIRD: f64 = 2.0 / 3.0;

pub fn create_colorsys_module() -> PyObjectRef {
    make_module(
        "colorsys",
        vec![
            ("rgb_to_yiq", make_builtin(rgb_to_yiq)),
            ("yiq_to_rgb", make_builtin(yiq_to_rgb)),
            ("rgb_to_hls", make_builtin(rgb_to_hls)),
            ("hls_to_rgb", make_builtin(hls_to_rgb)),
            ("rgb_to_hsv", make_builtin(rgb_to_hsv)),
            ("hsv_to_rgb", make_builtin(hsv_to_rgb)),
            ("ONE_THIRD", PyObject::float(ONE_THIRD)),
            ("ONE_SIXTH", PyObject::float(ONE_SIXTH)),
            ("TWO_THIRD", PyObject::float(TWO_THIRD)),
        ],
    )
}

fn triple_args(name: &str, args: &[PyObjectRef]) -> PyResult<(f64, f64, f64)> {
    check_args(name, args, 3)?;
    Ok((
        args[0].to_float()?,
        args[1].to_float()?,
        args[2].to_float()?,
    ))
}

fn triple(a: f64, b: f64, c: f64) -> PyObjectRef {
    PyObject::tuple(vec![
        PyObject::float(a),
        PyObject::float(b),
        PyObject::float(c),
    ])
}

fn rgb_to_yiq(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let (r, g, b) = triple_args("colorsys.rgb_to_yiq", args)?;
    let y = 0.30 * r + 0.59 * g + 0.11 * b;
    let i = 0.74 * (r - y) - 0.27 * (b - y);
    let q = 0.48 * (r - y) + 0.41 * (b - y);
    Ok(triple(y, i, q))
}

fn yiq_to_rgb(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let (y, i, q) = triple_args("colorsys.yiq_to_rgb", args)?;
    let r = (y + 0.9468822170900693 * i + 0.6235565819861433 * q).clamp(0.0, 1.0);
    let g = (y - 0.27478764629897834 * i - 0.6356910791873801 * q).clamp(0.0, 1.0);
    let b = (y - 1.1085450346420322 * i + 1.7090069284064666 * q).clamp(0.0, 1.0);
    Ok(triple(r, g, b))
}

fn rgb_to_hls(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let (r, g, b) = triple_args("colorsys.rgb_to_hls", args)?;
    let maxc = r.max(g).max(b);
    let minc = r.min(g).min(b);
    let sumc = maxc + minc;
    let rangec = maxc - minc;
    let l = sumc / 2.0;
    if minc == maxc {
        return Ok(triple(0.0, l, 0.0));
    }
    let s = if l <= 0.5 {
        rangec / sumc
    } else {
        rangec / (2.0 - sumc)
    };
    let rc = (maxc - r) / rangec;
    let gc = (maxc - g) / rangec;
    let bc = (maxc - b) / rangec;
    let h = if r == maxc {
        bc - gc
    } else if g == maxc {
        2.0 + rc - bc
    } else {
        4.0 + gc - rc
    };
    Ok(triple((h / 6.0).rem_euclid(1.0), l, s))
}

fn hls_to_rgb(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let (h, l, s) = triple_args("colorsys.hls_to_rgb", args)?;
    if s == 0.0 {
        return Ok(triple(l, l, l));
    }
    let m2 = if l <= 0.5 {
        l * (1.0 + s)
    } else {
        l + s - (l * s)
    };
    let m1 = 2.0 * l - m2;
    Ok(triple(
        hls_value(m1, m2, h + ONE_THIRD),
        hls_value(m1, m2, h),
        hls_value(m1, m2, h - ONE_THIRD),
    ))
}

fn hls_value(m1: f64, m2: f64, hue: f64) -> f64 {
    let hue = hue.rem_euclid(1.0);
    if hue < ONE_SIXTH {
        return m1 + (m2 - m1) * hue * 6.0;
    }
    if hue < 0.5 {
        return m2;
    }
    if hue < TWO_THIRD {
        return m1 + (m2 - m1) * (TWO_THIRD - hue) * 6.0;
    }
    m1
}

fn rgb_to_hsv(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let (r, g, b) = triple_args("colorsys.rgb_to_hsv", args)?;
    let maxc = r.max(g).max(b);
    let minc = r.min(g).min(b);
    let rangec = maxc - minc;
    if minc == maxc {
        return Ok(triple(0.0, 0.0, maxc));
    }
    let s = rangec / maxc;
    let rc = (maxc - r) / rangec;
    let gc = (maxc - g) / rangec;
    let bc = (maxc - b) / rangec;
    let h = if r == maxc {
        bc - gc
    } else if g == maxc {
        2.0 + rc - bc
    } else {
        4.0 + gc - rc
    };
    Ok(triple((h / 6.0).rem_euclid(1.0), s, maxc))
}

fn hsv_to_rgb(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let (h, s, v) = triple_args("colorsys.hsv_to_rgb", args)?;
    if s == 0.0 {
        return Ok(triple(v, v, v));
    }
    let scaled_h = h * 6.0;
    let i = scaled_h as i64;
    let f = scaled_h - i as f64;
    let p = v * (1.0 - s);
    let q = v * (1.0 - s * f);
    let t = v * (1.0 - s * (1.0 - f));
    Ok(match i.rem_euclid(6) {
        0 => triple(v, t, p),
        1 => triple(q, v, p),
        2 => triple(p, v, t),
        3 => triple(p, q, v),
        4 => triple(t, p, v),
        5 => triple(v, p, q),
        _ => unreachable!(),
    })
}
