use cohesix::ir::{Function, IRContext, Instruction, Module, Opcode};
use cohesix::pass_framework::PassManager;
use cohesix::passes::{ConstFold, DeadCode, NopPass};
use criterion::{criterion_group, criterion_main, Criterion};

fn make_test_context() -> IRContext {
    let mut module = Module::new("bench");
    let mut func = Function::new("f");
    for _ in 0..100 {
        func.body.push(Instruction::new(Opcode::Nop, vec![]));
        func.body
            .push(Instruction::new(Opcode::Add, vec!["2".into(), "3".into()]));
        func.body
            .push(Instruction::new(Opcode::Sub, vec!["5".into(), "2".into()]));
    }
    module.add_function(func);
    let mut ctx = IRContext::default();
    ctx.modules.push(module);
    ctx
}

fn bench_pass_manager(c: &mut Criterion) {
    c.bench_function("pass_manager", |b| {
        b.iter(|| {
            let mut ctx = make_test_context();
            let mut mgr = PassManager::new();
            mgr.add_pass(NopPass::new());
            mgr.add_pass(ConstFold::new());
            mgr.add_pass(DeadCode::new());
            mgr.run_all(&mut ctx);
        });
    });
}

criterion_group!(benches, bench_pass_manager);
criterion_main!(benches);
