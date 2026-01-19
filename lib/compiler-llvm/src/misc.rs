use inkwell::targets::TargetMachine;

pub trait TargetMachineExt {
    fn is_riscv64(&self) -> bool;
}

impl TargetMachineExt for TargetMachine {
    fn is_riscv64(&self) -> bool {
        self.get_triple()
            .as_str()
            .to_string_lossy()
            .starts_with("riscv64")
    }
}
