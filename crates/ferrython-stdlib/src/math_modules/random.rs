use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    check_args, make_builtin, make_module, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::{HashableKey, PyInt};
use indexmap::IndexMap;
use num_bigint::{BigInt, Sign};
use num_traits::{ToPrimitive, Zero};
use std::cell::RefCell;

// ── random module ──

pub fn create_random_module() -> PyObjectRef {
    make_module(
        "random",
        vec![
            ("__all__", random_all()),
            ("BPF", PyObject::int(53)),
            ("RECIP_BPF", PyObject::float(2.0f64.powi(-53))),
            ("NV_MAGICCONST", PyObject::float(1.715_527_769_921_413_5)),
            ("TWOPI", PyObject::float(std::f64::consts::TAU)),
            ("LOG4", PyObject::float(4.0f64.ln())),
            ("SG_MAGICCONST", PyObject::float(2.504_077_396_776_274)),
            ("_e", PyObject::float(std::f64::consts::E)),
            ("_pi", PyObject::float(std::f64::consts::PI)),
            ("_sqrt", make_builtin(random_math_sqrt)),
            ("_acos", make_builtin(random_math_acos)),
            ("_cos", make_builtin(random_math_cos)),
            ("_sin", make_builtin(random_math_sin)),
            ("_exp", make_builtin(random_math_exp)),
            ("_log", make_builtin(random_math_log)),
            ("_urandom", make_builtin(random_urandom)),
            ("random", make_builtin(random_random)),
            ("randint", make_builtin(random_randint)),
            ("choice", make_builtin(random_choice)),
            ("shuffle", make_builtin(random_shuffle)),
            ("seed", make_builtin(random_seed)),
            ("randrange", make_builtin(random_randrange)),
            (
                "uniform",
                make_builtin(|args| random_uniform_impl("random.uniform", args)),
            ),
            (
                "sample",
                make_builtin(|args| random_sample_impl("random.sample", args)),
            ),
            (
                "choices",
                make_builtin(|args| random_choices_impl("random.choices", args)),
            ),
            ("gauss", make_builtin(|args| random_gauss_impl(args))),
            (
                "normalvariate",
                make_builtin(|args| random_normalvariate_impl(args)),
            ),
            (
                "expovariate",
                make_builtin(|args| random_expovariate_impl(args)),
            ),
            (
                "triangular",
                make_builtin(|args| random_triangular_impl(args)),
            ),
            ("getrandbits", make_builtin(random_getrandbits)),
            ("getstate", make_builtin(random_getstate)),
            ("setstate", make_builtin(random_setstate)),
            (
                "Random",
                make_builtin(|_args| {
                    let mut attrs = IndexMap::new();
                    attrs.insert(CompactString::from("random"), make_builtin(random_random));
                    attrs.insert(CompactString::from("randint"), make_builtin(random_randint));
                    attrs.insert(CompactString::from("choice"), make_builtin(random_choice));
                    attrs.insert(CompactString::from("shuffle"), make_builtin(random_shuffle));
                    attrs.insert(CompactString::from("seed"), make_builtin(random_seed));
                    attrs.insert(
                        CompactString::from("randrange"),
                        make_builtin(random_randrange),
                    );
                    attrs.insert(
                        CompactString::from("uniform"),
                        make_builtin(|args| random_uniform_impl("Random.uniform", args)),
                    );
                    attrs.insert(
                        CompactString::from("sample"),
                        make_builtin(|args| random_sample_impl("Random.sample", args)),
                    );
                    attrs.insert(
                        CompactString::from("choices"),
                        make_builtin(|args| random_choices_impl("Random.choices", args)),
                    );
                    attrs.insert(
                        CompactString::from("gauss"),
                        make_builtin(|args| random_gauss_impl(args)),
                    );
                    attrs.insert(
                        CompactString::from("getrandbits"),
                        make_builtin(random_getrandbits),
                    );
                    attrs.insert(
                        CompactString::from("getstate"),
                        make_builtin(|args| random_getstate(args)),
                    );
                    attrs.insert(
                        CompactString::from("setstate"),
                        make_builtin(|args| random_setstate(args)),
                    );
                    attrs.insert(
                        CompactString::from("normalvariate"),
                        make_builtin(|args| random_normalvariate_impl(args)),
                    );
                    attrs.insert(
                        CompactString::from("expovariate"),
                        make_builtin(|args| random_expovariate_impl(args)),
                    );
                    attrs.insert(
                        CompactString::from("triangular"),
                        make_builtin(|args| random_triangular_impl(args)),
                    );
                    Ok(PyObject::module_with_attrs(
                        CompactString::from("Random"),
                        attrs,
                    ))
                }),
            ),
            (
                "SystemRandom",
                make_builtin(|_args| {
                    let mut attrs = IndexMap::new();
                    attrs.insert(CompactString::from("random"), make_builtin(random_random));
                    attrs.insert(CompactString::from("randint"), make_builtin(random_randint));
                    attrs.insert(CompactString::from("choice"), make_builtin(random_choice));
                    attrs.insert(CompactString::from("shuffle"), make_builtin(random_shuffle));
                    attrs.insert(CompactString::from("seed"), make_builtin(random_seed));
                    attrs.insert(
                        CompactString::from("randrange"),
                        make_builtin(random_randrange),
                    );
                    attrs.insert(
                        CompactString::from("sample"),
                        make_builtin(|args| random_sample_impl("SystemRandom.sample", args)),
                    );
                    attrs.insert(
                        CompactString::from("choices"),
                        make_builtin(|args| random_choices_impl("SystemRandom.choices", args)),
                    );
                    attrs.insert(
                        CompactString::from("uniform"),
                        make_builtin(|args| random_uniform_impl("SystemRandom.uniform", args)),
                    );
                    attrs.insert(
                        CompactString::from("gauss"),
                        make_builtin(|args| random_gauss_impl(args)),
                    );
                    attrs.insert(
                        CompactString::from("normalvariate"),
                        make_builtin(|args| random_normalvariate_impl(args)),
                    );
                    attrs.insert(
                        CompactString::from("expovariate"),
                        make_builtin(|args| random_expovariate_impl(args)),
                    );
                    attrs.insert(
                        CompactString::from("triangular"),
                        make_builtin(|args| random_triangular_impl(args)),
                    );
                    attrs.insert(
                        CompactString::from("getrandbits"),
                        make_builtin(random_getrandbits),
                    );
                    attrs.insert(
                        CompactString::from("getstate"),
                        make_builtin(|_| {
                            Err(PyException::not_implemented_error(
                                "System entropy source does not have state",
                            ))
                        }),
                    );
                    attrs.insert(
                        CompactString::from("setstate"),
                        make_builtin(|_| {
                            Err(PyException::not_implemented_error(
                                "System entropy source does not have state",
                            ))
                        }),
                    );
                    Ok(PyObject::module_with_attrs(
                        CompactString::from("SystemRandom"),
                        attrs,
                    ))
                }),
            ),
        ],
    )
}

// ── Seeded PRNG (xoshiro256**) for reproducible random sequences ──

/// Xoshiro256** state — fast, high-quality PRNG with proper seeding support.
struct Xoshiro256 {
    s: [u64; 4],
}

impl Xoshiro256 {
    fn new(seed: u64) -> Self {
        // SplitMix64 to expand a single u64 seed into 4 state words
        let mut z = seed;
        let mut s = [0u64; 4];
        for item in &mut s {
            z = z.wrapping_add(0x9e3779b97f4a7c15);
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
            *item = z ^ (z >> 31);
        }
        Self { s }
    }

    fn next_u64(&mut self) -> u64 {
        let result = (self.s[1].wrapping_mul(5)).rotate_left(7).wrapping_mul(9);
        let t = self.s[1] << 17;
        self.s[2] ^= self.s[0];
        self.s[3] ^= self.s[1];
        self.s[1] ^= self.s[2];
        self.s[0] ^= self.s[3];
        self.s[2] ^= t;
        self.s[3] = self.s[3].rotate_left(45);
        result
    }

    fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}

thread_local! {
    static RNG: RefCell<Xoshiro256> = RefCell::new({
        // Default seed from system time + thread id
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap_or_default().as_nanos() as u64;
        let tid = format!("{:?}", std::thread::current().id()).len() as u64;
        Xoshiro256::new(nanos ^ tid.wrapping_mul(0x517cc1b727220a95))
    });
}

fn simple_random() -> f64 {
    RNG.with(|rng| rng.borrow_mut().next_f64())
}

fn random_all() -> PyObjectRef {
    PyObject::list(
        [
            "Random",
            "SystemRandom",
            "random",
            "seed",
            "getstate",
            "setstate",
            "getrandbits",
            "randrange",
            "randint",
            "choice",
            "choices",
            "shuffle",
            "sample",
            "uniform",
            "triangular",
            "normalvariate",
            "gauss",
            "expovariate",
            "vonmisesvariate",
            "gammavariate",
            "betavariate",
            "paretovariate",
            "weibullvariate",
            "lognormvariate",
        ]
        .into_iter()
        .map(|name| PyObject::str_val(CompactString::from(name)))
        .collect(),
    )
}

fn random_math_sqrt(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("random._sqrt", args, 1)?;
    Ok(PyObject::float(args[0].to_float()?.sqrt()))
}

fn random_math_acos(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("random._acos", args, 1)?;
    Ok(PyObject::float(args[0].to_float()?.acos()))
}

fn random_math_cos(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("random._cos", args, 1)?;
    Ok(PyObject::float(args[0].to_float()?.cos()))
}

fn random_math_sin(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("random._sin", args, 1)?;
    Ok(PyObject::float(args[0].to_float()?.sin()))
}

fn random_math_exp(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("random._exp", args, 1)?;
    Ok(PyObject::float(args[0].to_float()?.exp()))
}

fn random_math_log(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() || args.len() > 2 {
        return Err(PyException::type_error("log expected 1 or 2 arguments"));
    }
    let value = args[0].to_float()?;
    if args.len() == 2 {
        return Ok(PyObject::float(value.log(args[1].to_float()?)));
    }
    Ok(PyObject::float(value.ln()))
}

fn random_urandom(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("random._urandom", args, 1)?;
    let n = args[0].to_int()?;
    if n < 0 {
        return Err(PyException::value_error("negative argument not allowed"));
    }
    let mut bytes = Vec::with_capacity(n as usize);
    RNG.with(|rng| {
        let mut r = rng.borrow_mut();
        while bytes.len() < n as usize {
            bytes.extend_from_slice(&r.next_u64().to_le_bytes());
        }
    });
    bytes.truncate(n as usize);
    Ok(PyObject::bytes(bytes))
}

fn random_uniform_impl(name: &str, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args(name, args, 2)?;
    let a = args[0].to_float()?;
    let b = args[1].to_float()?;
    Ok(PyObject::float(a + simple_random() * (b - a)))
}

fn random_gauss_impl(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let mu = if !args.is_empty() {
        args[0].to_float()?
    } else {
        0.0
    };
    let sigma = if args.len() > 1 {
        args[1].to_float()?
    } else {
        1.0
    };
    let u1 = simple_random().max(1e-10);
    let u2 = simple_random();
    let z = (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos();
    Ok(PyObject::float(mu + sigma * z))
}

fn random_normalvariate_impl(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    random_gauss_impl(args)
}

fn random_expovariate_impl(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("random.expovariate", args, 1)?;
    let lambd = args[0].to_float()?;
    if lambd == 0.0 {
        return Err(PyException::value_error("expovariate: lambd must not be 0"));
    }
    let u = simple_random().max(1e-10);
    Ok(PyObject::float(-u.ln() / lambd))
}

fn random_triangular_impl(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let low = if !args.is_empty() {
        args[0].to_float()?
    } else {
        0.0
    };
    let high = if args.len() > 1 {
        args[1].to_float()?
    } else {
        1.0
    };
    if low == high {
        return Ok(PyObject::float(low));
    }
    let mode = if args.len() > 2 {
        args[2].to_float()?
    } else {
        (low + high) / 2.0
    };
    let u = simple_random();
    let c = (mode - low) / (high - low);
    if u < c {
        Ok(PyObject::float(
            low + (u * (high - low) * (mode - low)).sqrt(),
        ))
    } else {
        Ok(PyObject::float(
            high - ((1.0 - u) * (high - low) * (high - mode)).sqrt(),
        ))
    }
}

fn kwargs_get<'a>(args: &'a [PyObjectRef], name: &str) -> Option<PyObjectRef> {
    let PyObjectPayload::Dict(d) = &args.last()?.payload else {
        return None;
    };
    d.read()
        .get(&HashableKey::str_key(CompactString::from(name)))
        .cloned()
}

fn visible_sequence(obj: &PyObjectRef) -> PyResult<Vec<PyObjectRef>> {
    if matches!(&obj.payload, PyObjectPayload::Dict(_)) {
        return Err(PyException::type_error("Population must be a sequence"));
    }
    obj.to_list()
}

fn random_sample_impl(name: &str, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args(name, args, 2)?;
    if matches!(&args[0].payload, PyObjectPayload::Dict(_)) {
        return Err(PyException::type_error("Population must be a sequence"));
    }
    let k = args[1].to_int()?;
    if k < 0 {
        return Err(PyException::value_error("Sample larger than population"));
    }
    let k = k as usize;
    let mut pool = args[0].to_list()?;
    if k > pool.len() {
        return Err(PyException::value_error("Sample larger than population"));
    }
    let mut result = Vec::with_capacity(k);
    for _ in 0..k {
        let idx = (simple_random() * pool.len() as f64) as usize;
        let idx = idx.min(pool.len().saturating_sub(1));
        result.push(pool.remove(idx));
    }
    Ok(PyObject::list(result))
}

fn positional_or_kw_population(args: &[PyObjectRef]) -> Option<PyObjectRef> {
    args.first()
        .filter(|arg| !matches!(&arg.payload, PyObjectPayload::Dict(_)))
        .cloned()
        .or_else(|| kwargs_get(args, "population"))
}

fn weights_arg(args: &[PyObjectRef]) -> Option<PyObjectRef> {
    args.iter()
        .skip(1)
        .find(|arg| !matches!(&arg.payload, PyObjectPayload::Dict(_)))
        .cloned()
        .or_else(|| kwargs_get(args, "weights"))
}

fn random_choices_impl(name: &str, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let population = positional_or_kw_population(args)
        .ok_or_else(|| PyException::type_error(format!("{name} requires population")))?;
    let items = visible_sequence(&population)?;
    if items.is_empty() {
        return Err(PyException::index_error(
            "Cannot choose from an empty population",
        ));
    }
    let k = kwargs_get(args, "k")
        .map(|value| value.to_int())
        .transpose()?
        .unwrap_or(1);
    if k <= 0 {
        return Ok(PyObject::list(vec![]));
    }
    let cum_weights = kwargs_get(args, "cum_weights");
    let weights = weights_arg(args);
    if cum_weights.is_some() && weights.is_some() {
        return Err(PyException::type_error(
            "Cannot specify both weights and cumulative weights",
        ));
    }
    let mut cumulative: Option<Vec<f64>> = None;
    if let Some(weight_obj) = weights {
        if !matches!(&weight_obj.payload, PyObjectPayload::None) {
            let values = weight_obj.to_list()?;
            if values.len() != items.len() {
                return Err(PyException::value_error(
                    "The number of weights does not match the population",
                ));
            }
            let mut running = 0.0;
            let mut out = Vec::with_capacity(values.len());
            for value in values {
                running += value.to_float()?;
                out.push(running);
            }
            cumulative = Some(out);
        }
    }
    if let Some(cum_obj) = cum_weights {
        let values = cum_obj.to_list()?;
        if values.len() != items.len() {
            return Err(PyException::value_error(
                "The number of weights does not match the population",
            ));
        }
        cumulative = Some(
            values
                .iter()
                .map(PyObjectMethods::to_float)
                .collect::<PyResult<Vec<_>>>()?,
        );
    }
    let mut result = Vec::with_capacity(k as usize);
    if let Some(cumulative) = cumulative {
        let total = *cumulative.last().unwrap_or(&0.0);
        if total <= 0.0 || !total.is_finite() {
            return Err(PyException::value_error("Total of weights must be finite"));
        }
        for _ in 0..k {
            let needle = simple_random() * total;
            let idx = cumulative
                .iter()
                .position(|weight| needle < *weight)
                .unwrap_or_else(|| cumulative.len().saturating_sub(1));
            result.push(items[idx.min(items.len() - 1)].clone());
        }
    } else {
        for _ in 0..k {
            let idx = (simple_random() * items.len() as f64) as usize;
            result.push(items[idx.min(items.len() - 1)].clone());
        }
    }
    Ok(PyObject::list(result))
}

fn random_getstate(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    RNG.with(|rng| {
        let r = rng.borrow();
        Ok(PyObject::tuple(vec![
            PyObject::big_int(BigInt::from(r.s[0])),
            PyObject::big_int(BigInt::from(r.s[1])),
            PyObject::big_int(BigInt::from(r.s[2])),
            PyObject::big_int(BigInt::from(r.s[3])),
        ]))
    })
}

fn random_setstate(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("setstate() requires 1 argument"));
    }
    let PyObjectPayload::Tuple(items) = &args[0].payload else {
        return Err(PyException::type_error(
            "state must be a 4-tuple of integers",
        ));
    };
    if items.len() < 4 {
        return Err(PyException::type_error(
            "state must be a 4-tuple of integers",
        ));
    }
    let s0 = random_state_word(&items[0])?;
    let s1 = random_state_word(&items[1])?;
    let s2 = random_state_word(&items[2])?;
    let s3 = random_state_word(&items[3])?;
    RNG.with(|rng| {
        let mut r = rng.borrow_mut();
        r.s = [s0, s1, s2, s3];
    });
    Ok(PyObject::none())
}

fn random_state_word(obj: &PyObjectRef) -> PyResult<u64> {
    match &obj.payload {
        PyObjectPayload::Int(PyInt::Small(value)) => Ok(*value as u64),
        PyObjectPayload::Int(PyInt::Big(value)) => value
            .to_u64()
            .or_else(|| value.to_i64().map(|v| v as u64))
            .ok_or_else(|| PyException::overflow_error("int too large")),
        PyObjectPayload::Bool(value) => Ok(u64::from(*value)),
        _ => Ok(obj.to_int()? as u64),
    }
}

fn random_getrandbits(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("random.getrandbits", args, 1)?;
    let k = args[0].to_int()?;
    if k < 0 {
        return Err(PyException::value_error(
            "number of bits must be greater than zero",
        ));
    }
    if k == 0 {
        return Ok(PyObject::int(0));
    }
    let mut value = BigInt::zero();
    let mut remaining = k as usize;
    RNG.with(|rng| {
        let mut r = rng.borrow_mut();
        while remaining > 0 {
            let take = remaining.min(64);
            let mut chunk = r.next_u64();
            if take < 64 {
                chunk &= (1u64 << take) - 1;
            }
            value = (value.clone() << take) | BigInt::from(chunk);
            remaining -= take;
        }
    });
    if value.sign() == Sign::NoSign {
        return Ok(PyObject::int(0));
    }
    if let Some(v) = value.to_i64() {
        Ok(PyObject::int(v))
    } else {
        Ok(PyObject::big_int(value))
    }
}

fn random_random(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::float(simple_random()))
}
fn random_randint(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("random.randint", args, 2)?;
    let a = args[0].to_int()?;
    let b = args[1].to_int()?;
    let range = (b - a + 1) as f64;
    Ok(PyObject::int(a + (simple_random() * range) as i64))
}
fn random_choice(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("random.choice", args, 1)?;
    let items = args[0].to_list()?;
    if items.is_empty() {
        return Err(PyException::index_error(
            "Cannot choose from an empty sequence",
        ));
    }
    let idx = (simple_random() * items.len() as f64) as usize;
    Ok(items[idx.min(items.len() - 1)].clone())
}
fn random_shuffle(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("random.shuffle", args, 1)?;
    // Fisher-Yates in-place shuffle
    if let PyObjectPayload::List(list_arc) = &args[0].payload {
        let mut items = list_arc.write();
        let n = items.len();
        for i in (1..n).rev() {
            let j = (simple_random() * (i + 1) as f64) as usize;
            let j = j.min(i);
            items.swap(i, j);
        }
    }
    Ok(PyObject::none())
}
fn random_seed(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let seed_val: u64 = if args.is_empty() || matches!(args[0].payload, PyObjectPayload::None) {
        // No seed or None → use system time
        std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64
    } else {
        match &args[0].payload {
            PyObjectPayload::Int(n) => {
                let v = n.to_i64().unwrap_or(0);
                v as u64
            }
            PyObjectPayload::Float(f) => f.to_bits(),
            PyObjectPayload::Str(s) => {
                // Hash the string for seed
                let mut h: u64 = 0;
                for b in s.as_bytes() {
                    h = h.wrapping_mul(31).wrapping_add(*b as u64);
                }
                h
            }
            _ => args[0].py_to_string().len() as u64,
        }
    };
    RNG.with(|rng| *rng.borrow_mut() = Xoshiro256::new(seed_val));
    Ok(PyObject::none())
}
fn random_randrange(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "randrange requires at least 1 argument",
        ));
    }
    let start = if args.len() == 1 {
        0
    } else {
        args[0].to_int()?
    };
    let stop = if args.len() == 1 {
        args[0].to_int()?
    } else {
        args[1].to_int()?
    };
    let step = if args.len() > 2 { args[2].to_int()? } else { 1 };
    let range = ((stop - start) as f64 / step as f64).ceil() as i64;
    if range <= 0 {
        return Err(PyException::value_error("empty range for randrange()"));
    }
    let idx = (simple_random() * range as f64) as i64;
    Ok(PyObject::int(start + idx * step))
}
