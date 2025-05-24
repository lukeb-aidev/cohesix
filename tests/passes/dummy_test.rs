// CLASSIFICATION: COMMUNITY
// Filename: dummy_test.rs v0.2
// Date Modified: 2025-05-27
// Author: Lukas Bower

//! Integration tests for IR pass implementations (NOP, DeadCode, ConstFold).

use cohesix::ir::{Instruction, Opcode, IRContext, Module};
use cohesix::pass_framework::PassManager;
use cohesix::passes::{ConstFold, DeadCode, NopPass};

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_context() -> IRContext {
        // Create a module with instructions: NOP, ADD 2 3, SUB 5 2
        let mut module = Module::new("test");
        let mut func = crate::ir::Function { name: "f".into(), body: vec![] };
        func.body.push(Instruction::new(Opcode::Nop, vec![]));
        func.body.push(Instruction::new(Opcode::Add, vec!["2".into(), "3".into()]));
        func.body.push(Instruction::new(Opcode::Sub, vec!["5".into(), "2".into()]));
        module.add_function(func);
        let mut ctx = IRContext::default();
        ctx.modules.push(module);
        ctx
    }

    #[test]
    fn test_nop_pass_removes_nop() {
        let mut ctx = make_test_context();
        let pass = NopPass::new();
        pass.run(&mut ctx);
        let body = &ctx.modules[0].functions[0].body;
        assert!(body.iter().all(|i| i.opcode != Opcode::Nop), "NOP instructions should be removed");
    }

    #[test]
    fn test_deadcode_pass_retains_side_effects() {
        let mut ctx = make_test_context();
        // Nop will remain for DeadCode; only NOP is dead
        let pass = DeadCode::new();
        pass.run(&mut ctx);
        let body = &ctx.modules[0].functions[0].body;
        // NOP is dead, so next instructions remain
        assert!(body.iter().all(|i| i.opcode != Opcode::Nop), "DeadCode should remove NOP");
        assert_eq!(body.len(), 2);
    }

    #[test]
    fn test_const_fold_pass_folds_constants() {
        let mut ctx = make_test_context();
        // Replace Add 2 3 with Load 5
        let pass = ConstFold::new();
        pass.run(&mut ctx);
        let body = &ctx.modules[0].functions[0].body;
        // First element was NOP but retained by const fold
        // Second should now be a Load of 5
        assert_eq!(body[1].opcode, Opcode::Load);
        assert_eq!(body[1].operands[0], "5");
    }

    #[test]
    fn test_pass_manager_runs_all() {
        let mut ctx = make_test_context();
        let mut mgr = PassManager::new();
        mgr.add_pass(NopPass::new());
        mgr.add_pass(ConstFold::new());
        mgr.add_pass(DeadCode::new());
        mgr.run_all(&mut ctx);
        // After all passes: no NOP, folded instructions, deadcode removed
        let body = &ctx.modules[0].functions[0].body;
        assert_eq!(body.len(), 2, "Final body should have two instructions");
        assert!(body.iter().all(|i| i.opcode != Opcode::Nop));
    }
}
