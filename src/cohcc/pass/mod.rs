// CLASSIFICATION: COMMUNITY
// Filename: mod.rs v1.0
// Author: Lukas Bower
// Date Modified: 2025-05-31

// === Coh_CC Pass Framework (Batch 5) ===

pub trait IRPass {
    fn name(&self) -> &'static str;
    fn run(&self, module: &mut crate::ir::IRModule);
}

pub struct PassManager {
    passes: Vec<Box<dyn IRPass>>,
}

impl PassManager {
    pub fn new() -> Self {
        PassManager { passes: Vec::new() }
    }

    pub fn add_pass<P: IRPass + 'static>(&mut self, pass: P) {
        self.passes.push(Box::new(pass));
    }

    pub fn run_all(&mut self, module: &mut crate::ir::IRModule) {
        for pass in &self.passes {
            println!("Running pass: {}", pass.name());
            pass.run(module);
        }
    }
}
