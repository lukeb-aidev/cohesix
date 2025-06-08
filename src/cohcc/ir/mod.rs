// CLASSIFICATION: COMMUNITY
// Filename: mod.rs v1.0
// Author: Lukas Bower
// Date Modified: 2025-07-14

// === Coh_CC IR Framework (Batch 5) ===

pub enum IRNode {
    Function { name: String, args: Vec<String> },
    Let { var: String, value: Box<IRNode> },
    Call { callee: String, args: Vec<IRNode> },
    Literal(String),
    Return { value: Box<IRNode> },
}

pub struct IRModule {
    pub functions: Vec<IRNode>,
}

impl IRModule {
    pub fn new() -> Self {
        IRModule { functions: Vec::new() }
    }

    pub fn add(&mut self, node: IRNode) {
        self.functions.push(node);
    }
}
