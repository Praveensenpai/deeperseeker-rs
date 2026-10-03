use deeperseeker::infra::pow::PowSolver;

#[test]
fn test_pow_solver_instantiation() {
    let solver = PowSolver::new("wasm/deepseek_pow_solver.wasm");
    assert!(solver.is_ok(), "PowSolver should successfully load WASM module");
}
