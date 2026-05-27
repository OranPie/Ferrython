use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    check_args_min, CompareOp, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};

// ── queue.Queue / LifoQueue / PriorityQueue methods ──

pub(crate) fn call_queue_method(
    inst: &ferrython_core::object::InstanceData,
    method: &str,
    args: &[PyObjectRef],
) -> PyResult<PyObjectRef> {
    let attrs = inst.attrs.read();
    let kind = attrs
        .get("__queue__")
        .map(|v| v.py_to_string())
        .unwrap_or_default();
    let items_ref = attrs.get("_items").cloned();
    let maxsize = attrs.get("maxsize").and_then(|v| v.as_int()).unwrap_or(0);
    drop(attrs);

    let items_obj = items_ref.ok_or_else(|| PyException::runtime_error("queue has no _items"))?;

    match method {
        "put" | "put_nowait" => {
            check_args_min(method, args, 1)?;
            if let PyObjectPayload::List(lock) = &items_obj.payload {
                let mut items = lock.write();
                if maxsize > 0 && items.len() as i64 >= maxsize {
                    return Err(PyException::runtime_error("queue.Full"));
                }
                items.push(args[0].clone());
                // PriorityQueue: keep sorted (min-heap via sort)
                if kind == "PriorityQueue" {
                    items.sort_by(|a, b| {
                        let lt = a
                            .compare(b, CompareOp::Lt)
                            .map(|v| v.is_truthy())
                            .unwrap_or(false);
                        if lt {
                            std::cmp::Ordering::Less
                        } else {
                            let gt = a
                                .compare(b, CompareOp::Gt)
                                .map(|v| v.is_truthy())
                                .unwrap_or(false);
                            if gt {
                                std::cmp::Ordering::Greater
                            } else {
                                std::cmp::Ordering::Equal
                            }
                        }
                    });
                }
            }
            Ok(PyObject::none())
        }
        "get" | "get_nowait" => {
            if let PyObjectPayload::List(lock) = &items_obj.payload {
                let mut items = lock.write();
                if items.is_empty() {
                    return Err(PyException::runtime_error("Empty"));
                }
                let result = match kind.as_str() {
                    "LifoQueue" => items.pop().unwrap(),
                    _ => items.remove(0), // FIFO or PriorityQueue (sorted, take smallest)
                };
                Ok(result)
            } else {
                Err(PyException::type_error("queue internal error"))
            }
        }
        "empty" => {
            if let PyObjectPayload::List(lock) = &items_obj.payload {
                Ok(PyObject::bool_val(lock.read().is_empty()))
            } else {
                Ok(PyObject::bool_val(true))
            }
        }
        "full" => {
            if maxsize <= 0 {
                Ok(PyObject::bool_val(false))
            } else if let PyObjectPayload::List(lock) = &items_obj.payload {
                Ok(PyObject::bool_val(lock.read().len() as i64 >= maxsize))
            } else {
                Ok(PyObject::bool_val(false))
            }
        }
        "qsize" => {
            if let PyObjectPayload::List(lock) = &items_obj.payload {
                Ok(PyObject::int(lock.read().len() as i64))
            } else {
                Ok(PyObject::int(0))
            }
        }
        "task_done" | "join" => Ok(PyObject::none()),
        _ => Err(PyException::attribute_error(format!(
            "'{}' object has no attribute '{}'",
            kind, method
        ))),
    }
}
