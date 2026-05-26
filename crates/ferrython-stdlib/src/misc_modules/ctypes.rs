use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    make_builtin, make_module, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::PyInt;
use indexmap::IndexMap;
use num_bigint::BigInt;
use num_traits::ToPrimitive;

#[cfg(target_os = "linux")]
#[inline]
unsafe fn errno_ptr() -> *mut i32 {
    libc::__errno_location()
}
#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "dragonfly",
    target_os = "openbsd",
    target_os = "netbsd"
))]
#[inline]
unsafe fn errno_ptr() -> *mut i32 {
    libc::__error()
}
#[cfg(all(
    unix,
    not(any(
        target_os = "linux",
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "openbsd",
        target_os = "netbsd"
    ))
))]
#[inline]
unsafe fn errno_ptr() -> *mut i32 {
    static mut DUMMY: i32 = 0;
    &mut DUMMY as *mut i32
}

// ── ctypes module ──

/// Call a C function at `sym_addr` with the given Python arguments.
/// Arguments are converted: int→i64, float→f64, bytes/str→pointer, None→NULL.
/// Returns i64 result as Python int (caller can set .restype to change).
fn ctypes_call_function(
    sym_addr: usize,
    fn_name: &str,
    args: &[PyObjectRef],
) -> PyResult<PyObjectRef> {
    // Convert Python args to C values
    let mut c_args: Vec<u64> = Vec::with_capacity(args.len());
    // Keep CString alive for the duration of the call
    let mut _string_keepalive: Vec<std::ffi::CString> = Vec::new();

    for (i, arg) in args.iter().enumerate() {
        match &arg.payload {
            PyObjectPayload::Int(n) => c_args.push(n.to_i64().unwrap_or(0) as u64),
            PyObjectPayload::Float(f) => c_args.push(f.to_bits()),
            PyObjectPayload::Bool(b) => c_args.push(if *b { 1 } else { 0 }),
            PyObjectPayload::Bytes(b) => {
                let cs = std::ffi::CString::new(b.as_slice())
                    .unwrap_or_else(|_| std::ffi::CString::new("").unwrap());
                c_args.push(cs.as_ptr() as u64);
                _string_keepalive.push(cs);
            }
            PyObjectPayload::Str(s) => {
                let cs = std::ffi::CString::new(s.as_str())
                    .unwrap_or_else(|_| std::ffi::CString::new("").unwrap());
                c_args.push(cs.as_ptr() as u64);
                _string_keepalive.push(cs);
            }
            PyObjectPayload::None => c_args.push(0),
            // ctypes type instances: extract .value
            PyObjectPayload::Instance(_) => {
                if let Some(val) = arg.get_attr("value") {
                    match &val.payload {
                        PyObjectPayload::Int(n) => c_args.push(n.to_i64().unwrap_or(0) as u64),
                        PyObjectPayload::Float(f) => c_args.push(f.to_bits()),
                        PyObjectPayload::Bool(b) => c_args.push(if *b { 1 } else { 0 }),
                        PyObjectPayload::Bytes(b) => {
                            let cs = std::ffi::CString::new(b.as_slice()).unwrap_or_default();
                            c_args.push(cs.as_ptr() as u64);
                            _string_keepalive.push(cs);
                        }
                        PyObjectPayload::Str(s) => {
                            let cs = std::ffi::CString::new(s.as_str()).unwrap_or_default();
                            c_args.push(cs.as_ptr() as u64);
                            _string_keepalive.push(cs);
                        }
                        _ => c_args.push(0),
                    }
                } else {
                    c_args.push(0);
                }
            }
            _ => {
                return Err(PyException::type_error(&format!(
                    "ctypes: cannot convert argument {} of type {} for {}",
                    i,
                    arg.type_name(),
                    fn_name
                )))
            }
        }
    }

    // Call the function using the system ABI (x86_64 SysV: first 6 args in registers)
    let result: i64 = unsafe {
        let fn_ptr = sym_addr as *const ();
        match c_args.len() {
            0 => {
                let f: extern "C" fn() -> i64 = std::mem::transmute(fn_ptr);
                f()
            }
            1 => {
                let f: extern "C" fn(u64) -> i64 = std::mem::transmute(fn_ptr);
                f(c_args[0])
            }
            2 => {
                let f: extern "C" fn(u64, u64) -> i64 = std::mem::transmute(fn_ptr);
                f(c_args[0], c_args[1])
            }
            3 => {
                let f: extern "C" fn(u64, u64, u64) -> i64 = std::mem::transmute(fn_ptr);
                f(c_args[0], c_args[1], c_args[2])
            }
            4 => {
                let f: extern "C" fn(u64, u64, u64, u64) -> i64 = std::mem::transmute(fn_ptr);
                f(c_args[0], c_args[1], c_args[2], c_args[3])
            }
            5 => {
                let f: extern "C" fn(u64, u64, u64, u64, u64) -> i64 = std::mem::transmute(fn_ptr);
                f(c_args[0], c_args[1], c_args[2], c_args[3], c_args[4])
            }
            6 => {
                let f: extern "C" fn(u64, u64, u64, u64, u64, u64) -> i64 =
                    std::mem::transmute(fn_ptr);
                f(
                    c_args[0], c_args[1], c_args[2], c_args[3], c_args[4], c_args[5],
                )
            }
            _ => {
                return Err(PyException::type_error(&format!(
                    "ctypes: too many arguments ({}) for {}",
                    c_args.len(),
                    fn_name
                )))
            }
        }
    };

    Ok(PyObject::int(result))
}

fn make_ctype(name: &str) -> PyObjectRef {
    // Create a callable ctypes type: c_int(42) → instance with .value attribute
    let type_name = CompactString::from(name);
    let cls = PyObject::class(type_name.clone(), vec![], IndexMap::new());
    let cls_clone = cls.clone();
    let name_owned = name.to_string();
    PyObject::native_closure(name, move |args: &[PyObjectRef]| {
        let value = if args.is_empty() {
            // Default values depend on type
            if name_owned.contains("char_p") || name_owned.contains("wchar_p") {
                PyObject::none()
            } else if name_owned.contains("double") || name_owned.contains("float") {
                PyObject::float(0.0)
            } else if name_owned.contains("bool") {
                PyObject::bool_val(false)
            } else {
                PyObject::int(0)
            }
        } else {
            normalize_ctype_value(&name_owned, &args[0])
        };
        let inst = PyObject::instance(cls_clone.clone());
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut attrs = d.attrs.write();
            attrs.insert(CompactString::from("value"), value);
            attrs.insert(
                CompactString::from("_type_"),
                PyObject::str_val(CompactString::from(name_owned.as_str())),
            );
        }
        Ok(inst)
    })
}

fn py_int_to_bigint(obj: &PyObjectRef) -> Option<BigInt> {
    match &obj.payload {
        PyObjectPayload::Bool(v) => Some(BigInt::from(if *v { 1 } else { 0 })),
        PyObjectPayload::Int(PyInt::Small(v)) => Some(BigInt::from(*v)),
        PyObjectPayload::Int(PyInt::Big(v)) => Some(v.as_ref().clone()),
        _ => None,
    }
}

fn bigint_to_pyobject(value: BigInt) -> PyObjectRef {
    value
        .to_i64()
        .map(PyObject::int)
        .unwrap_or_else(|| PyObject::big_int(value))
}

fn ctype_unsigned_bits(name: &str) -> Option<u32> {
    match name {
        "c_uint8" | "c_ubyte" => Some(8),
        "c_uint16" | "c_ushort" => Some(16),
        "c_uint32" | "c_uint" => Some(32),
        "c_uint64" | "c_ulonglong" => Some(64),
        "c_ulong" => Some((std::mem::size_of::<libc::c_ulong>() * 8) as u32),
        "c_size_t" => Some((std::mem::size_of::<usize>() * 8) as u32),
        _ => None,
    }
}

fn ctype_signed_bits(name: &str) -> Option<u32> {
    match name {
        "c_int8" | "c_byte" | "c_char" => Some(8),
        "c_int16" | "c_short" => Some(16),
        "c_int32" | "c_int" => Some(32),
        "c_int64" | "c_longlong" => Some(64),
        "c_long" => Some((std::mem::size_of::<libc::c_long>() * 8) as u32),
        "c_ssize_t" => Some((std::mem::size_of::<isize>() * 8) as u32),
        _ => None,
    }
}

fn normalize_ctype_value(type_name: &str, value: &PyObjectRef) -> PyObjectRef {
    let Some(bits) = ctype_unsigned_bits(type_name) else {
        return value.clone();
    };
    let Some(n) = py_int_to_bigint(value) else {
        return value.clone();
    };
    let modulus = BigInt::from(1u8) << bits;
    let wrapped = ((n % &modulus) + &modulus) % &modulus;
    bigint_to_pyobject(wrapped)
}

fn ctypes_instance_type_name(obj: &PyObjectRef) -> Option<String> {
    if let PyObjectPayload::Instance(inst) = &obj.payload {
        if let Some(t) = inst.attrs.read().get("_type_") {
            return Some(t.py_to_string());
        }
    }
    None
}

fn ctypes_value_obj(obj: &PyObjectRef) -> PyObjectRef {
    if let PyObjectPayload::Instance(inst) = &obj.payload {
        if let Some(value) = inst.attrs.read().get("value") {
            return value.clone();
        }
    }
    obj.clone()
}

fn ctypes_bigint_arg(obj: &PyObjectRef) -> PyResult<BigInt> {
    let value = ctypes_value_obj(obj);
    py_int_to_bigint(&value).ok_or_else(|| {
        PyException::type_error(format!(
            "integer argument expected, got {}",
            value.type_name()
        ))
    })
}

fn ctypes_i128_arg(obj: &PyObjectRef, bits_hint: Option<u32>) -> PyResult<i128> {
    let mut n = ctypes_bigint_arg(obj)?;
    if let Some(bits) = bits_hint {
        if bits > 0 {
            let sign_bit = BigInt::from(1u8) << (bits - 1);
            if n >= sign_bit {
                n -= BigInt::from(1u8) << bits;
            }
        }
    }
    n.to_i128()
        .ok_or_else(|| PyException::overflow_error("Python int too large to convert"))
}

fn ctypes_u128_arg(obj: &PyObjectRef, bits_hint: Option<u32>) -> PyResult<u128> {
    let mut n = ctypes_bigint_arg(obj)?;
    if n < BigInt::from(0u8) {
        let bits = bits_hint.unwrap_or(64);
        let modulus = BigInt::from(1u8) << bits;
        n = ((n % &modulus) + &modulus) % &modulus;
    }
    n.to_u128()
        .ok_or_else(|| PyException::overflow_error("Python int too large to convert"))
}

fn ctypes_signed_bits_for_arg(obj: &PyObjectRef, default_bits: u32) -> u32 {
    ctypes_instance_type_name(obj)
        .as_deref()
        .and_then(|name| ctype_unsigned_bits(name).or_else(|| ctype_signed_bits(name)))
        .unwrap_or(default_bits)
}

fn ctypes_bytes_arg(obj: &PyObjectRef) -> PyResult<Vec<u8>> {
    let value = ctypes_value_obj(obj);
    match &value.payload {
        PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => Ok((**b).clone()),
        PyObjectPayload::Str(s) => Ok(s.as_bytes().to_vec()),
        PyObjectPayload::None => Ok(Vec::new()),
        _ => Err(PyException::type_error(format!(
            "bytes-like argument expected, got {}",
            value.type_name()
        ))),
    }
}

fn pybytes_from_format(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "PyBytes_FromFormat requires a format string",
        ));
    }
    let format = ctypes_bytes_arg(&args[0])?;
    let mut arg_index = 1usize;
    let mut out = Vec::with_capacity(format.len());
    let mut i = 0usize;

    while i < format.len() {
        if format[i] != b'%' {
            out.push(format[i]);
            i += 1;
            continue;
        }
        i += 1;
        if i >= format.len() {
            out.push(b'%');
            break;
        }
        if format[i] == b'%' {
            out.push(b'%');
            i += 1;
            continue;
        }

        while i < format.len() && matches!(format[i], b'-' | b'+' | b' ' | b'#' | b'0') {
            i += 1;
        }
        while i < format.len() && format[i].is_ascii_digit() {
            i += 1;
        }
        let precision = if i < format.len() && format[i] == b'.' {
            i += 1;
            let start = i;
            while i < format.len() && format[i].is_ascii_digit() {
                i += 1;
            }
            std::str::from_utf8(&format[start..i])
                .ok()
                .and_then(|s| s.parse::<usize>().ok())
        } else {
            None
        };

        let length = if i < format.len() && matches!(format[i], b'l' | b'z') {
            let marker = format[i];
            i += 1;
            Some(marker)
        } else {
            None
        };
        if i >= format.len() {
            out.push(b'%');
            if let Some(marker) = length {
                out.push(marker);
            }
            break;
        }

        let conv = format[i];
        i += 1;
        let arg = if matches!(conv, b's' | b'c' | b'd' | b'i' | b'u' | b'x' | b'p') {
            if arg_index >= args.len() {
                return Err(PyException::type_error(
                    "not enough arguments for PyBytes_FromFormat",
                ));
            }
            let arg = &args[arg_index];
            arg_index += 1;
            arg
        } else {
            out.push(b'%');
            if let Some(marker) = length {
                out.push(marker);
            }
            out.push(conv);
            continue;
        };

        match conv {
            b's' => {
                let mut bytes = ctypes_bytes_arg(arg)?;
                if let Some(limit) = precision {
                    bytes.truncate(limit);
                }
                out.extend(bytes);
            }
            b'c' => {
                let value = ctypes_i128_arg(arg, Some(ctypes_signed_bits_for_arg(arg, 32)))?;
                if !(0..=255).contains(&value) {
                    return Err(PyException::overflow_error(
                        "PyBytes_FromFormat(): %c argument out of range",
                    ));
                }
                out.push(value as u8);
            }
            b'd' | b'i' => {
                let bits = match length {
                    Some(b'l') => std::mem::size_of::<libc::c_long>() as u32 * 8,
                    Some(b'z') => std::mem::size_of::<isize>() as u32 * 8,
                    _ => ctypes_signed_bits_for_arg(arg, 32),
                };
                let value = ctypes_i128_arg(arg, Some(bits))?;
                out.extend(value.to_string().as_bytes());
            }
            b'u' => {
                let bits = match length {
                    Some(b'l') => std::mem::size_of::<libc::c_ulong>() as u32 * 8,
                    Some(b'z') => std::mem::size_of::<usize>() as u32 * 8,
                    _ => ctypes_instance_type_name(arg)
                        .as_deref()
                        .and_then(ctype_unsigned_bits)
                        .unwrap_or(32),
                };
                let value = ctypes_u128_arg(arg, Some(bits))?;
                out.extend(value.to_string().as_bytes());
            }
            b'x' => {
                let bits = ctypes_instance_type_name(arg)
                    .as_deref()
                    .and_then(ctype_unsigned_bits)
                    .unwrap_or(32);
                let value = ctypes_u128_arg(arg, Some(bits))?;
                out.extend(format!("{:x}", value).as_bytes());
            }
            b'p' => {
                let bits = std::mem::size_of::<usize>() as u32 * 8;
                let value = ctypes_u128_arg(arg, Some(bits))?;
                out.extend(format!("{:#x}", value).as_bytes());
            }
            _ => unreachable!(),
        }
    }

    Ok(PyObject::bytes(out))
}

/// Return the standard byte size for a ctypes type name
fn ctype_sizeof(name: &str) -> i64 {
    match name {
        n if n.contains("int8")
            || n.contains("byte")
            || n.contains("char") && !n.contains("char_p")
            || n.contains("bool") =>
        {
            1
        }
        n if n.contains("int16") || n.contains("short") => 2,
        n if n.contains("int32")
            || n == "c_int"
            || n == "c_uint"
            || n.contains("float") && !n.contains("double") =>
        {
            4
        }
        n if n.contains("int64")
            || n.contains("long")
            || n.contains("double")
            || n.contains("size_t")
            || n.contains("ssize_t") =>
        {
            8
        }
        n if n.contains("_p") || n.contains("void_p") => std::mem::size_of::<usize>() as i64,
        _ => 8,
    }
}

pub fn create_ctypes_module() -> PyObjectRef {
    // ctypes stub — provides type definitions with .value support
    // so that programs that import ctypes get basic functionality.

    let c_int = make_ctype("c_int");
    let c_long = make_ctype("c_long");
    let c_char = make_ctype("c_char");
    let c_char_p = make_ctype("c_char_p");
    let c_wchar_p = make_ctype("c_wchar_p");
    let c_void_p = make_ctype("c_void_p");
    let c_double = make_ctype("c_double");
    let c_float = make_ctype("c_float");
    let c_uint = make_ctype("c_uint");
    let c_ulong = make_ctype("c_ulong");
    let c_short = make_ctype("c_short");
    let c_ushort = make_ctype("c_ushort");
    let c_byte = make_ctype("c_byte");
    let c_ubyte = make_ctype("c_ubyte");
    let c_bool = make_ctype("c_bool");
    let c_longlong = make_ctype("c_longlong");
    let c_ulonglong = make_ctype("c_ulonglong");
    let c_size_t = make_ctype("c_size_t");
    let c_ssize_t = make_ctype("c_ssize_t");
    let py_object = make_ctype("py_object");

    let pythonapi = {
        let mut attrs = IndexMap::new();
        attrs.insert(
            CompactString::from("PyBytes_FromFormat"),
            PyObject::native_function("PyBytes_FromFormat", pybytes_from_format),
        );
        PyObject::module_with_attrs(CompactString::from("ctypes.pythonapi"), attrs)
    };

    let structure_cls = {
        let mut ns = IndexMap::new();
        // _fields_ is typically set by subclasses, but provide a default empty list
        ns.insert(CompactString::from("_fields_"), PyObject::list(vec![]));
        ns.insert(CompactString::from("_pack_"), PyObject::int(0));
        PyObject::class(CompactString::from("Structure"), vec![], ns)
    };
    let union_cls = {
        let mut ns = IndexMap::new();
        ns.insert(CompactString::from("_fields_"), PyObject::list(vec![]));
        PyObject::class(CompactString::from("Union"), vec![], ns)
    };
    let array_cls = PyObject::class(CompactString::from("Array"), vec![], IndexMap::new());

    // CDLL — real dlopen/dlsym based foreign function interface
    let cdll_fn = make_builtin(|args: &[PyObjectRef]| {
        let name = args.first().map(|a| a.py_to_string()).unwrap_or_default();

        // dlopen the library
        let c_name = std::ffi::CString::new(name.as_str())
            .map_err(|_| PyException::os_error(&format!("invalid library name: {}", name)))?;
        let handle = unsafe { libc::dlopen(c_name.as_ptr(), libc::RTLD_LAZY | libc::RTLD_GLOBAL) };
        if handle.is_null() {
            let err = unsafe { libc::dlerror() };
            let msg = if err.is_null() {
                format!("cannot load library '{}'", name)
            } else {
                unsafe { std::ffi::CStr::from_ptr(err).to_string_lossy().into_owned() }
            };
            return Err(PyException::os_error(&msg));
        }
        let handle_val = handle as usize;

        let cls = PyObject::class(CompactString::from("CDLL"), vec![], IndexMap::new());
        let mut cls_ns = IndexMap::new();

        // __getattr__ for function lookup via dlsym
        let lib_name = name.clone();
        cls_ns.insert(
            CompactString::from("__getattr__"),
            PyObject::native_closure("__getattr__", move |args: &[PyObjectRef]| {
                let attr_name = if args.len() > 1 {
                    args[1].py_to_string()
                } else if !args.is_empty() {
                    args[0].py_to_string()
                } else {
                    return Err(PyException::type_error("__getattr__ requires a name"));
                };
                // Look up symbol via dlsym
                let c_sym = std::ffi::CString::new(attr_name.as_str()).map_err(|_| {
                    PyException::attribute_error(&format!("invalid symbol name: {}", attr_name))
                })?;
                let sym = unsafe { libc::dlsym(handle_val as *mut libc::c_void, c_sym.as_ptr()) };
                if sym.is_null() {
                    return Err(PyException::attribute_error(&format!(
                        "undefined symbol: {}",
                        attr_name
                    )));
                }
                let sym_addr = sym as usize;
                let fn_name = attr_name.clone();

                // Return a callable that invokes the C function
                // Supports calling conventions: integers, pointers (bytes/str → c_char_p)
                Ok(PyObject::native_closure(
                    &format!("{}.{}", lib_name, attr_name),
                    move |call_args: &[PyObjectRef]| {
                        ctypes_call_function(sym_addr, &fn_name, call_args)
                    },
                ))
            }),
        );

        let inst = PyObject::instance_with_attrs(cls, cls_ns);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut attrs = d.attrs.write();
            attrs.insert(
                CompactString::from("_name"),
                PyObject::str_val(CompactString::from(name.as_str())),
            );
            attrs.insert(
                CompactString::from("_handle"),
                PyObject::int(handle_val as i64),
            );
        }
        Ok(inst)
    });

    // POINTER(type) → returns a new pointer type callable
    let pointer_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error("POINTER requires a type"));
        }
        let base_type = args[0].clone();
        let type_name = format!("LP_{}", base_type.py_to_string());
        let ptr_cls = PyObject::class(
            CompactString::from(type_name.as_str()),
            vec![],
            IndexMap::new(),
        );
        let ptr_cls_clone = ptr_cls.clone();
        Ok(PyObject::native_closure(
            &type_name,
            move |args: &[PyObjectRef]| {
                let inst = PyObject::instance(ptr_cls_clone.clone());
                if let PyObjectPayload::Instance(ref d) = inst.payload {
                    let mut attrs = d.attrs.write();
                    attrs.insert(CompactString::from("_type_"), base_type.clone());
                    attrs.insert(
                        CompactString::from("contents"),
                        if args.is_empty() {
                            PyObject::none()
                        } else {
                            args[0].clone()
                        },
                    );
                }
                Ok(inst)
            },
        ))
    });

    // byref(obj, offset=0) → reference wrapper with ._obj
    let byref_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error("byref requires an argument"));
        }
        let cls = PyObject::class(CompactString::from("CArgObject"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut attrs = d.attrs.write();
            attrs.insert(CompactString::from("_obj"), args[0].clone());
            attrs.insert(CompactString::from("value"), args[0].clone());
            let offset = if args.len() > 1 {
                args[1].as_int().unwrap_or(0)
            } else {
                0
            };
            attrs.insert(CompactString::from("_offset"), PyObject::int(offset));
        }
        Ok(inst)
    });

    // sizeof(type_or_instance)
    let sizeof_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error("sizeof requires an argument"));
        }
        // Try to get type name from _type_ attr or class name
        let type_name = if let Some(t) = args[0].get_attr("_type_") {
            t.py_to_string()
        } else {
            args[0].py_to_string()
        };
        Ok(PyObject::int(ctype_sizeof(&type_name)))
    });

    make_module(
        "ctypes",
        vec![
            ("c_int", c_int),
            ("c_long", c_long),
            ("c_char", c_char),
            ("c_char_p", c_char_p),
            ("c_wchar_p", c_wchar_p),
            ("c_void_p", c_void_p),
            ("c_double", c_double),
            ("c_float", c_float),
            ("c_uint", c_uint),
            ("c_ulong", c_ulong),
            ("c_short", c_short),
            ("c_ushort", c_ushort),
            ("c_byte", c_byte),
            ("c_ubyte", c_ubyte),
            ("c_bool", c_bool),
            ("c_longlong", c_longlong),
            ("c_ulonglong", c_ulonglong),
            ("c_size_t", c_size_t),
            ("c_ssize_t", c_ssize_t),
            ("py_object", py_object),
            ("pythonapi", pythonapi),
            ("c_int8", make_ctype("c_int8")),
            ("c_int16", make_ctype("c_int16")),
            ("c_int32", make_ctype("c_int32")),
            ("c_int64", make_ctype("c_int64")),
            ("c_uint8", make_ctype("c_uint8")),
            ("c_uint16", make_ctype("c_uint16")),
            ("c_uint32", make_ctype("c_uint32")),
            ("c_uint64", make_ctype("c_uint64")),
            ("Structure", structure_cls),
            ("Union", union_cls),
            ("Array", array_cls),
            ("CDLL", cdll_fn.clone()),
            ("cdll", cdll_fn.clone()),
            ("windll", cdll_fn.clone()),
            ("oledll", cdll_fn),
            ("POINTER", pointer_fn.clone()),
            ("pointer", pointer_fn),
            (
                "cast",
                make_builtin(|args| {
                    if args.len() < 2 {
                        return Err(PyException::type_error("cast requires obj and type"));
                    }
                    // Return a new instance of the target type wrapping the source value
                    let source = &args[0];
                    let target_type = &args[1];
                    match &target_type.payload {
                        PyObjectPayload::NativeFunction(nf) => (nf.func)(&[source.clone()]),
                        PyObjectPayload::NativeClosure(nc) => (nc.func)(&[source.clone()]),
                        _ => Ok(source.clone()),
                    }
                }),
            ),
            ("byref", byref_fn),
            (
                "addressof",
                make_builtin(|args| {
                    if args.is_empty() {
                        return Err(PyException::type_error("addressof requires an argument"));
                    }
                    // Return a fake address based on the Arc pointer
                    let ptr = PyObjectRef::as_ptr(&args[0]) as usize;
                    Ok(PyObject::int(ptr as i64))
                }),
            ),
            ("sizeof", sizeof_fn),
            (
                "create_string_buffer",
                make_builtin(|args| {
                    let size = args.first().and_then(|a| a.as_int()).unwrap_or(256) as usize;
                    Ok(PyObject::bytes(vec![0u8; size]))
                }),
            ),
            (
                "create_unicode_buffer",
                make_builtin(|args| {
                    let size = args.first().and_then(|a| a.as_int()).unwrap_or(256) as usize;
                    Ok(PyObject::str_val(CompactString::from("\0".repeat(size))))
                }),
            ),
            (
                "get_errno",
                make_builtin(|_| {
                    #[cfg(unix)]
                    {
                        let e = unsafe { *errno_ptr() };
                        Ok(PyObject::int(e as i64))
                    }
                    #[cfg(not(unix))]
                    Err(PyException::os_error(
                        "get_errno() is not supported on this platform",
                    ))
                }),
            ),
            (
                "set_errno",
                make_builtin(|args| {
                    let new_val = args.first().and_then(|a| a.as_int()).unwrap_or(0);
                    #[cfg(unix)]
                    {
                        let old = unsafe { *errno_ptr() };
                        unsafe {
                            *errno_ptr() = new_val as i32;
                        }
                        Ok(PyObject::int(old as i64))
                    }
                    #[cfg(not(unix))]
                    {
                        let _ = new_val;
                        Err(PyException::os_error(
                            "set_errno() is not supported on this platform",
                        ))
                    }
                }),
            ),
            (
                "get_last_error",
                make_builtin(|_| {
                    Err(PyException::os_error(
                        "get_last_error() is not supported on this platform",
                    ))
                }),
            ),
            (
                "set_last_error",
                make_builtin(|_| {
                    Err(PyException::os_error(
                        "set_last_error() is not supported on this platform",
                    ))
                }),
            ),
            ("util", {
                // ctypes.util.find_library
                let mut util_attrs = IndexMap::new();
                util_attrs.insert(
                    CompactString::from("find_library"),
                    make_builtin(|args| {
                        let name = args.first().map(|a| a.py_to_string()).unwrap_or_default();
                        // Try common library paths
                        let candidates = vec![
                            format!("lib{}.so", name),
                            format!("lib{}.dylib", name),
                            format!("{}.dll", name),
                        ];
                        for candidate in &candidates {
                            if std::path::Path::new(candidate).exists() {
                                return Ok(PyObject::str_val(CompactString::from(
                                    candidate.as_str(),
                                )));
                            }
                            let path = format!("/usr/lib/{}", candidate);
                            if std::path::Path::new(&path).exists() {
                                return Ok(PyObject::str_val(CompactString::from(path)));
                            }
                            let path2 = format!("/usr/lib/x86_64-linux-gnu/{}", candidate);
                            if std::path::Path::new(&path2).exists() {
                                return Ok(PyObject::str_val(CompactString::from(path2)));
                            }
                        }
                        Ok(PyObject::none())
                    }),
                );
                PyObject::module_with_attrs(CompactString::from("ctypes.util"), util_attrs)
            }),
        ],
    )
}
