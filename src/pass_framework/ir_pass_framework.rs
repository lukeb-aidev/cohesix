// pass_framework/ir_pass_framework.rs – façade (auto-patch v8)
//
// We now treat IR types canonically under crate::ir; this file merely
// re-exports them for older call-sites.

pub use crate::ir::function::Function;
pub use crate::ir::module::Module;

// Re-export IRContext too, in case something still points here.
pub use crate::ir::context::IRContext;
