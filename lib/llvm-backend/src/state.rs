use inkwell::{
    basic_block::BasicBlock,
    values::{BasicValue, BasicValueEnum},
};
use wasmparser::BinaryReaderError;

enum ControlFrame {
    If {
        dest: BasicBlock,
        stack_size_snapshot: usize,
    },
    Block {
        dest: BasicBlock,
        stack_size_snapshot: usize,
        num_ret_values: usize,
    },
}

pub struct State {
    stack: Vec<BasicValueEnum>,
    control_stack: Vec<ControlFrame>,
    value_counter: usize,
}

impl State {
    pub fn new() -> Self {
        Self {
            stack: vec![],
            control_stack: vec![],
            value_counter: 0,
        }
    }

    pub fn var_name(&mut self) -> String {
        let s = self.value_counter.to_string();
        self.value_counter += 1;
        s
    }

    pub fn push1<T: BasicValue>(&mut self, value: T) {
        self.stack.push(value.as_basic_value_enum())
    }

    pub fn pop1(&mut self) -> Result<BasicValueEnum, BinaryReaderError> {
        self.stack.pop().ok_or_else(|| BinaryReaderError {
            message: "invalid value stack",
            offset: -1isize as usize,
        })
    }

    pub fn pop2(&mut self) -> Result<(BasicValueEnum, BasicValueEnum), BinaryReaderError> {
        let v2 = self.pop1()?;
        let v1 = self.pop1()?;
        Ok((v1, v2))
    }

    pub fn pop3(
        &mut self,
    ) -> Result<(BasicValueEnum, BasicValueEnum, BasicValueEnum), BinaryReaderError> {
        let v3 = self.pop1()?;
        let v2 = self.pop1()?;
        let v1 = self.pop1()?;
        Ok((v1, v2, v3))
    }

    pub fn peek1(&self) -> Result<BasicValueEnum, BinaryReaderError> {
        self.stack
            .get(self.stack.len() - 1)
            .ok_or_else(|| BinaryReaderError {
                message: "invalid value stack",
                offset: -1isize as usize,
            })
            .map(|v| *v)
    }

    pub fn peekn(&self, n: usize) -> Result<&[BasicValueEnum], BinaryReaderError> {
        self.stack
            .get(self.stack.len() - n..)
            .ok_or_else(|| BinaryReaderError {
                message: "invalid value stack",
                offset: -1isize as usize,
            })
    }

    pub fn popn(&mut self, n: usize) -> Result<(), BinaryReaderError> {
        if self.stack.len() < n {
            return Err(BinaryReaderError {
                message: "invalid value stack",
                offset: -1isize as usize,
            });
        }

        let new_len = self.stack.len() - n;
        self.stack.truncate(new_len);
        Ok(())
    }

    pub fn push_block(&mut self, dest: BasicBlock, num_ret_values: usize) {
        self.control_stack.push(ControlFrame::Block {
            dest,
            stack_size_snapshot: self.stack.len(),
            num_ret_values,
        });
    }
}
