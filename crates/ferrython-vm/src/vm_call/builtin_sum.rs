use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{PyObject, PyObjectPayload, PyObjectRef};

use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_sum_builtin(&mut self, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Err(PyException::type_error(
                "sum() requires at least 1 argument",
            ));
        }
        let start = if args.len() > 1 {
            args[1].clone()
        } else {
            PyObject::int(0)
        };
        let mut total = start;
        match &args[0].payload {
            PyObjectPayload::List(cell) => {
                let items = cell.read();
                total = self.sum_items(total, &items)?;
            }
            PyObjectPayload::Tuple(items) => {
                total = self.sum_items(total, items)?;
            }
            PyObjectPayload::Range(rd) => {
                total = self.sum_range(total, rd.start, rd.stop, rd.step)?;
            }
            PyObjectPayload::RangeIter(ri) => {
                total = self.sum_range_iter(total, ri)?;
            }
            PyObjectPayload::Iterator(_) => {
                let items = self.collect_iterable(&args[0])?;
                total = self.sum_items(total, &items)?;
            }
            PyObjectPayload::Generator(gen_arc) => {
                total = self.sum_generator(total, gen_arc.clone())?;
            }
            _ => {
                let items = self.collect_iterable(&args[0])?;
                total = self.sum_items(total, &items)?;
            }
        }
        Ok(total)
    }
}
