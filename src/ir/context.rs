use crate::ir::module::Module;

#[derive(Clone, Debug, Default)]
pub struct IRContext {
    pub modules: Vec<Module>,
}
