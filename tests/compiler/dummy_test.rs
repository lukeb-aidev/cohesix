// CLASSIFICATION: COMMUNITY
// Filename: dummy_test.rs v0.1
// Date Modified: 2025-05-24
// Author: Lukas Bower

//! TODO: Implement dummy_test.rs.

// CLASSIFICATION: COMMUNITY
// Filename: dummy_test.rs v0.2
// Date Modified: 2025-05-27
// Author: Lukas Bower

//! Integration test for basic IR instruction functionality.

use cohesix::ir::{Instruction, Opcode};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_instruction_new_and_to_string() {
        let instr = Instruction::new(
            Opcode::Add,
            vec!["r1".into(), "r2".into()],
        );
        // Check opcode and operands
        assert_eq!(instr.opcode, Opcode::Add);
        assert_eq!(instr.operands, vec!["r1".to_string(), "r2".to_string()]);
        // to_string should include opcode and operands
        let s = instr.to_string();
        assert!(s.contains("Add"));
        assert!(s.contains("r1"));
        assert!(s.contains("r2"));
    }

    #[test]
    fn test_branch_to_string() {
        let instr = Instruction::new(
            Opcode::Branch { condition: "eq".into() },
            vec!["r1".into(), "r2".into()],
        );
        let s = instr.to_string();
        assert!(s.contains("Branch r1, r2 if eq"));
    }
}