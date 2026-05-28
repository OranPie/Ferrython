//! Return and import opcode handlers.

use crate::frame::BlockKind;
use crate::VirtualMachine;
use compact_str::CompactString;
use ferrython_bytecode::opcode::Opcode;
use ferrython_bytecode::Instruction;
use ferrython_core::error::PyException;
use ferrython_core::object::{
    has_descriptor_get, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};

// ── Group 11: Return + Import ────────────────────────────────────────
impl VirtualMachine {
    pub(crate) fn exec_return_import(
        &mut self,
        instr: Instruction,
    ) -> Result<Option<PyObjectRef>, PyException> {
        match instr.op {
            Opcode::ReturnValue => {
                let frame = self.vm_frame();
                let value = frame.pop();
                // If inside a finally block, the new return overrides any pending return
                frame.pending_jump = None;
                let mut found_finally = false;
                while let Some(block) = frame.block_stack.last() {
                    if block.kind() == BlockKind::Finally {
                        let handler = block.handler();
                        frame.block_stack.pop();
                        frame.pending_return = Some(value.clone());
                        frame.push(PyObject::none());
                        frame.ip = handler;
                        found_finally = true;
                        break;
                    } else {
                        frame.block_stack.pop();
                    }
                }
                if !found_finally {
                    // Return immediately — new return value overrides any pending
                    return Ok(Some(value));
                }
            }
            Opcode::LoadFastReturnValue => {
                let frame = self.vm_frame();
                let val = match frame
                    .locals
                    .get(instr.arg as usize)
                    .and_then(|v| v.as_ref())
                {
                    Some(v) => v.clone(),
                    None => {
                        return Err(PyException::unbound_local_error(format!(
                            "local variable '{}' referenced before assignment",
                            frame
                                .code
                                .varnames
                                .get(instr.arg as usize)
                                .map(|s| s.as_str())
                                .unwrap_or("?")
                        )))
                    }
                };
                // Handle finally blocks
                frame.pending_jump = None;
                let mut found_finally = false;
                while let Some(block) = frame.block_stack.last() {
                    if block.kind() == BlockKind::Finally {
                        let handler = block.handler();
                        frame.block_stack.pop();
                        frame.pending_return = Some(val.clone());
                        frame.push(PyObject::none());
                        frame.ip = handler;
                        found_finally = true;
                        break;
                    } else {
                        frame.block_stack.pop();
                    }
                }
                if !found_finally {
                    return Ok(Some(val));
                }
            }
            Opcode::LoadConstReturnValue => {
                let frame = self.vm_frame();
                let val = frame.constant_cache[instr.arg as usize].clone();
                frame.pending_jump = None;
                let mut found_finally = false;
                while let Some(block) = frame.block_stack.last() {
                    if block.kind() == BlockKind::Finally {
                        let handler = block.handler();
                        frame.block_stack.pop();
                        frame.pending_return = Some(val.clone());
                        frame.push(PyObject::none());
                        frame.ip = handler;
                        found_finally = true;
                        break;
                    } else {
                        frame.block_stack.pop();
                    }
                }
                if !found_finally {
                    return Ok(Some(val));
                }
            }
            Opcode::ImportName => {
                let frame = self.vm_frame();
                let fromlist = frame.pop();
                let level_obj = frame.pop();
                let level = level_obj.as_int().unwrap_or(0) as usize;
                let name = frame.code.names[instr.arg as usize].clone();
                let filename = frame.code.filename.clone();
                let has_fromlist = !matches!(&fromlist.payload, PyObjectPayload::None);

                let module = self.import_module_dotted(&name, level, has_fromlist, &filename)?;
                self.vm_push(module);
                return Ok(None);
            }
            Opcode::ImportFrom => {
                let (name, module, mod_name, mod_file, filename) = {
                    let frame = self.vm_frame();
                    let name = frame.code.names[instr.arg as usize].clone();
                    let module = frame.peek().clone();
                    // Prefer __name__, but fall back to __package__ for relative imports
                    let raw_name = module
                        .get_attr("__name__")
                        .map(|n| n.py_to_string())
                        .unwrap_or_default();
                    let mod_name = if raw_name == "<package>" || raw_name.is_empty() {
                        // Use __package__ or derive from __file__
                        module
                            .get_attr("__package__")
                            .map(|p| p.py_to_string())
                            .filter(|s| !s.is_empty())
                            .or_else(|| {
                                module.get_attr("__file__").map(|f| {
                                    let fp = f.py_to_string();
                                    let path = std::path::Path::new(&fp);
                                    let is_init = path
                                        .file_name()
                                        .map(|f| f == "__init__.py")
                                        .unwrap_or(false);
                                    if is_init {
                                        path.parent()
                                            .and_then(|p| p.file_name())
                                            .and_then(|n| n.to_str())
                                            .unwrap_or("")
                                            .to_string()
                                    } else {
                                        path.file_stem()
                                            .and_then(|n| n.to_str())
                                            .unwrap_or("")
                                            .to_string()
                                    }
                                })
                            })
                            .unwrap_or(raw_name)
                    } else {
                        raw_name
                    };
                    let mod_file = module
                        .get_attr("__file__")
                        .map(|f| f.py_to_string())
                        .unwrap_or_else(|| "unknown location".to_string());
                    let filename = frame.code.filename.clone();
                    (name, module, mod_name, mod_file, filename)
                };
                match module.get_attr(&name) {
                    Some(v) => {
                        // Descriptor protocol: if the value has __get__ and was found
                        // via class lookup (not instance dict), invoke __get__.
                        // This handles six.moves lazy descriptors.
                        if has_descriptor_get(&v) {
                            if let Some(get_method) = v.get_attr("__get__") {
                                let (instance_arg, owner_arg) = match &module.payload {
                                    PyObjectPayload::Instance(inst) => {
                                        (module.clone(), inst.class.clone())
                                    }
                                    _ => (module.clone(), PyObject::none()),
                                };
                                match self.call_object(get_method, vec![instance_arg, owner_arg]) {
                                    Ok(result) => {
                                        self.vm_frame().push(result);
                                    }
                                    Err(_) => {
                                        self.vm_frame().push(v);
                                    }
                                }
                            } else {
                                self.vm_frame().push(v);
                            }
                        } else {
                            self.vm_frame().push(v);
                        }
                    }
                    None => {
                        // PEP 562: module-level __getattr__ for ImportFrom
                        if let PyObjectPayload::Module(_) = &module.payload {
                            if let Some(ga) = module.get_attr("__getattr__") {
                                let name_arg =
                                    PyObject::str_val(CompactString::from(name.as_str()));
                                if let Ok(result) = self.call_object(ga, vec![name_arg]) {
                                    self.vm_frame().push(result);
                                    return Ok(None);
                                }
                            }
                        }
                        // CPython fallback: try importing package.submodule
                        if !mod_name.is_empty() {
                            let submod_name = format!("{}.{}", mod_name, name);
                            // Use the correct search root: for packages (__init__.py),
                            // the importer must be the parent of the package directory
                            // so "urllib3/exceptions" resolves relative to site-packages/
                            let search_file = if mod_file.ends_with("__init__.py") {
                                // Go up two levels: __init__.py -> pkg_dir -> parent
                                let p = std::path::Path::new(&mod_file);
                                p.parent()
                                    .and_then(|pkg| pkg.parent())
                                    .map(|root| {
                                        root.join("__importer__").to_string_lossy().to_string()
                                    })
                                    .unwrap_or_else(|| filename.to_string())
                            } else {
                                filename.to_string()
                            };
                            match self.import_module_dotted(&submod_name, 0, true, &search_file) {
                                Ok(submod) => {
                                    match &module.payload {
                                        PyObjectPayload::Module(md) => {
                                            md.attrs.write().insert(name.clone(), submod.clone());
                                        }
                                        PyObjectPayload::Instance(inst) => {
                                            inst.attrs.write().insert(name.clone(), submod.clone());
                                        }
                                        _ => {}
                                    }
                                    self.vm_frame().push(submod);
                                }
                                Err(_e) => {
                                    // If the error itself is an ImportError for a name inside the submodule,
                                    // bubble it up rather than wrapping it.
                                    if _e.kind == ferrython_core::error::ExceptionKind::ImportError
                                    {
                                        let msg = _e.message.clone();
                                        if msg.starts_with("cannot import name")
                                            && !msg.contains(&format!("'{}'", name))
                                        {
                                            return Err(_e);
                                        }
                                    }
                                    return Err(PyException::import_error(format!(
                                        "cannot import name '{}' from '{}' ({})",
                                        name, mod_name, mod_file
                                    )));
                                }
                            }
                        } else {
                            return Err(PyException::import_error(format!(
                                "cannot import name '{}' from module",
                                name
                            )));
                        }
                    }
                }
            }
            Opcode::ImportStar => {
                let frame = self.vm_frame();
                let module = frame.pop();
                if let PyObjectPayload::Module(mod_data) = &module.payload {
                    let attrs = mod_data.attrs.read();
                    let all_names: Option<Vec<String>> = attrs.get("__all__").and_then(|v| {
                        v.to_list().ok().map(|items| {
                            items
                                .iter()
                                .map(|x: &PyObjectRef| x.py_to_string())
                                .collect::<Vec<String>>()
                        })
                    });
                    let mut globals = frame.globals.write();
                    for (k, v) in attrs.iter() {
                        if k.starts_with('_') && all_names.is_none() {
                            continue;
                        }
                        if let Some(ref names) = all_names {
                            if !names.contains(&k.to_string()) {
                                continue;
                            }
                        }
                        globals.insert(k.clone(), v.clone());
                    }
                }
            }
            _ => unreachable!(),
        }
        Ok(None)
    }
}
