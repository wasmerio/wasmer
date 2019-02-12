use inkwell::{
    basic_block::BasicBlock,
    values::{BasicValue, BasicValueEnum, PhiValue},
};
use smallvec::SmallVec;
use std::cell::Cell;
use wasmparser::BinaryReaderError;

pub enum ControlFrame {
    Block {
        end_label: BasicBlock,
        phis: SmallVec<[PhiValue; 1]>,
        stack_size_snapshot: usize,
    },
}

impl ControlFrame {
    pub fn dest(&self) -> &BasicBlock {
        match self {
            ControlFrame::Block { ref end_label, .. } => end_label,
        }
    }

    pub fn phis(&self) -> &[PhiValue] {
        match self {
            ControlFrame::Block { ref phis, .. } => phis.as_slice(),
        }
    }
}

pub struct State {
    stack: Vec<BasicValueEnum>,
    control_stack: Vec<ControlFrame>,
    value_counter: Cell<usize>,
    block_counter: Cell<usize>,
}

impl State {
    pub fn new() -> Self {
        Self {
            stack: vec![],
            control_stack: vec![],
            value_counter: Cell::new(0),
            block_counter: Cell::new(0),
        }
    }

    pub fn reset_stack(&mut self, frame: &ControlFrame) {
        let stack_size_snapshot = match frame {
            ControlFrame::Block {
                stack_size_snapshot,
                ..
            } => *stack_size_snapshot,
        };
        self.stack.truncate(stack_size_snapshot);
    }

    pub fn frame_at_depth(&self, depth: u32) -> Result<&ControlFrame, BinaryReaderError> {
        let index = self.control_stack.len() - 1 - (depth as usize);
        self.control_stack.get(index).ok_or(BinaryReaderError {
            message: "invalid control stack depth",
            offset: -1isize as usize,
        })
    }

    pub fn pop_frame(&mut self) -> Result<ControlFrame, BinaryReaderError> {
        self.control_stack.pop().ok_or(BinaryReaderError {
            message: "cannot pop from control stack",
            offset: -1isize as usize,
        })
    }

    pub fn block_name(&self) -> String {
        let counter = self.block_counter.get();
        let s = format!("block{}", counter);
        self.block_counter.set(counter + 1);
        s
    }

    pub fn var_name(&self) -> String {
        let counter = self.value_counter.get();
        let s = format!("s{}", counter);
        self.value_counter.set(counter + 1);
        s
    }

    pub fn push1<T: BasicValue>(&mut self, value: T) {
        self.stack.push(value.as_basic_value_enum())
    }

    pub fn pop1(&mut self) -> Result<BasicValueEnum, BinaryReaderError> {
        self.stack.pop().ok_or(BinaryReaderError {
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
            .ok_or(BinaryReaderError {
                message: "invalid value stack",
                offset: -1isize as usize,
            })
            .map(|v| *v)
    }

    pub fn peekn(&self, n: usize) -> Result<&[BasicValueEnum], BinaryReaderError> {
        self.stack
            .get(self.stack.len() - n..)
            .ok_or(BinaryReaderError {
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

    pub fn push_block(&mut self, end_label: BasicBlock, phis: SmallVec<[PhiValue; 1]>) {
        self.control_stack.push(ControlFrame::Block {
            end_label,
            phis,
            stack_size_snapshot: self.stack.len(),
        });
    }
}
