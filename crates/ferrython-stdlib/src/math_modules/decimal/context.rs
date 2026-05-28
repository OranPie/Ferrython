use compact_str::CompactString;
use ferrython_core::error::ExceptionKind;
use ferrython_core::object::{
    make_builtin, to_shared_fx, InstanceData, PyObject, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

// Signal names used by decimal module
pub(super) const SIGNAL_NAMES: &[&str] = &[
    "Clamped",
    "InvalidOperation",
    "DivisionByZero",
    "Inexact",
    "Rounded",
    "Subnormal",
    "Underflow",
    "Overflow",
    "FloatOperation",
];

pub(super) fn make_signal_types() -> Vec<(CompactString, PyObjectRef)> {
    SIGNAL_NAMES
        .iter()
        .map(|&name| {
            let kind = match name {
                "DivisionByZero" => ExceptionKind::ZeroDivisionError,
                "Overflow" => ExceptionKind::OverflowError,
                _ => ExceptionKind::ArithmeticError,
            };
            (CompactString::from(name), PyObject::exception_type(kind))
        })
        .collect()
}

pub(super) fn make_decimal_flags_dict(signals: &[(CompactString, PyObjectRef)]) -> PyObjectRef {
    let mut map = IndexMap::new();
    for (_, sig_obj) in signals {
        let key = HashableKey::from_object(sig_obj).unwrap();
        map.insert(key, PyObject::bool_val(false));
    }
    PyObject::dict(map)
}

pub(super) fn add_context_flags_and_methods(
    ctx_ns: &mut IndexMap<CompactString, PyObjectRef>,
    signals: &[(CompactString, PyObjectRef)],
) {
    ctx_ns.insert(
        CompactString::from("flags"),
        make_decimal_flags_dict(signals),
    );
    ctx_ns.insert(
        CompactString::from("traps"),
        make_decimal_flags_dict(signals),
    );
    let sigs_for_clear = signals.iter().map(|(_, o)| o.clone()).collect::<Vec<_>>();
    ctx_ns.insert(
        CompactString::from("clear_flags"),
        PyObject::native_closure("clear_flags", move |args: &[PyObjectRef]| {
            if let Some(self_obj) = args.first() {
                if let PyObjectPayload::Instance(ref inst) = self_obj.payload {
                    let mut new_flags = IndexMap::new();
                    for sig in &sigs_for_clear {
                        let key = HashableKey::from_object(sig).unwrap();
                        new_flags.insert(key, PyObject::bool_val(false));
                    }
                    inst.attrs
                        .write()
                        .insert(CompactString::from("flags"), PyObject::dict(new_flags));
                }
            }
            Ok(PyObject::none())
        }),
    );
    let sigs_for_clear2 = signals.iter().map(|(_, o)| o.clone()).collect::<Vec<_>>();
    ctx_ns.insert(
        CompactString::from("clear_traps"),
        PyObject::native_closure("clear_traps", move |args: &[PyObjectRef]| {
            if let Some(self_obj) = args.first() {
                if let PyObjectPayload::Instance(ref inst) = self_obj.payload {
                    let mut new_traps = IndexMap::new();
                    for sig in &sigs_for_clear2 {
                        let key = HashableKey::from_object(sig).unwrap();
                        new_traps.insert(key, PyObject::bool_val(false));
                    }
                    inst.attrs
                        .write()
                        .insert(CompactString::from("traps"), PyObject::dict(new_traps));
                }
            }
            Ok(PyObject::none())
        }),
    );
    ctx_ns.insert(
        CompactString::from("copy"),
        make_builtin(|args| {
            if let Some(self_obj) = args.first() {
                if let PyObjectPayload::Instance(ref inst) = self_obj.payload {
                    let attrs = inst.attrs.read().clone();
                    let new_inst = InstanceData {
                        class: inst.class.clone(),
                        attrs: to_shared_fx(attrs.into_iter().collect()),
                        is_special: true,
                        dict_storage: None,
                        class_flags: inst.class_flags,
                        finalizer_state: std::cell::Cell::new(0),
                    };
                    return Ok(PyObject::wrap(PyObjectPayload::Instance(
                        std::mem::ManuallyDrop::new(Box::new(new_inst)),
                    )));
                }
            }
            Ok(PyObject::none())
        }),
    );
}
