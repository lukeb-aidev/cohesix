// CLASSIFICATION: COMMUNITY
// Filename: irq_controller.rs v0.1
// Author: Lukas Bower
// Date Modified: 2028-11-21
#![no_std]

use crate::coherr;

pub struct IrqController;

impl IrqController {
    pub fn register_irq(irq: usize) -> Result<(), ()> {
        coherr!("irq_register {}", irq);
        Ok(())
    }
}
