// CLASSIFICATION: COMMUNITY
// Filename: ir_pass_framework.rs v1.0
// Date Modified: 2025-05-26
// Author: Lukas Bower

//! Consolidated IR and Pass Framework for the Coh_CC compiler.

/// IR definitions and data structures
pub mod ir {
    /// Represents a single IR instruction.
    #[derive(Clone, Debug)]
    pub struct Instruction {
        pub opcode: String,
        pub operands: Vec<String>,
    }

    /// A function is a named collection of instructions.
    #[derive(Clone, Debug)]
    pub struct Function {
        pub name: String,
        pub body: Vec<Instruction>,
    }

    /// A module is a collection of functions (compilation unit).
    #[derive(Clone, Debug)]
    pub struct Module {
        pub functions: Vec<Function>,
    }

    /// Shared IR context for passing between passes.
    #[derive(Default, Debug)]
    pub struct IRContext {
        pub modules: Vec<Module>,
    }
}

/// Pass framework and manager
pub mod pass {
    use crate::pass_framework::ir_pass_framework::ir::IRContext;

    /// Trait implemented by all IR transformation or analysis passes.
    pub trait IRPass {
        /// Returns the unique name of the pass.
        fn name(&self) -> &'static str;
        /// Runs the pass against the IR context, mutating as needed.
        fn run(&self, ctx: &mut IRContext);
    }

    /// Manages registration and execution of multiple IR passes.
    pub struct PassManager {
        passes: Vec<Box<dyn IRPass>>,
    }

    impl PassManager {
        /// Creates an empty pass manager.
        pub fn new() -> Self {
            PassManager { passes: Vec::new() }
        }

        /// Registers a new pass.
        pub fn add_pass<P: IRPass + 'static>(&mut self, pass: P) {
            self.passes.push(Box::new(pass));
        }

        /// Executes all registered passes in order.
        pub fn run_all(&self, ctx: &mut IRContext) {
            for p in &self.passes {
                log::info!("Running pass: {}", p.name());
                p.run(ctx);
            }
        }
    }
}
