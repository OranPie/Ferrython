use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    check_args, make_builtin, make_module, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::cell::RefCell;

// ── random module ──

pub fn create_random_module() -> PyObjectRef {
    make_module(
        "random",
        vec![
            ("random", make_builtin(random_random)),
            ("randint", make_builtin(random_randint)),
            ("choice", make_builtin(random_choice)),
            ("shuffle", make_builtin(random_shuffle)),
            ("seed", make_builtin(random_seed)),
            ("randrange", make_builtin(random_randrange)),
            (
                "uniform",
                make_builtin(|args| {
                    check_args("random.uniform", args, 2)?;
                    let a = args[0].to_float()?;
                    let b = args[1].to_float()?;
                    Ok(PyObject::float(a + simple_random() * (b - a)))
                }),
            ),
            (
                "sample",
                make_builtin(|args| {
                    check_args("random.sample", args, 2)?;
                    let items = args[0].to_list()?;
                    let k = args[1].to_int()? as usize;
                    if k > items.len() {
                        return Err(PyException::value_error("Sample larger than population"));
                    }
                    let mut result = Vec::with_capacity(k);
                    let mut pool = items.clone();
                    for _ in 0..k {
                        let idx = (simple_random() * pool.len() as f64) as usize;
                        let idx = idx.min(pool.len() - 1);
                        result.push(pool.remove(idx));
                    }
                    Ok(PyObject::list(result))
                }),
            ),
            (
                "choices",
                make_builtin(|args| {
                    if args.is_empty() {
                        return Err(PyException::type_error(
                            "random.choices requires at least 1 argument",
                        ));
                    }
                    let items = args[0].to_list()?;
                    let mut k = 1usize;
                    let mut weights: Option<Vec<f64>> = None;
                    for arg in args.iter().skip(1) {
                        if let PyObjectPayload::Dict(d) = &arg.payload {
                            let d = d.read();
                            if let Some(kv) = d.get(&HashableKey::str_key(CompactString::from("k")))
                            {
                                k = kv.to_int()? as usize;
                            }
                            if let Some(wv) =
                                d.get(&HashableKey::str_key(CompactString::from("weights")))
                            {
                                let wl = wv.to_list()?;
                                weights =
                                    Some(wl.iter().map(|w| w.to_float().unwrap_or(1.0)).collect());
                            }
                            if let Some(cwv) =
                                d.get(&HashableKey::str_key(CompactString::from("cum_weights")))
                            {
                                let cwl = cwv.to_list()?;
                                let cw: Vec<f64> =
                                    cwl.iter().map(|w| w.to_float().unwrap_or(0.0)).collect();
                                // Convert cumulative weights back to regular weights
                                let mut w = Vec::with_capacity(cw.len());
                                for i in 0..cw.len() {
                                    w.push(if i == 0 { cw[0] } else { cw[i] - cw[i - 1] });
                                }
                                weights = Some(w);
                            }
                        }
                    }
                    if items.is_empty() {
                        return Err(PyException::value_error(
                            "Cannot choose from an empty population",
                        ));
                    }
                    let mut result = Vec::with_capacity(k);
                    if let Some(ref w) = weights {
                        let total: f64 = w.iter().sum();
                        for _ in 0..k {
                            let mut r = simple_random() * total;
                            let mut chosen = items.len() - 1;
                            for (i, &weight) in w.iter().enumerate() {
                                r -= weight;
                                if r <= 0.0 {
                                    chosen = i;
                                    break;
                                }
                            }
                            result.push(items[chosen.min(items.len() - 1)].clone());
                        }
                    } else {
                        for _ in 0..k {
                            let idx = (simple_random() * items.len() as f64) as usize;
                            result.push(items[idx.min(items.len() - 1)].clone());
                        }
                    }
                    Ok(PyObject::list(result))
                }),
            ),
            (
                "gauss",
                make_builtin(|args| {
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
                    // Box-Muller transform
                    let u1 = simple_random().max(1e-10);
                    let u2 = simple_random();
                    let z = (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos();
                    Ok(PyObject::float(mu + sigma * z))
                }),
            ),
            (
                "normalvariate",
                make_builtin(|args| {
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
                }),
            ),
            (
                "expovariate",
                make_builtin(|args| {
                    check_args("random.expovariate", args, 1)?;
                    let lambd = args[0].to_float()?;
                    if lambd == 0.0 {
                        return Err(PyException::value_error("expovariate: lambd must not be 0"));
                    }
                    let u = simple_random().max(1e-10);
                    Ok(PyObject::float(-u.ln() / lambd))
                }),
            ),
            (
                "triangular",
                make_builtin(|args| {
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
                }),
            ),
            (
                "getrandbits",
                make_builtin(|args| {
                    check_args("random.getrandbits", args, 1)?;
                    let k = args[0].to_int()? as u32;
                    if k == 0 {
                        return Ok(PyObject::int(0));
                    }
                    let mut result: i64 = 0;
                    for _ in 0..k.min(62) {
                        result = (result << 1) | (if simple_random() < 0.5 { 1 } else { 0 });
                    }
                    Ok(PyObject::int(result))
                }),
            ),
            (
                "getstate",
                make_builtin(|_| {
                    RNG.with(|rng| {
                        let r = rng.borrow();
                        Ok(PyObject::tuple(vec![
                            PyObject::int(r.s[0] as i64),
                            PyObject::int(r.s[1] as i64),
                            PyObject::int(r.s[2] as i64),
                            PyObject::int(r.s[3] as i64),
                        ]))
                    })
                }),
            ),
            (
                "setstate",
                make_builtin(|args| {
                    if args.is_empty() {
                        return Err(PyException::type_error("setstate() requires 1 argument"));
                    }
                    let state = &args[0];
                    if let PyObjectPayload::Tuple(items) = &state.payload {
                        if items.len() >= 4 {
                            let s0 = items[0].to_int()? as u64;
                            let s1 = items[1].to_int()? as u64;
                            let s2 = items[2].to_int()? as u64;
                            let s3 = items[3].to_int()? as u64;
                            RNG.with(|rng| {
                                let mut r = rng.borrow_mut();
                                r.s = [s0, s1, s2, s3];
                            });
                            return Ok(PyObject::none());
                        }
                    }
                    Err(PyException::type_error(
                        "state must be a 4-tuple of integers",
                    ))
                }),
            ),
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
                        make_builtin(|args| {
                            check_args("Random.uniform", args, 2)?;
                            let a = args[0].to_float()?;
                            let b = args[1].to_float()?;
                            Ok(PyObject::float(a + simple_random() * (b - a)))
                        }),
                    );
                    attrs.insert(
                        CompactString::from("sample"),
                        make_builtin(|args| {
                            check_args("Random.sample", args, 2)?;
                            let items = args[0].to_list()?;
                            let k = args[1].to_int()? as usize;
                            if k > items.len() {
                                return Err(PyException::value_error(
                                    "Sample larger than population",
                                ));
                            }
                            let mut result = Vec::with_capacity(k);
                            let mut pool = items.clone();
                            for _ in 0..k {
                                let idx = (simple_random() * pool.len() as f64) as usize;
                                let idx = idx.min(pool.len() - 1);
                                result.push(pool.remove(idx));
                            }
                            Ok(PyObject::list(result))
                        }),
                    );
                    attrs.insert(
                        CompactString::from("gauss"),
                        make_builtin(|args| {
                            check_args("Random.gauss", args, 2)?;
                            let mu = args[0].to_float()?;
                            let sigma = args[1].to_float()?;
                            let u1 = simple_random();
                            let u2 = simple_random();
                            let z =
                                (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos();
                            Ok(PyObject::float(mu + sigma * z))
                        }),
                    );
                    attrs.insert(
                        CompactString::from("getrandbits"),
                        make_builtin(|args| {
                            check_args("Random.getrandbits", args, 1)?;
                            let k = args[0].to_int()?;
                            if k <= 0 {
                                return Err(PyException::value_error(
                                    "number of bits must be greater than zero",
                                ));
                            }
                            let val = if k <= 64 {
                                (simple_random() * (1u64 << k.min(63)) as f64) as i64
                            } else {
                                (simple_random() * i64::MAX as f64) as i64
                            };
                            Ok(PyObject::int(val))
                        }),
                    );
                    attrs.insert(
                        CompactString::from("getstate"),
                        make_builtin(|_| Ok(PyObject::tuple(vec![]))),
                    );
                    attrs.insert(
                        CompactString::from("setstate"),
                        make_builtin(|_| Ok(PyObject::none())),
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
                    attrs.insert(
                        CompactString::from("randrange"),
                        make_builtin(random_randrange),
                    );
                    attrs.insert(
                        CompactString::from("getrandbits"),
                        make_builtin(|args| {
                            check_args("SystemRandom.getrandbits", args, 1)?;
                            let k = args[0].to_int()?;
                            let val = (simple_random() * (1u64 << k.min(63)) as f64) as i64;
                            Ok(PyObject::int(val))
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
