use inkwell::{
    basic_block::BasicBlock,
    values::{BasicValue, BasicValueEnum, PhiValue},
};
use smallvec::SmallVec;
use std::cell::Cell;
use wasmparser::BinaryReaderError;

#[derive(Debug)]
pub enum ControlFrame {
    Block {
        next: BasicBlock,
        phis: SmallVec<[PhiValue; 1]>,
        stack_size_snapshot: usize,
    },
    Loop {
        body: BasicBlock,
        next: BasicBlock,
        phis: SmallVec<[PhiValue; 1]>,
        stack_size_snapshot: usize,
    },
    IfElse {
        if_then: BasicBlock,
        if_else: BasicBlock,
        next: BasicBlock,
        phis: SmallVec<[PhiValue; 1]>,
        stack_size_snapshot: usize,
        if_else_state: IfElseState,
    },
}

#[derive(Debug)]
pub enum IfElseState {
    If,
    Else,
}

impl ControlFrame {
    pub fn code_after(&self) -> &BasicBlock {
        match self {
            ControlFrame::Block { ref next, .. }
            | ControlFrame::Loop { ref next, .. }
            | ControlFrame::IfElse { ref next, .. } => next,
        }
    }

    pub fn br_dest(&self) -> &BasicBlock {
        match self {
            ControlFrame::Block { ref next, .. } | ControlFrame::IfElse { ref next, .. } => next,
            ControlFrame::Loop { ref body, .. } => body,
        }
    }

    pub fn phis(&self) -> &[PhiValue] {
        match self {
            ControlFrame::Block { ref phis, .. }
            | ControlFrame::Loop { ref phis, .. }
            | ControlFrame::IfElse { ref phis, .. } => phis.as_slice(),
        }
    }

    pub fn is_loop(&self) -> bool {
        match self {
            ControlFrame::Loop { .. } => true,
            _ => false,
        }
    }
}

#[derive(Debug, Eq, PartialEq, Copy, Clone, Hash)]
pub enum ExtraInfo {
    None,

    // This values is required to be arithmetic 32-bit NaN (or 32x4) by the WAsm
    // machine, but which might not be in the LLVM value. The conversion to
    // arithmetic NaN is pending. It is required for correctness.
    PendingF32NaN,

    // This values is required to be arithmetic 64-bit NaN (or 64x2) by the WAsm
    // machine, but which might not be in the LLVM value. The conversion to
    // arithmetic NaN is pending. It is required for correctness.
    PendingF64NaN,
}
impl Default for ExtraInfo {
    fn default() -> Self {
        ExtraInfo::None
    }
}

#[derive(Debug)]
pub struct State {
    pub stack: Vec<(BasicValueEnum, ExtraInfo)>,
    control_stack: Vec<ControlFrame>,
    value_counter: Cell<usize>,

    pub reachable: bool,
}

impl State {
    pub fn new() -> Self {
        Self {
            stack: vec![],
            control_stack: vec![],
            value_counter: Cell::new(0),
            reachable: true,
        }
    }

    pub fn reset_stack(&mut self, frame: &ControlFrame) {
        let stack_size_snapshot = match frame {
            ControlFrame::Block {
                stack_size_snapshot,
                ..
            }
            | ControlFrame::Loop {
                stack_size_snapshot,
                ..
            }
            | ControlFrame::IfElse {
                stack_size_snapshot,
                ..
            } => *stack_size_snapshot,
        };
        self.stack.truncate(stack_size_snapshot);
    }

    pub fn outermost_frame(&self) -> Result<&ControlFrame, BinaryReaderError> {
        self.control_stack.get(0).ok_or(BinaryReaderError {
            message: "invalid control stack depth",
            offset: -1isize as usize,
        })
    }

    pub fn frame_at_depth(&self, depth: u32) -> Result<&ControlFrame, BinaryReaderError> {
        let index = self.control_stack.len() - 1 - (depth as usize);
        self.control_stack.get(index).ok_or(BinaryReaderError {
            message: "invalid control stack depth",
            offset: -1isize as usize,
        })
    }

    pub fn frame_at_depth_mut(
        &mut self,
        depth: u32,
    ) -> Result<&mut ControlFrame, BinaryReaderError> {
        let index = self.control_stack.len() - 1 - (depth as usize);
        self.control_stack.get_mut(index).ok_or(BinaryReaderError {
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

    pub fn var_name(&self) -> String {
        let counter = self.value_counter.get();
        let s = format!("s{}", counter);
        self.value_counter.set(counter + 1);
        s
    }

    pub fn push1<T: BasicValue>(&mut self, value: T) {
        self.push1_extra(value, ExtraInfo::None);
    }

    pub fn push1_extra<T: BasicValue>(&mut self, value: T, info: ExtraInfo) {
        self.stack.push((value.as_basic_value_enum(), info));
    }

    pub fn pop1(&mut self) -> Result<BasicValueEnum, BinaryReaderError> {
        Ok(self.pop1_extra()?.0)
    }

    pub fn pop1_extra(&mut self) -> Result<(BasicValueEnum, ExtraInfo), BinaryReaderError> {
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

    pub fn pop2_extra(
        &mut self,
    ) -> Result<((BasicValueEnum, ExtraInfo), (BasicValueEnum, ExtraInfo)), BinaryReaderError> {
        let v2 = self.pop1_extra()?;
        let v1 = self.pop1_extra()?;
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

    pub fn pop3_extra(
        &mut self,
    ) -> Result<
        (
            (BasicValueEnum, ExtraInfo),
            (BasicValueEnum, ExtraInfo),
            (BasicValueEnum, ExtraInfo),
        ),
        BinaryReaderError,
    > {
        let v3 = self.pop1_extra()?;
        let v2 = self.pop1_extra()?;
        let v1 = self.pop1_extra()?;
        Ok((v1, v2, v3))
    }

    pub fn peek1_extra(&self) -> Result<(BasicValueEnum, ExtraInfo), BinaryReaderError> {
        self.stack
            .get(self.stack.len() - 1)
            .ok_or(BinaryReaderError {
                message: "invalid value stack",
                offset: -1isize as usize,
            })
            .map(|v| *v)
    }

    pub fn peekn(&self, n: usize) -> Result<Vec<BasicValueEnum>, BinaryReaderError> {
        Ok(self.peekn_extra(n)?.iter().map(|x| x.0).collect())
    }

    pub fn peekn_extra(
        &self,
        n: usize,
    ) -> Result<&[(BasicValueEnum, ExtraInfo)], BinaryReaderError> {
        self.stack
            .get(self.stack.len() - n..)
            .ok_or(BinaryReaderError {
                message: "invalid value stack",
                offset: -1isize as usize,
            })
    }

    pub fn popn_save_extra(
        &mut self,
        n: usize,
    ) -> Result<Vec<(BasicValueEnum, ExtraInfo)>, BinaryReaderError> {
        let v = self.peekn_extra(n)?.to_vec();
        self.popn(n)?;
        Ok(v)
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

    pub fn push_block(&mut self, next: BasicBlock, phis: SmallVec<[PhiValue; 1]>) {
        self.control_stack.push(ControlFrame::Block {
            next,
            phis,
            stack_size_snapshot: self.stack.len(),
        });
    }

    pub fn push_loop(&mut self, body: BasicBlock, next: BasicBlock, phis: SmallVec<[PhiValue; 1]>) {
        self.control_stack.push(ControlFrame::Loop {
            body,
            next,
            phis,
            stack_size_snapshot: self.stack.len(),
        });
    }

    pub fn push_if(
        &mut self,
        if_then: BasicBlock,
        if_else: BasicBlock,
        next: BasicBlock,
        phis: SmallVec<[PhiValue; 1]>,
    ) {
        self.control_stack.push(ControlFrame::IfElse {
            if_then,
            if_else,
            next,
            phis,
            stack_size_snapshot: self.stack.len(),
            if_else_state: IfElseState::If,
        });
    }
}
