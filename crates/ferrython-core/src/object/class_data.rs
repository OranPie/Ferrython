use compact_str::CompactString;
use rustc_hash::FxHashMap;
use std::cell::Cell;
use std::rc::Rc;

use super::{
    FxAttrMap, PyCell, PyObjectMethods, PyObjectPayload, PyObjectRef, PyWeakRef,
    CLASS_FLAG_HAS_DESCRIPTORS, CLASS_FLAG_HAS_GETATTR, CLASS_FLAG_HAS_GETATTRIBUTE,
    CLASS_FLAG_HAS_SETATTR, CLASS_FLAG_HAS_SLOTS,
};

// Thread-local monotonic counter for class versioning. Incremented each time a
// ClassData is created or mutated. Used by inline caches to detect staleness.
thread_local! {
    static CLASS_VERSION_COUNTER: Cell<u64> = const { Cell::new(1) };
}

/// Allocate a fresh class version number.
#[inline(always)]
pub fn next_class_version() -> u64 {
    CLASS_VERSION_COUNTER.with(|c| {
        let v = c.get();
        c.set(v.wrapping_add(1));
        v
    })
}

#[derive(Debug, Clone)]
pub struct ClassData {
    pub name: CompactString,
    pub bases: Vec<PyObjectRef>,
    pub namespace: Rc<PyCell<FxAttrMap>>,
    pub mro: Vec<PyObjectRef>,
    /// Custom metaclass, if any (e.g., SingletonMeta). None = default `type`.
    pub metaclass: Option<PyObjectRef>,
    /// Per-class method resolution cache: avoids repeated MRO scans for the same attr name.
    /// Cleared on any namespace mutation (class attr assignment).
    /// Uses FxHashMap for faster hashing (no insertion-order needed).
    pub method_cache: Rc<PyCell<FxHashMap<CompactString, Option<PyObjectRef>>>>,
    /// Fast-path flag: true if this class or any base defines Property, __set__, or __delete__.
    /// When false, instance attr lookup can skip the descriptor protocol entirely.
    pub has_descriptors: bool,
    /// Weak references to direct subclasses (for type.__subclasses__()).
    pub subclasses: Rc<PyCell<Vec<PyWeakRef>>>,
    /// `__slots__` declared on *this* class (None means no __slots__ declared).
    pub slots: Option<Vec<CompactString>>,
    /// Fast-path flag: true if this class (or any base) defines a custom __getattribute__.
    /// When false, the VM skips the expensive MRO lookup on every LoadAttr.
    pub has_getattribute: bool,
    /// Fast-path flag: true if this class (or any base) defines a custom __setattr__.
    /// When false, StoreAttr can write directly to instance attrs dict.
    pub has_setattr: bool,
    /// Pre-computed method vtable: flattened MRO methods for O(1) lookup.
    /// Built at class creation time from own namespace + all bases in MRO order.
    /// Cleared on namespace mutation alongside method_cache.
    pub method_vtable: Rc<PyCell<FxHashMap<CompactString, PyObjectRef>>>,
    /// Instance attribute shape: maps attr name → dense index for O(1) attr access.
    /// Built from __init__ analysis or __slots__. Instances store values in a Vec
    /// indexed by these offsets. Attrs not in the shape fall back to overflow dict.
    pub attr_shape: Rc<FxHashMap<CompactString, usize>>,
    /// Monotonic version counter — incremented on any class mutation to invalidate
    /// inline caches and method vtable.
    pub class_version: u64,
    /// Cached flag: true if this class inherits from `dict`.
    /// Pre-computed at class creation to avoid walking the hierarchy per instance.
    pub is_dict_subclass: bool,
    /// Number of expected instance attrs (from attr_shape).
    /// Used to pre-allocate IndexMap capacity in instance creation.
    pub expected_attrs: usize,
    /// Fast-path flag: true if this class can be instantiated without checking
    /// enum, abstract methods, custom __new__, or dataclass markers.
    /// Computed at class creation time; invalidated on class mutation.
    pub is_simple_class: Cell<bool>,
    /// Cached flag: true if this class inherits from an ExceptionType.
    /// Pre-computed at class creation to avoid recursive base walk per instantiation.
    pub is_exception_subclass: bool,
    /// Fast-path flag: true if this class or any base defines __getattr__.
    /// When false, negative attribute lookups can skip the __getattr__ MRO scan.
    pub has_getattr: bool,
    /// Cached InstanceData flags (has_getattribute, has_descriptors, etc.)
    /// Pre-computed to avoid recomputing per instance creation.
    pub instance_flags: u8,
    /// Cached __init__ function for fast instantiation (avoids vtable/namespace
    /// lookup per call). Populated lazily on first instantiation. Cleared on class mutation.
    pub cached_init: PyCell<Option<PyObjectRef>>,
    /// Cached inline __init__ slots: `Some(slots)` = inlinable (each slot is
    /// `(arg_local_index, name_index)` for LOAD_FAST+STORE_ATTR pairs).
    /// `None` = not inlinable. Outer Option: not yet analyzed.
    pub cached_init_inline: PyCell<Option<Option<Vec<(usize, usize)>>>>,
    /// Cached flag: true if __new__ is defined in this class's namespace.
    /// Pre-computed at class creation, invalidated on mutation.
    pub has_custom_new: Cell<bool>,
    /// Cached builtin base type name (e.g. "tuple", "list", "int") if this class
    /// inherits from a builtin type. None if no builtin type in MRO.
    /// Used by fast instantiation paths to store __builtin_value__.
    pub builtin_base_name: Option<CompactString>,
}

impl ClassData {
    pub fn new(
        name: CompactString,
        bases: Vec<PyObjectRef>,
        namespace: FxAttrMap,
        mro: Vec<PyObjectRef>,
        metaclass: Option<PyObjectRef>,
    ) -> Self {
        // Extract __slots__ from the namespace if present
        let slots: Option<Vec<CompactString>> = namespace.get("__slots__").and_then(|s| {
            match &s.payload {
                PyObjectPayload::List(items) => {
                    let items = items.read();
                    Some(
                        items
                            .iter()
                            .map(|item: &PyObjectRef| CompactString::from(item.py_to_string()))
                            .collect::<Vec<_>>(),
                    )
                }
                PyObjectPayload::Tuple(items) => Some(
                    items
                        .iter()
                        .map(|item: &PyObjectRef| CompactString::from(item.py_to_string()))
                        .collect::<Vec<_>>(),
                ),
                PyObjectPayload::Str(s) => {
                    // Single string slot: __slots__ = "x"
                    Some(vec![s.to_compact_string()])
                }
                _ => None,
            }
        });
        // Detect __getattribute__ override in namespace or any base class
        let has_getattribute = namespace.contains_key("__getattribute__")
            || mro.iter().any(|base| {
                if let PyObjectPayload::Class(bcd) = &base.payload {
                    bcd.namespace.read().contains_key("__getattribute__")
                } else {
                    false
                }
            });
        // Detect __getattr__ fallback in namespace or any base class
        let has_getattr = namespace.contains_key("__getattr__")
            || mro.iter().any(|base| {
                if let PyObjectPayload::Class(bcd) = &base.payload {
                    bcd.namespace.read().contains_key("__getattr__")
                } else {
                    false
                }
            });
        // Detect __setattr__ override in namespace or any base class
        let has_setattr = namespace.contains_key("__setattr__")
            || mro.iter().any(|base| {
                if let PyObjectPayload::Class(bcd) = &base.payload {
                    bcd.namespace.read().contains_key("__setattr__")
                } else {
                    false
                }
            });
        // Detect data descriptors (Property, __set__, __delete__) in this class or bases
        let has_descriptors = Self::detect_descriptors(&namespace, &mro);
        // If MRO is empty but we have bases, build a simple linearization
        let mro = if mro.is_empty() && !bases.is_empty() {
            let mut result = Vec::new();
            for base in &bases {
                if !result
                    .iter()
                    .any(|r: &PyObjectRef| PyObjectRef::ptr_eq(r, base))
                {
                    result.push(base.clone());
                }
                if let PyObjectPayload::Class(cd) = &base.payload {
                    for m in &cd.mro {
                        if !result
                            .iter()
                            .any(|r: &PyObjectRef| PyObjectRef::ptr_eq(r, m))
                        {
                            result.push(m.clone());
                        }
                    }
                }
            }
            result
        } else {
            mro
        };
        // Build method vtable by flattening MRO methods
        let mut vtable = FxHashMap::default();
        for base in mro.iter().rev() {
            if let PyObjectPayload::Class(bcd) = &base.payload {
                for (k, v) in bcd.namespace.read().iter() {
                    vtable.insert(k.clone(), v.clone());
                }
            }
        }
        for (k, v) in namespace.iter() {
            vtable.insert(k.clone(), v.clone());
        }

        // Build attribute shape from __slots__ and __init__ StoreAttr targets
        let mut attr_shape = FxHashMap::default();
        if let Some(ref s) = slots {
            for (i, name) in s.iter().enumerate() {
                attr_shape.insert(name.clone(), i);
            }
        }
        if let Some(init_fn) = namespace.get("__init__") {
            if let PyObjectPayload::Function(ref pf) = init_fn.payload {
                use ferrython_bytecode::Opcode;
                for instr in &pf.code.instructions {
                    if instr.op == Opcode::StoreAttr {
                        let name_idx = instr.arg as usize;
                        if name_idx < pf.code.names.len() {
                            let attr_name = &pf.code.names[name_idx];
                            if !attr_shape.contains_key(attr_name.as_str()) {
                                let idx = attr_shape.len();
                                attr_shape.insert(attr_name.clone(), idx);
                            }
                        }
                    }
                }
            }
        }

        // Detect dict subclass (cache once instead of per-instance traversal)
        let is_dict_subclass = Self::check_dict_subclass(&bases);

        // Detect builtin base type (tuple, list, int, etc.) for __builtin_value__ storage
        let builtin_base_name = super::helpers::get_builtin_base_type_name_from_bases(&bases);

        let expected_attrs = attr_shape.len();

        // A class is "simple" if instantiation needs no special dispatch:
        // no enum, no abstract methods (own or inherited), no custom __new__,
        // no __dataclass__, and no metaclass __call__.
        let is_abstract_marker = |val: &PyObjectRef| -> bool {
            if let PyObjectPayload::Tuple(items) = &val.payload {
                items.len() == 2 && items[0].as_str() == Some("__abstract__")
            } else if let PyObjectPayload::Property(pd) = &val.payload {
                if let Some(fg) = &pd.fget {
                    if let PyObjectPayload::Tuple(items) = &fg.payload {
                        return items.len() == 2 && items[0].as_str() == Some("__abstract__");
                    }
                }
                false
            } else {
                false
            }
        };
        let has_abstractmethods_marker = |val: &PyObjectRef| -> bool {
            match &val.payload {
                PyObjectPayload::Set(set) => !set.read().is_empty(),
                PyObjectPayload::FrozenSet(set) => !set.is_empty(),
                PyObjectPayload::Tuple(items) => !items.is_empty(),
                PyObjectPayload::List(items) => !items.read().is_empty(),
                _ => false,
            }
        };
        let is_callable_abstract = |val: &PyObjectRef| -> bool {
            val.get_attr("__isabstractmethod__")
                .map(|flag| flag.is_truthy())
                .unwrap_or(false)
        };
        let has_own_abstract = namespace
            .values()
            .any(|val| is_abstract_marker(val) || is_callable_abstract(val));
        // Simpler: check if any MRO base has unoverridden abstract methods
        let has_abstract = has_own_abstract || {
            let mut found = false;
            for base in &mro {
                if let PyObjectPayload::Class(bcd) = &base.payload {
                    let bns = bcd.namespace.read();
                    if let Some(abs_methods) = bns.get("__abstractmethods__") {
                        if has_abstractmethods_marker(abs_methods) {
                            found = true;
                            break;
                        }
                    }
                    for (name, val) in bns.iter() {
                        if (is_abstract_marker(val) || is_callable_abstract(val))
                            && !namespace.contains_key(name.as_str())
                        {
                            found = true;
                            break;
                        }
                    }
                    if found {
                        break;
                    }
                }
            }
            found
        };
        let inherits_custom_new = bases.iter().any(|base| {
            if let PyObjectPayload::Class(cd) = &base.payload {
                cd.has_custom_new.get()
            } else {
                false
            }
        });
        let is_simple_class = metaclass.is_none()
            && !has_abstract
            && !namespace.contains_key("__enum__")
            && !namespace.contains_key("__dataclass__")
            && !namespace.contains_key("__new__")
            && !inherits_custom_new
            && !namespace.contains_key("__namedtuple__");

        // Pre-compute exception subclass flag (avoids recursive base walk per instantiation)
        let is_exception_subclass = bases.iter().any(|base| {
            fn check_exc(obj: &PyObjectRef) -> bool {
                if matches!(&obj.payload, PyObjectPayload::ExceptionType(_)) {
                    return true;
                }
                if let PyObjectPayload::Class(cd) = &obj.payload {
                    cd.is_exception_subclass
                } else {
                    false
                }
            }
            check_exc(base)
        });

        // Pre-compute instance flags
        let mut instance_flags = 0u8;
        if has_getattribute {
            instance_flags |= CLASS_FLAG_HAS_GETATTRIBUTE;
        }
        if has_descriptors {
            instance_flags |= CLASS_FLAG_HAS_DESCRIPTORS;
        }
        if has_setattr {
            instance_flags |= CLASS_FLAG_HAS_SETATTR;
        }
        if slots.is_some() {
            instance_flags |= CLASS_FLAG_HAS_SLOTS;
        }
        if has_getattr {
            instance_flags |= CLASS_FLAG_HAS_GETATTR;
        }

        let has_custom_new = namespace.contains_key("__new__") || inherits_custom_new;

        Self {
            name,
            bases,
            namespace: Rc::new(PyCell::new(namespace)),
            mro,
            metaclass,
            method_cache: Rc::new(PyCell::new(FxHashMap::default())),
            subclasses: Rc::new(PyCell::new(Vec::new())),
            slots,
            has_getattribute,
            has_getattr,
            has_setattr,
            has_descriptors,
            method_vtable: Rc::new(PyCell::new(vtable)),
            attr_shape: Rc::new(attr_shape),
            class_version: next_class_version(),
            is_dict_subclass,
            expected_attrs,
            is_simple_class: Cell::new(is_simple_class),
            is_exception_subclass,
            instance_flags,
            cached_init: PyCell::new(None),
            cached_init_inline: PyCell::new(None),
            has_custom_new: Cell::new(has_custom_new),
            builtin_base_name,
        }
    }

    /// Rebuild method vtable after a class mutation. Call after modifying the namespace.
    pub fn rebuild_vtable(&mut self) {
        let mut vtable = FxHashMap::default();
        for base in self.mro.iter().rev() {
            if let PyObjectPayload::Class(bcd) = &base.payload {
                for (k, v) in bcd.namespace.read().iter() {
                    vtable.insert(k.clone(), v.clone());
                }
            }
        }
        for (k, v) in self.namespace.read().iter() {
            vtable.insert(k.clone(), v.clone());
        }
        self.method_vtable = Rc::new(PyCell::new(vtable));
        self.class_version = next_class_version();
        // Invalidate cached __init__ and __new__ flags
        *self.cached_init.write() = None;
        *self.cached_init_inline.write() = None;
        self.has_custom_new
            .set(self.namespace.read().contains_key("__new__"));
    }

    /// Collect all allowed slot names from this class and its MRO.
    /// Returns `None` if no class in the hierarchy defines `__slots__`.
    pub fn collect_all_slots(&self) -> Option<Vec<CompactString>> {
        let mut all_slots: Vec<CompactString> = Vec::new();
        let mut found_any = false;

        // CPython rule: if ANY class in the MRO lacks __slots__, instances
        // get __dict__ and arbitrary attribute access is allowed.
        // Check that the class itself AND every base in MRO define __slots__.
        let mut all_have_slots = self.slots.is_some();

        if let Some(ref s) = self.slots {
            found_any = true;
            for name in s {
                if !all_slots.contains(name) {
                    all_slots.push(name.clone());
                }
            }
        }
        for cls in &self.mro {
            if let PyObjectPayload::Class(cd) = &cls.payload {
                if let Some(ref s) = cd.slots {
                    found_any = true;
                    for name in s {
                        if !all_slots.contains(name) {
                            all_slots.push(name.clone());
                        }
                    }
                } else {
                    all_have_slots = false;
                }
            } else if let PyObjectPayload::BuiltinType(n) = &cls.payload {
                // object has no __slots__ → allows __dict__
                if n.as_str() == "object" {
                    // object is special: it doesn't restrict __dict__
                    // (only restrict if ALL user classes in MRO have __slots__)
                }
            }
        }

        // If any non-object class in MRO lacks __slots__, allow __dict__
        if !all_have_slots {
            return None;
        }
        if found_any {
            Some(all_slots)
        } else {
            None
        }
    }

    /// Whether `__dict__` is allowed on instances of this class.
    pub fn has_dict_slot(&self) -> bool {
        if let Some(ref slots) = self.collect_all_slots() {
            slots.iter().any(|s| s.as_str() == "__dict__")
        } else {
            true // no __slots__ → __dict__ is always available
        }
    }

    /// Invalidate the method cache and vtable (call after any namespace mutation).
    pub fn invalidate_cache(&self) {
        self.method_cache.write().clear();
        self.method_vtable.write().clear();
        // Class mutation may have added __new__/__enum__/__namedtuple__ or abstract methods,
        // so conservatively disable the simple-class fast path.
        self.is_simple_class.set(false);
    }

    /// Detect if this class or any base has data descriptors (Property, __set__, __delete__).
    /// When false, instance attribute lookup can skip the full descriptor protocol and
    /// check instance __dict__ directly — a significant hot-path optimization.
    fn detect_descriptors(namespace: &FxAttrMap, mro: &[PyObjectRef]) -> bool {
        fn class_inherits_property(class: &PyObjectRef) -> bool {
            if let PyObjectPayload::Class(cd) = &class.payload {
                if cd.name.as_str() == "property" {
                    return true;
                }
                for base in &cd.bases {
                    match &base.payload {
                        PyObjectPayload::BuiltinType(name)
                        | PyObjectPayload::BuiltinFunction(name)
                            if name.as_str() == "property" =>
                        {
                            return true;
                        }
                        PyObjectPayload::Class(_) if class_inherits_property(base) => return true,
                        _ => {}
                    }
                }
            }
            false
        }

        // Check own namespace for Property or descriptor-like objects
        for v in namespace.values() {
            match &v.payload {
                PyObjectPayload::Property(_) => return true,
                PyObjectPayload::Instance(inst) => {
                    if class_inherits_property(&inst.class) {
                        return true;
                    }
                    let attrs = inst.attrs.read();
                    if attrs.contains_key("__set__") || attrs.contains_key("__delete__") {
                        return true;
                    }
                    // Check class for __set__/__delete__
                    if let PyObjectPayload::Class(icd) = &inst.class.payload {
                        if icd.namespace.read().contains_key("__set__")
                            || icd.namespace.read().contains_key("__delete__")
                        {
                            return true;
                        }
                    }
                }
                _ => {}
            }
        }
        // Check bases
        for base in mro {
            if let PyObjectPayload::Class(bcd) = &base.payload {
                if bcd.has_descriptors {
                    return true;
                }
            }
        }
        false
    }

    /// Check if this class inherits from dict (cached at class creation).
    fn check_dict_subclass(bases: &[PyObjectRef]) -> bool {
        for base in bases {
            let is_dict = match &base.payload {
                PyObjectPayload::BuiltinType(n) => n.as_str() == "dict",
                PyObjectPayload::Class(bcd) => bcd.name.as_str() == "dict" || bcd.is_dict_subclass,
                _ => false,
            };
            if is_dict {
                return true;
            }
        }
        false
    }
}
