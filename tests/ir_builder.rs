// CLASSIFICATION: COMMUNITY
// Filename: ir_builder.rs v0.1
// Date Modified: 2025-07-11
// Author: Cohesix Codex

use cohesix::ir::{DeadCodeEliminationPass, IRBuilder, PassManager};

#[test]
fn builder_and_pass_framework() {
    let mut builder = IRBuilder::new("demo");
    builder.start_function("main");
    builder.build_add("1", "2");
    builder.emit(cohesix::ir::Instruction::new(
        cohesix::ir::Opcode::Nop,
        vec![],
    ));
    builder.finish_function();

    let mut module = builder.finalize();

    let mut pm = PassManager::new();
    pm.add_pass(DeadCodeEliminationPass::new());
    pm.run_all(&mut module);

    let text = module.print_ir();
    assert!(text.contains("Add"));
    assert!(!text.contains("Nop"));
}
