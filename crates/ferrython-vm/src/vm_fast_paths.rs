//! Small VM dispatch fast-path helpers.

use compact_str::CompactString;
use ferrython_core::object::{
    IteratorData, PyObject, PyObjectPayload, PyObjectRef, CLASS_FLAG_HAS_DESCRIPTORS,
    CLASS_FLAG_HAS_SETATTR, CLASS_FLAG_HAS_SLOTS,
};
use ferrython_core::types::PyInt;

#[inline(always)]
fn native_function_binds_to_class_name(
    class_name: &CompactString,
    attr_name: &CompactString,
    native_name: &str,
) -> bool {
    let expected_len = class_name.len() + attr_name.len() + 1;
    if native_name.len() != expected_len {
        return false;
    }
    native_name.starts_with(class_name.as_str())
        && native_name.as_bytes().get(class_name.len()) == Some(&b'.')
        && &native_name[class_name.len() + 1..] == attr_name.as_str()
}

#[inline(always)]
pub(crate) fn native_function_binds_to_class(
    cd: &ferrython_core::object::ClassData,
    attr_name: &CompactString,
    native_name: &str,
) -> bool {
    if native_function_binds_to_class_name(&cd.name, attr_name, native_name) {
        return true;
    }
    for base in cd.mro.iter().chain(cd.bases.iter()) {
        if let PyObjectPayload::Class(base_cd) = &base.payload {
            if native_function_binds_to_class_name(&base_cd.name, attr_name, native_name) {
                return true;
            }
        }
    }
    false
}

#[inline(always)]
pub(crate) fn fast_deque_native_closure_returns_none(name: &str, arg_count: usize) -> Option<bool> {
    match (name, arg_count) {
        ("deque.append", 1) | ("deque.appendleft", 1) => Some(true),
        ("deque.pop", 0) | ("deque.popleft", 0) => Some(false),
        _ => None,
    }
}

#[inline(always)]
pub(crate) fn try_fast_builtin_setattr_stack(
    stack: &mut Vec<PyObjectRef>,
    func_idx: usize,
) -> bool {
    if stack.len() < func_idx + 4 {
        return false;
    }

    unsafe {
        let base = stack.as_mut_ptr();
        let obj = &*base.add(func_idx + 1);
        let name_obj = &*base.add(func_idx + 2);
        let inst = match &obj.payload {
            PyObjectPayload::Instance(inst)
                if inst.class_flags
                    & (CLASS_FLAG_HAS_SETATTR
                        | CLASS_FLAG_HAS_DESCRIPTORS
                        | CLASS_FLAG_HAS_SLOTS)
                    == 0 =>
            {
                inst
            }
            _ => return false,
        };
        let name = match &name_obj.payload {
            PyObjectPayload::Str(s) => s.as_str(),
            _ => return false,
        };
        let map = &mut *inst.attrs.data_ptr();
        let value = std::ptr::read(base.add(func_idx + 3));
        if let Some(slot) = map.get_mut(name) {
            let old = std::mem::replace(slot, value);
            drop(old);
        } else {
            map.insert(CompactString::from(name), value);
        }
        let _func = std::ptr::read(base.add(func_idx));
        let _obj = std::ptr::read(base.add(func_idx + 1));
        let _name = std::ptr::read(base.add(func_idx + 2));
        stack.set_len(func_idx);
    }
    true
}

#[inline(always)]
pub(crate) fn fast_callable_bool(arg: &PyObjectRef) -> Option<bool> {
    match &arg.payload {
        PyObjectPayload::Function(_)
        | PyObjectPayload::BuiltinFunction(_)
        | PyObjectPayload::BuiltinType(_)
        | PyObjectPayload::BoundMethod { .. }
        | PyObjectPayload::BuiltinBoundMethod(_)
        | PyObjectPayload::Class(_)
        | PyObjectPayload::ExceptionType(_)
        | PyObjectPayload::NativeFunction(_)
        | PyObjectPayload::NativeClosure(_)
        | PyObjectPayload::Partial(_) => Some(true),
        PyObjectPayload::Instance(_) => None,
        _ => Some(false),
    }
}

#[inline(always)]
pub(crate) fn fast_int_conversion(arg: &PyObjectRef) -> Option<PyObjectRef> {
    match &arg.payload {
        PyObjectPayload::Int(_) => Some(arg.clone()),
        PyObjectPayload::Bool(b) => Some(PyObject::int(if *b { 1 } else { 0 })),
        PyObjectPayload::Float(f) => Some(PyObject::int(*f as i64)),
        PyObjectPayload::Str(s) => s.trim().parse::<i64>().ok().map(PyObject::int),
        _ => None,
    }
}

#[inline(always)]
pub(crate) fn fast_small_int_sequence_min_max(
    arg: &PyObjectRef,
    is_max: bool,
) -> Option<PyObjectRef> {
    let items: &[PyObjectRef] = match &arg.payload {
        PyObjectPayload::List(v) => unsafe { (&*v.data_ptr()).as_slice() },
        PyObjectPayload::Tuple(v) => v.as_slice(),
        _ => return None,
    };
    if items.is_empty() {
        return None;
    }
    let mut best_idx = 0usize;
    let mut best_val = match &items[0].payload {
        PyObjectPayload::Int(PyInt::Small(n)) => *n,
        _ => return None,
    };
    for (i, item) in items[1..].iter().enumerate() {
        let value = match &item.payload {
            PyObjectPayload::Int(PyInt::Small(n)) => *n,
            _ => return None,
        };
        if (is_max && value > best_val) || (!is_max && value < best_val) {
            best_idx = i + 1;
            best_val = value;
        }
    }
    Some(items[best_idx].clone())
}

#[inline(always)]
pub(crate) fn fast_small_int_sequence_sorted(arg: &PyObjectRef) -> Option<PyObjectRef> {
    let all_small = match &arg.payload {
        PyObjectPayload::List(v) => unsafe { &*v.data_ptr() }
            .iter()
            .all(|item| matches!(&item.payload, PyObjectPayload::Int(PyInt::Small(_)))),
        PyObjectPayload::Tuple(items) => items
            .iter()
            .all(|item| matches!(&item.payload, PyObjectPayload::Int(PyInt::Small(_)))),
        _ => return None,
    };
    if !all_small {
        return None;
    }

    let mut items = match &arg.payload {
        PyObjectPayload::List(v) if PyObjectRef::strong_count(arg) == 1 => {
            std::mem::take(unsafe { &mut *v.data_ptr() })
        }
        PyObjectPayload::List(v) => unsafe { &*v.data_ptr() }.clone(),
        PyObjectPayload::Tuple(items) => items.to_vec(),
        _ => return None,
    };
    items.sort_unstable_by(|a, b| {
        let av = if let PyObjectPayload::Int(PyInt::Small(v)) = &a.payload {
            *v
        } else {
            0
        };
        let bv = if let PyObjectPayload::Int(PyInt::Small(v)) = &b.payload {
            *v
        } else {
            0
        };
        av.cmp(&bv)
    });
    Some(PyObject::list(items))
}

#[inline(always)]
pub(crate) fn fast_exact_type(arg: &PyObjectRef) -> Option<PyObjectRef> {
    match &arg.payload {
        PyObjectPayload::None => Some(PyObject::builtin_type_by_name("NoneType")),
        PyObjectPayload::Ellipsis => Some(PyObject::builtin_type_by_name("ellipsis")),
        PyObjectPayload::NotImplemented => {
            Some(PyObject::builtin_type_by_name("NotImplementedType"))
        }
        PyObjectPayload::Bool(_) => Some(PyObject::builtin_type_by_name("bool")),
        PyObjectPayload::Int(_) => Some(PyObject::builtin_type_by_name("int")),
        PyObjectPayload::Float(_) => Some(PyObject::builtin_type_by_name("float")),
        PyObjectPayload::Complex { .. } => Some(PyObject::builtin_type_by_name("complex")),
        PyObjectPayload::Str(_) => Some(PyObject::builtin_type_by_name("str")),
        PyObjectPayload::Bytes(_) => Some(PyObject::builtin_type_by_name("bytes")),
        PyObjectPayload::ByteArray(_) => Some(PyObject::builtin_type_by_name("bytearray")),
        PyObjectPayload::List(_) => Some(PyObject::builtin_type_by_name("list")),
        PyObjectPayload::Tuple(_) => Some(PyObject::builtin_type_by_name("tuple")),
        PyObjectPayload::Set(_) => Some(PyObject::builtin_type_by_name("set")),
        PyObjectPayload::FrozenSet(_) => Some(PyObject::builtin_type_by_name("frozenset")),
        PyObjectPayload::Dict(_) | PyObjectPayload::InstanceDict(_) => {
            Some(PyObject::builtin_type_by_name("dict"))
        }
        PyObjectPayload::MappingProxy(_) => Some(PyObject::builtin_type_by_name("mappingproxy")),
        PyObjectPayload::Function(_) => Some(PyObject::builtin_type_by_name("function")),
        PyObjectPayload::BuiltinFunction(_)
        | PyObjectPayload::NativeFunction(_)
        | PyObjectPayload::NativeClosure(_) => {
            Some(PyObject::builtin_type_by_name("builtin_function_or_method"))
        }
        PyObjectPayload::BuiltinType(_)
        | PyObjectPayload::Class(_)
        | PyObjectPayload::ExceptionType(_) => Some(PyObject::builtin_type_by_name("type")),
        PyObjectPayload::BoundMethod { .. } => Some(PyObject::builtin_type_by_name("method")),
        PyObjectPayload::BuiltinBoundMethod(_) => {
            Some(PyObject::builtin_type_by_name("builtin_method"))
        }
        PyObjectPayload::Code(_) => Some(PyObject::builtin_type_by_name("code")),
        PyObjectPayload::Instance(inst) => Some(inst.class.clone()),
        PyObjectPayload::Module(_) => Some(PyObject::builtin_type_by_name("module")),
        PyObjectPayload::Iterator(iter_data) => {
            let guard = iter_data.read();
            let name = match &*guard {
                IteratorData::MapOne { .. } | IteratorData::Map { .. } => "map",
                IteratorData::Filter { .. } => "filter",
                IteratorData::Zip { .. } => "zip",
                IteratorData::Enumerate { .. } => "enumerate",
                IteratorData::Range { .. } => "range_iterator",
                IteratorData::List { .. } => "list_iterator",
                IteratorData::Tuple { .. } => "tuple_iterator",
                IteratorData::Str { .. } => "str_ascii_iterator",
                IteratorData::DictEntries { .. } => "dict_itemiterator",
                IteratorData::DictKeys { .. } => "dict_keyiterator",
                IteratorData::Sentinel { .. } => "callable_iterator",
                _ => "iterator",
            };
            Some(PyObject::builtin_type_by_name(name))
        }
        PyObjectPayload::RangeIter(_) => Some(PyObject::builtin_type_by_name("range_iterator")),
        PyObjectPayload::VecIter(_)
        | PyObjectPayload::WeakValueIter(_)
        | PyObjectPayload::WeakKeyIter(_)
        | PyObjectPayload::RefIter { .. } => Some(PyObject::builtin_type_by_name("list_iterator")),
        PyObjectPayload::RevRefIter { .. } => {
            Some(PyObject::builtin_type_by_name("list_reverseiterator"))
        }
        PyObjectPayload::Slice(_) => Some(PyObject::builtin_type_by_name("slice")),
        PyObjectPayload::Cell(_) => Some(PyObject::builtin_type_by_name("cell")),
        PyObjectPayload::ExceptionInstance(ei) => Some(PyObject::exception_type(ei.kind)),
        PyObjectPayload::Generator(_) => Some(PyObject::builtin_type_by_name("generator")),
        PyObjectPayload::Coroutine(_) | PyObjectPayload::BuiltinAwaitable(_) => {
            Some(PyObject::builtin_type_by_name("coroutine"))
        }
        PyObjectPayload::AsyncGenerator(_) => {
            Some(PyObject::builtin_type_by_name("async_generator"))
        }
        PyObjectPayload::AsyncGenAwaitable { .. } => {
            Some(PyObject::builtin_type_by_name("async_generator_asend"))
        }
        PyObjectPayload::Property(_) => Some(PyObject::builtin_type_by_name("property")),
        PyObjectPayload::StaticMethod(_) => Some(PyObject::builtin_type_by_name("staticmethod")),
        PyObjectPayload::ClassMethod(_) => Some(PyObject::builtin_type_by_name("classmethod")),
        PyObjectPayload::Super { .. } => Some(PyObject::builtin_type_by_name("super")),
        PyObjectPayload::Partial(_) => Some(PyObject::builtin_type_by_name("functools.partial")),
        PyObjectPayload::Range(_) => Some(PyObject::builtin_type_by_name("range")),
        PyObjectPayload::DeferredSleep { .. } => Some(PyObject::builtin_type_by_name("coroutine")),
        PyObjectPayload::DictKeys { .. } => {
            Some(PyObject::builtin_type_by_name("dict_keyiterator"))
        }
        PyObjectPayload::DictValues { .. } | PyObjectPayload::DictItems { .. } => {
            Some(PyObject::builtin_type_by_name("dict_itemiterator"))
        }
    }
}
