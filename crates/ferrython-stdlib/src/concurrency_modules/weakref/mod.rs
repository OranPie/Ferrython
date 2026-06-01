use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    call_callable, call_callable_kw, make_builtin, make_module, CompareOp, FxAttrMap, FxHashKeyMap,
    PyCell, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef, PyWeakRef, SyncUsize,
    VecIterData, WeakKeyIterData, WeakKeyIterKind, WeakObjectKind, WeakValueIterData,
    WeakValueIterKind,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::cell::Cell;
use std::rc::Rc;

mod finalize;
mod mappings;
mod reference;

type WeakKeyStorage = Rc<PyCell<IndexMap<usize, (PyObjectRef, PyObjectRef)>>>;
type WeakValueStorage = Rc<PyCell<IndexMap<HashableKey, (PyObjectRef, PyObjectRef)>>>;

fn upgrade_or_none(weak: &PyWeakRef) -> PyObjectRef {
    match weak.upgrade() {
        Some(obj) => obj,
        None => PyObject::none(),
    }
}

fn upgrade_or_err(weak: &PyWeakRef) -> Result<PyObjectRef, PyException> {
    weak.upgrade().ok_or_else(|| {
        PyException::new(
            ExceptionKind::ReferenceError,
            "weakly-referenced object no longer exists",
        )
    })
}

fn weak_ref_target(ref_obj: &PyObjectRef) -> Option<PyObjectRef> {
    let PyObjectPayload::Instance(inst) = &ref_obj.payload else {
        return None;
    };
    let target_fn = inst.attrs.read().get("__weakref_target__").cloned()?;
    call_callable(&target_fn, &[]).ok().and_then(|obj| {
        if matches!(&obj.payload, PyObjectPayload::None) {
            None
        } else {
            Some(obj)
        }
    })
}

pub fn create_weakref_module() -> PyObjectRef {
    let mut reference_type_namespace = IndexMap::new();
    reference_type_namespace.insert(
        CompactString::from("__slots__"),
        PyObject::tuple(Vec::new()),
    );
    let reference_type = PyObject::class(
        CompactString::from("weakref"),
        vec![],
        reference_type_namespace,
    );
    let proxy_type = PyObject::class(CompactString::from("weakproxy"), vec![], IndexMap::new());
    let callable_proxy_type = PyObject::class(
        CompactString::from("weakcallableproxy"),
        vec![],
        IndexMap::new(),
    );

    reference::configure_reference_type(&reference_type);
    let proxy_fn = reference::make_proxy_fn(&proxy_type, &callable_proxy_type);
    let weak_method_fn = reference::make_weak_method_fn(&reference_type);
    let finalize_type = finalize::create_finalize_type(&reference_type);

    make_module(
        "weakref",
        vec![
            // ── ref(obj, callback=None) ──
            // Returns a callable weak reference. Calling it returns the referent or None.
            ("ref", reference_type.clone()),
            // ── proxy(obj, callback=None) ──
            // Returns a proxy that auto-dereferences on attribute access.
            ("proxy", proxy_fn),
            // ── WeakValueDictionary() ──
            // Dict where values are weak references; dead entries are auto-pruned.
            (
                "WeakValueDictionary",
                PyObject::native_function(
                    "WeakValueDictionary",
                    mappings::make_weak_value_dictionary,
                ),
            ),
            // ── WeakKeyDictionary() ──
            // Dict where keys are weak references; dead entries are auto-pruned.
            (
                "WeakKeyDictionary",
                PyObject::native_function("WeakKeyDictionary", mappings::make_weak_key_dictionary),
            ),
            // ── WeakSet() ──
            // A set of weak references. Dead entries are auto-pruned.
            ("WeakSet", mappings::make_weak_set_class()),
            // ── finalize(obj, func, *args, **kwargs) ──
            ("finalize", finalize_type),
            // ── getweakrefcount(obj) ──
            (
                "getweakrefcount",
                make_builtin(|args| {
                    if args.is_empty() {
                        return Err(PyException::type_error(
                            "getweakrefcount requires 1 argument",
                        ));
                    }
                    Ok(PyObject::int(PyObjectRef::weak_count(&args[0]) as i64))
                }),
            ),
            // ── getweakrefs(obj) ──
            (
                "getweakrefs",
                make_builtin(|args| {
                    if args.is_empty() {
                        return Err(PyException::type_error("getweakrefs requires 1 argument"));
                    }
                    let mut refs = PyObjectRef::weak_objects(&args[0]);
                    refs.sort_by_key(|obj| {
                        if let PyObjectPayload::Instance(inst) = &obj.payload {
                            if let PyObjectPayload::Class(cd) = &inst.class.payload {
                                return if cd.name.as_str() == "weakref" { 0 } else { 1 };
                            }
                        }
                        1
                    });
                    Ok(PyObject::list(refs))
                }),
            ),
            // ── ReferenceType (the type of weak references) ──
            ("ReferenceType", reference_type.clone()),
            // ── ProxyType ──
            ("ProxyType", proxy_type),
            // ── CallableProxyType ──
            ("CallableProxyType", callable_proxy_type),
            // ── WeakMethod(method, callback=None) ──
            ("WeakMethod", weak_method_fn),
        ],
    )
}
