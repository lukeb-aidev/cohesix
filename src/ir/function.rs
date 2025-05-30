use crate::ir::instruction::Instruction;

#[derive(Clone, Debug)]
pub struct Function {
    pub name: String,
    pub body: Vec<Instruction>,
}

impl std::fmt::Display for Function {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}
