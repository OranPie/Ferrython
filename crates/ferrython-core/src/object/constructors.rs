//! Singleton values and PyObject factory/constructor methods.

use crate::error::{ExceptionKind, PyResult};
use crate::types::{HashableKey, PyFunction, PyInt};
use compact_str::CompactString;
use indexmap::IndexMap;
use num_bigint::BigInt;
use parking_lot::RwLock;
use std::any::Any;
use std::sync::Arc;

use super::payload::*;
use super::methods::PyObjectMethods;

// ── Singletons ──
use std::sync::LazyLock;
static NONE_SINGLETON: LazyLock<PyObjectRef> = LazyLock::new(|| Arc::new(PyObject { payload: PyObjectPayload::None }));
static TRUE_SINGLETON: LazyLock<PyObjectRef> = LazyLock::new(|| Arc::new(PyObject { payload: PyObjectPayload::Bool(true) }));
static FALSE_SINGLETON: LazyLock<PyObjectRef> = LazyLock::new(|| Arc::new(PyObject { payload: PyObjectPayload::Bool(false) }));

// ── PyObject constructors ──

impl PyObject {
    pub fn wrap(payload: PyObjectPayload) -> PyObjectRef {
        ferrython_gc::notify_alloc();
        Arc::new(PyObject { payload })
    }
    pub fn none() -> PyObjectRef { NONE_SINGLETON.clone() }
    pub fn ellipsis() -> PyObjectRef { Self::wrap(PyObjectPayload::Ellipsis) }
    pub fn not_implemented() -> PyObjectRef { Self::wrap(PyObjectPayload::NotImplemented) }
    pub fn bool_val(v: bool) -> PyObjectRef { if v { TRUE_SINGLETON.clone() } else { FALSE_SINGLETON.clone() } }
    pub fn int(v: i64) -> PyObjectRef { Self::wrap(PyObjectPayload::Int(PyInt::Small(v))) }
    pub fn big_int(v: BigInt) -> PyObjectRef { Self::wrap(PyObjectPayload::Int(PyInt::Big(Box::new(v)))) }
    pub fn float(v: f64) -> PyObjectRef { Self::wrap(PyObjectPayload::Float(v)) }
    pub fn complex(real: f64, imag: f64) -> PyObjectRef { Self::wrap(PyObjectPayload::Complex { real, imag }) }
    pub fn str_val(v: CompactString) -> PyObjectRef { Self::wrap(PyObjectPayload::Str(v)) }
    pub fn bytes(v: Vec<u8>) -> PyObjectRef { Self::wrap(PyObjectPayload::Bytes(v)) }
    pub fn bytearray(v: Vec<u8>) -> PyObjectRef { Self::wrap(PyObjectPayload::ByteArray(v)) }
    pub fn list(items: Vec<PyObjectRef>) -> PyObjectRef { Self::wrap(PyObjectPayload::List(Arc::new(RwLock::new(items)))) }
    pub fn tuple(items: Vec<PyObjectRef>) -> PyObjectRef { Self::wrap(PyObjectPayload::Tuple(items)) }
    pub fn set(items: IndexMap<HashableKey, PyObjectRef>) -> PyObjectRef { Self::wrap(PyObjectPayload::Set(Arc::new(RwLock::new(items)))) }
    pub fn dict(items: IndexMap<HashableKey, PyObjectRef>) -> PyObjectRef { Self::wrap(PyObjectPayload::Dict(Arc::new(RwLock::new(items)))) }
    pub fn function(func: PyFunction) -> PyObjectRef { Self::wrap(PyObjectPayload::Function(func)) }
    pub fn builtin_function(name: CompactString) -> PyObjectRef { Self::wrap(PyObjectPayload::BuiltinFunction(name)) }
    pub fn builtin_type(name: CompactString) -> PyObjectRef { Self::wrap(PyObjectPayload::BuiltinType(name)) }
    pub fn code(code: ferrython_bytecode::CodeObject) -> PyObjectRef { Self::wrap(PyObjectPayload::Code(Box::new(code))) }
    pub fn class(name: CompactString, bases: Vec<PyObjectRef>, namespace: IndexMap<CompactString, PyObjectRef>) -> PyObjectRef {
        Self::wrap(PyObjectPayload::Class(ClassData { name, bases, namespace: Arc::new(RwLock::new(namespace)), mro: Vec::new() }))
    }
    pub fn instance(class: PyObjectRef) -> PyObjectRef {
        Self::wrap(PyObjectPayload::Instance(InstanceData { class, attrs: Arc::new(RwLock::new(IndexMap::new())) }))
    }
    pub fn module(name: CompactString) -> PyObjectRef {
        Self::wrap(PyObjectPayload::Module(ModuleData { name, attrs: IndexMap::new() }))
    }
    pub fn module_with_attrs(name: CompactString, attrs: IndexMap<CompactString, PyObjectRef>) -> PyObjectRef {
        Self::wrap(PyObjectPayload::Module(ModuleData { name, attrs }))
    }
    pub fn native_function(name: &str, func: fn(&[PyObjectRef]) -> PyResult<PyObjectRef>) -> PyObjectRef {
        Self::wrap(PyObjectPayload::NativeFunction { name: CompactString::from(name), func })
    }
    pub fn dict_from_pairs(pairs: Vec<(PyObjectRef, PyObjectRef)>) -> PyObjectRef {
        let mut map = IndexMap::new();
        for (k, v) in pairs {
            if let Ok(hk) = k.to_hashable_key() {
                map.insert(hk, v);
            }
        }
        Self::wrap(PyObjectPayload::Dict(Arc::new(RwLock::new(map))))
    }
    pub fn slice(start: Option<PyObjectRef>, stop: Option<PyObjectRef>, step: Option<PyObjectRef>) -> PyObjectRef {
        Self::wrap(PyObjectPayload::Slice { start, stop, step })
    }
    pub fn frozenset(items: IndexMap<HashableKey, PyObjectRef>) -> PyObjectRef {
        Self::wrap(PyObjectPayload::FrozenSet(items))
    }
    pub fn range(start: i64, stop: i64, step: i64) -> PyObjectRef {
        Self::wrap(PyObjectPayload::Iterator(Arc::new(std::sync::Mutex::new(IteratorData::Range { current: start, stop, step }))))
    }
    pub fn cell(cell: Arc<RwLock<Option<PyObjectRef>>>) -> PyObjectRef {
        Self::wrap(PyObjectPayload::Cell(cell))
    }
    pub fn exception_type(kind: ExceptionKind) -> PyObjectRef {
        Self::wrap(PyObjectPayload::ExceptionType(kind))
    }
    pub fn exception_instance(kind: ExceptionKind, message: impl Into<String>) -> PyObjectRef {
        let msg: String = message.into();
        let args = if msg.is_empty() { vec![] } else { vec![PyObject::str_val(CompactString::from(msg.as_str()))] };
        Self::wrap(PyObjectPayload::ExceptionInstance {
            kind,
            message: CompactString::from(msg),
            args,
        })
    }
    pub fn exception_instance_with_args(kind: ExceptionKind, message: impl Into<String>, args: Vec<PyObjectRef>) -> PyObjectRef {
        Self::wrap(PyObjectPayload::ExceptionInstance {
            kind,
            message: CompactString::from(message.into()),
            args,
        })
    }
    pub fn generator(name: CompactString, frame: Box<dyn Any + Send + Sync>) -> PyObjectRef {
        Self::wrap(PyObjectPayload::Generator(Arc::new(RwLock::new(GeneratorState {
            name,
            frame: Some(frame),
            started: false,
            finished: false,
        }))))
    }
}

