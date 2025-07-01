// CLASSIFICATION: COMMUNITY
// Filename: passes_test.rs v1.0
// Author: Lukas Bower
// Date Modified: 2025-05-31
/// Tests for IR pass framework

use crate::prelude::*;
#[cfg(test)]
mod tests {
    use super::*;
    use crate::passes::pass_registry::PassRegistry;
    use crate::passes::ir_pass_trait::IRPass;
    use crate::ir::context::IRContext;

    struct DummyPass;

    impl IRPass for DummyPass {
        fn name(&self) -> &'static str {
            "DummyPass"
        }

        fn run(&self, _context: &mut IRContext) {
            println!("[DummyPass] running...");
        }
    }

    #[test]
    fn pass_registry_smoke_test() {
        let mut registry = PassRegistry::new();
        registry.register(Box::new(DummyPass));

        let mut ctx = IRContext::new();
        registry.run_all_passes(&mut ctx);
    }
}
