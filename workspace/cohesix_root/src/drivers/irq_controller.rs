// CLASSIFICATION: COMMUNITY
// Filename: irq_controller.rs v0.2
// Author: Lukas Bower
// Date Modified: 2028-11-21
use crate::coherr;

pub struct IrqController;

impl IrqController {
    pub fn register_irq(irq: usize) -> Result<(), ()> {
        coherr!("irq_register {}", irq);
        Ok(())
    }

    pub fn enable_irq(irq: usize) -> Result<(), ()> {
        coherr!("irq_enable {}", irq);
        Ok(())
    }
}
