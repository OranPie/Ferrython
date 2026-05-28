//! Import statement compilation.

use ferrython_ast::*;
use ferrython_bytecode::{ConstantValue, Opcode};

use super::super::{Compiler, Result};

impl Compiler {
    // ── import compilation ──────────────────────────────────────────

    pub(super) fn compile_import(&mut self, names: &[Alias]) -> Result<()> {
        for alias in names {
            // Push level (0 for absolute import)
            let zero_idx = self.add_const(ConstantValue::Integer(0));
            self.emit_arg(Opcode::LoadConst, zero_idx);

            if alias.asname.is_some() && alias.name.contains('.') {
                // `import a.b.c as X` — ImportName with None fromlist returns
                // top-level `a`, then walk IMPORT_FROM chain to reach `a.b.c`.
                let parts: Vec<&str> = alias.name.split('.').collect();
                let none_idx = self.add_const(ConstantValue::None);
                self.emit_arg(Opcode::LoadConst, none_idx);

                let name_idx = self.add_name(&alias.name);
                self.emit_arg(Opcode::ImportName, name_idx);

                // Walk IMPORT_FROM for parts[1..] to reach the deepest submodule
                // Stack after ImportName: [a]
                // Each iteration: IMPORT_FROM x -> [parent, x], RotTwo -> [x, parent], PopTop -> [x]
                for part in &parts[1..] {
                    let from_idx = self.add_name(part);
                    self.emit_arg(Opcode::ImportFrom, from_idx);
                    self.emit_op(Opcode::RotTwo);
                    self.emit_op(Opcode::PopTop);
                }

                self.store_name(alias.asname.as_ref().unwrap());
            } else {
                // Regular import: `import a.b.c` stores `a`, `import foo` stores `foo`
                let none_idx = self.add_const(ConstantValue::None);
                self.emit_arg(Opcode::LoadConst, none_idx);

                let name_idx = self.add_name(&alias.name);
                self.emit_arg(Opcode::ImportName, name_idx);

                if let Some(ref asname) = alias.asname {
                    self.store_name(asname);
                } else {
                    let top = alias.name.split('.').next().unwrap_or(&alias.name);
                    if alias.name.contains('.') {
                        self.store_name(top);
                    } else {
                        self.store_name(&alias.name);
                    }
                }
            }
        }
        Ok(())
    }

    pub(super) fn compile_import_from(
        &mut self,
        module: Option<&str>,
        names: &[Alias],
        level: u32,
    ) -> Result<()> {
        // Push level
        let level_idx = self.add_const(ConstantValue::Integer(level as i64));
        self.emit_arg(Opcode::LoadConst, level_idx);

        // Build fromlist tuple
        if names.len() == 1 && names[0].name.as_str() == "*" {
            let star_idx =
                self.add_const(ConstantValue::Tuple(vec![ConstantValue::Str("*".into())]));
            self.emit_arg(Opcode::LoadConst, star_idx);
        } else {
            let from_names: Vec<ConstantValue> = names
                .iter()
                .map(|a| ConstantValue::Str(a.name.clone()))
                .collect();
            let tuple_idx = self.add_const(ConstantValue::Tuple(from_names));
            self.emit_arg(Opcode::LoadConst, tuple_idx);
        }

        let mod_name = module.unwrap_or("");
        let mod_idx = self.add_name(mod_name);
        self.emit_arg(Opcode::ImportName, mod_idx);

        if names.len() == 1 && names[0].name.as_str() == "*" {
            self.emit_op(Opcode::ImportStar);
        } else {
            for alias in names {
                let from_idx = self.add_name(&alias.name);
                self.emit_arg(Opcode::ImportFrom, from_idx);
                let store_as = alias.asname.as_deref().unwrap_or(&alias.name);
                self.store_name(store_as);
            }
            // Pop the module left by ImportName
            self.emit_op(Opcode::PopTop);
        }

        Ok(())
    }
}
