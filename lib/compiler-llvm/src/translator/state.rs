use inkwell::{
    basic_block::BasicBlock,
    values::{BasicValue, BasicValueEnum, PhiValue},
};
use smallvec::SmallVec;
use std::ops::{BitAnd, BitOr, BitOrAssign};
use wasmer_types::CompileError;

#[derive(Debug)]
pub enum ControlFrame<'ctx> {
    Block {
        next: BasicBlock<'ctx>,
        phis: SmallVec<[PhiValue<'ctx>; 1]>,
        stack_size_snapshot: usize,
    },
    Loop {
        body: BasicBlock<'ctx>,
        next: BasicBlock<'ctx>,
        phis: SmallVec<[PhiValue<'ctx>; 1]>,
        loop_body_phis: SmallVec<[PhiValue<'ctx>; 1]>,
        stack_size_snapshot: usize,
    },
    IfElse {
        if_then: BasicBlock<'ctx>,
        if_else: BasicBlock<'ctx>,
        next: BasicBlock<'ctx>,
        then_phis: SmallVec<[PhiValue<'ctx>; 1]>,
        else_phis: SmallVec<[PhiValue<'ctx>; 1]>,
        next_phis: SmallVec<[PhiValue<'ctx>; 1]>,
        stack_size_snapshot: usize,
        if_else_state: IfElseState,
    },
}

#[derive(Debug)]
pub enum IfElseState {
    If,
    Else,
}

impl<'ctx> ControlFrame<'ctx> {
    pub fn code_after(&self) -> &BasicBlock<'ctx> {
        match self {
            ControlFrame::Block { ref next, .. }
            | ControlFrame::Loop { ref next, .. }
            | ControlFrame::IfElse { ref next, .. } => next,
        }
    }

    pub fn br_dest(&self) -> &BasicBlock<'ctx> {
        match self {
            ControlFrame::Block { ref next, .. } | ControlFrame::IfElse { ref next, .. } => next,
            ControlFrame::Loop { ref body, .. } => body,
        }
    }

    pub fn phis(&self) -> &[PhiValue<'ctx>] {
        match self {
            ControlFrame::Block { ref phis, .. } | ControlFrame::Loop { ref phis, .. } => {
                phis.as_slice()
            }
            ControlFrame::IfElse { ref next_phis, .. } => next_phis.as_slice(),
        }
    }

    /// PHI nodes for stack values in the loop body.
    pub fn loop_body_phis(&self) -> &[PhiValue<'ctx>] {
        match self {
            ControlFrame::Block { .. } | ControlFrame::IfElse { .. } => &[],
            ControlFrame::Loop {
                ref loop_body_phis, ..
            } => loop_body_phis.as_slice(),
        }
    }

    pub fn is_loop(&self) -> bool {
        matches!(self, ControlFrame::Loop { .. })
    }
}

#[derive(Debug, Default, Eq, PartialEq, Copy, Clone, Hash)]
pub struct ExtraInfo {
    state: u8,
}
impl ExtraInfo {
    // This value is required to be arithmetic 32-bit NaN (or 32x4) by the WAsm
    // machine, but which might not be in the LLVM value. The conversion to
    // arithmetic NaN is pending. It is required for correctness.
    //
    // When applied to a 64-bit value, this flag has no meaning and must be
    // ignored. It may be set in such cases to allow for common handling of
    // 32 and 64-bit operations.
    pub const fn pending_f32_nan() -> ExtraInfo {
        ExtraInfo { state: 1 }
    }

    // This value is required to be arithmetic 64-bit NaN (or 64x2) by the WAsm
    // machine, but which might not be in the LLVM value. The conversion to
    // arithmetic NaN is pending. It is required for correctness.
    //
    // When applied to a 32-bit value, this flag has no meaning and must be
    // ignored. It may be set in such cases to allow for common handling of
    // 32 and 64-bit operations.
    pub const fn pending_f64_nan() -> ExtraInfo {
        ExtraInfo { state: 2 }
    }

    // This value either does not contain a 32-bit NaN, or it contains an
    // arithmetic NaN. In SIMD, applies to all 4 lanes.
    pub const fn arithmetic_f32() -> ExtraInfo {
        ExtraInfo { state: 4 }
    }

    // This value either does not contain a 64-bit NaN, or it contains an
    // arithmetic NaN. In SIMD, applies to both lanes.
    pub const fn arithmetic_f64() -> ExtraInfo {
        ExtraInfo { state: 8 }
    }

    pub const fn has_pending_f32_nan(&self) -> bool {
        self.state & ExtraInfo::pending_f32_nan().state != 0
    }
    pub const fn has_pending_f64_nan(&self) -> bool {
        self.state & ExtraInfo::pending_f64_nan().state != 0
    }
    pub const fn is_arithmetic_f32(&self) -> bool {
        self.state & ExtraInfo::arithmetic_f32().state != 0
    }
    pub const fn is_arithmetic_f64(&self) -> bool {
        self.state & ExtraInfo::arithmetic_f64().state != 0
    }

    pub const fn strip_pending(&self) -> ExtraInfo {
        ExtraInfo {
            state: self.state
                & !(ExtraInfo::pending_f32_nan().state | ExtraInfo::pending_f64_nan().state),
        }
    }
}

// Union two ExtraInfos.
impl BitOr for ExtraInfo {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        debug_assert!(!(self.has_pending_f32_nan() && other.has_pending_f64_nan()));
        debug_assert!(!(self.has_pending_f64_nan() && other.has_pending_f32_nan()));
        ExtraInfo {
            state: if self.is_arithmetic_f32() || other.is_arithmetic_f32() {
                ExtraInfo::arithmetic_f32().state
            } else if self.has_pending_f32_nan() || other.has_pending_f32_nan() {
                ExtraInfo::pending_f32_nan().state
            } else {
                0
            } + if self.is_arithmetic_f64() || other.is_arithmetic_f64() {
                ExtraInfo::arithmetic_f64().state
            } else if self.has_pending_f64_nan() || other.has_pending_f64_nan() {
                ExtraInfo::pending_f64_nan().state
            } else {
                0
            },
        }
    }
}
impl BitOrAssign for ExtraInfo {
    fn bitor_assign(&mut self, other: Self) {
        *self = *self | other;
    }
}

// Intersection for ExtraInfo.
impl BitAnd for ExtraInfo {
    type Output = Self;
    fn bitand(self, other: Self) -> Self {
        // Pending canonicalizations are not safe to discard, or even reorder.
        debug_assert!(
            self.has_pending_f32_nan() == other.has_pending_f32_nan()
                || self.is_arithmetic_f32()
                || other.is_arithmetic_f32()
        );
        debug_assert!(
            self.has_pending_f64_nan() == other.has_pending_f64_nan()
                || self.is_arithmetic_f64()
                || other.is_arithmetic_f64()
        );
        let info = match (
            self.is_arithmetic_f32() && other.is_arithmetic_f32(),
            self.is_arithmetic_f64() && other.is_arithmetic_f64(),
        ) {
            (false, false) => Default::default(),
            (true, false) => ExtraInfo::arithmetic_f32(),
            (false, true) => ExtraInfo::arithmetic_f64(),
            (true, true) => ExtraInfo::arithmetic_f32() | ExtraInfo::arithmetic_f64(),
        };
        match (self.has_pending_f32_nan(), self.has_pending_f64_nan()) {
            (false, false) => info,
            (true, false) => info | ExtraInfo::pending_f32_nan(),
            (false, true) => info | ExtraInfo::pending_f64_nan(),
            (true, true) => unreachable!("Can't form ExtraInfo with two pending canonicalizations"),
        }
    }
}

#[derive(Debug)]
pub struct State<'ctx> {
    pub stack: Vec<(BasicValueEnum<'ctx>, ExtraInfo)>,
    control_stack: Vec<ControlFrame<'ctx>>,

    pub reachable: bool,
}

impl<'ctx> State<'ctx> {
    pub fn new() -> Self {
        Self {
            stack: vec![],
            control_stack: vec![],
            reachable: true,
        }
    }

    pub fn has_control_frames(&self) -> bool {
        !self.control_stack.is_empty()
    }

    pub fn reset_stack(&mut self, frame: &ControlFrame<'ctx>) {
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

    pub fn outermost_frame(&self) -> Result<&ControlFrame<'ctx>, CompileError> {
        self.control_stack.get(0).ok_or_else(|| {
            CompileError::Codegen("outermost_frame: invalid control stack depth".to_string())
        })
    }

    pub fn frame_at_depth(&self, depth: u32) -> Result<&ControlFrame<'ctx>, CompileError> {
        let index = self
            .control_stack
            .len()
            .checked_sub(1 + (depth as usize))
            .ok_or_else(|| {
                CompileError::Codegen("frame_at_depth: invalid control stack depth".to_string())
            })?;
        Ok(&self.control_stack[index])
    }

    pub fn frame_at_depth_mut(
        &mut self,
        depth: u32,
    ) -> Result<&mut ControlFrame<'ctx>, CompileError> {
        let index = self
            .control_stack
            .len()
            .checked_sub(1 + (depth as usize))
            .ok_or_else(|| {
                CompileError::Codegen("frame_at_depth_mut: invalid control stack depth".to_string())
            })?;
        Ok(&mut self.control_stack[index])
    }

    pub fn pop_frame(&mut self) -> Result<ControlFrame<'ctx>, CompileError> {
        self.control_stack.pop().ok_or_else(|| {
            CompileError::Codegen("pop_frame: cannot pop from control stack".to_string())
        })
    }

    pub fn push1<T: BasicValue<'ctx>>(&mut self, value: T) {
        self.push1_extra(value, Default::default());
    }

    pub fn push1_extra<T: BasicValue<'ctx>>(&mut self, value: T, info: ExtraInfo) {
        self.stack.push((value.as_basic_value_enum(), info));
    }

    pub fn pop1(&mut self) -> Result<BasicValueEnum<'ctx>, CompileError> {
        Ok(self.pop1_extra()?.0)
    }

    pub fn pop1_extra(&mut self) -> Result<(BasicValueEnum<'ctx>, ExtraInfo), CompileError> {
        self.stack
            .pop()
            .ok_or_else(|| CompileError::Codegen("pop1_extra: invalid value stack".to_string()))
    }

    pub fn pop2(&mut self) -> Result<(BasicValueEnum<'ctx>, BasicValueEnum<'ctx>), CompileError> {
        let v2 = self.pop1()?;
        let v1 = self.pop1()?;
        Ok((v1, v2))
    }

    #[allow(clippy::type_complexity)]
    pub fn pop2_extra(
        &mut self,
    ) -> Result<
        (
            (BasicValueEnum<'ctx>, ExtraInfo),
            (BasicValueEnum<'ctx>, ExtraInfo),
        ),
        CompileError,
    > {
        let v2 = self.pop1_extra()?;
        let v1 = self.pop1_extra()?;
        Ok((v1, v2))
    }

    pub fn pop3(
        &mut self,
    ) -> Result<
        (
            BasicValueEnum<'ctx>,
            BasicValueEnum<'ctx>,
            BasicValueEnum<'ctx>,
        ),
        CompileError,
    > {
        let v3 = self.pop1()?;
        let v2 = self.pop1()?;
        let v1 = self.pop1()?;
        Ok((v1, v2, v3))
    }

    #[allow(clippy::type_complexity)]
    pub fn pop3_extra(
        &mut self,
    ) -> Result<
        (
            (BasicValueEnum<'ctx>, ExtraInfo),
            (BasicValueEnum<'ctx>, ExtraInfo),
            (BasicValueEnum<'ctx>, ExtraInfo),
        ),
        CompileError,
    > {
        let v3 = self.pop1_extra()?;
        let v2 = self.pop1_extra()?;
        let v1 = self.pop1_extra()?;
        Ok((v1, v2, v3))
    }

    pub fn peek1_extra(&self) -> Result<(BasicValueEnum<'ctx>, ExtraInfo), CompileError> {
        let index =
            self.stack.len().checked_sub(1).ok_or_else(|| {
                CompileError::Codegen("peek1_extra: invalid value stack".to_string())
            })?;
        Ok(self.stack[index])
    }

    pub fn peekn(&self, n: usize) -> Result<Vec<BasicValueEnum<'ctx>>, CompileError> {
        Ok(self.peekn_extra(n)?.iter().map(|x| x.0).collect())
    }

    pub fn peekn_extra(
        &self,
        n: usize,
    ) -> Result<&[(BasicValueEnum<'ctx>, ExtraInfo)], CompileError> {
        let index =
            self.stack.len().checked_sub(n).ok_or_else(|| {
                CompileError::Codegen("peekn_extra: invalid value stack".to_string())
            })?;
        Ok(&self.stack[index..])
    }

    pub fn popn_save_extra(
        &mut self,
        n: usize,
    ) -> Result<Vec<(BasicValueEnum<'ctx>, ExtraInfo)>, CompileError> {
        let v = self.peekn_extra(n)?.to_vec();
        self.popn(n)?;
        Ok(v)
    }

    pub fn popn(&mut self, n: usize) -> Result<(), CompileError> {
        let index = self
            .stack
            .len()
            .checked_sub(n)
            .ok_or_else(|| CompileError::Codegen("popn: invalid value stack".to_string()))?;

        self.stack.truncate(index);
        Ok(())
    }

    pub fn push_block(&mut self, next: BasicBlock<'ctx>, phis: SmallVec<[PhiValue<'ctx>; 1]>) {
        self.control_stack.push(ControlFrame::Block {
            next,
            phis,
            stack_size_snapshot: self.stack.len(),
        });
    }

    pub fn push_loop(
        &mut self,
        body: BasicBlock<'ctx>,
        next: BasicBlock<'ctx>,
        loop_body_phis: SmallVec<[PhiValue<'ctx>; 1]>,
        phis: SmallVec<[PhiValue<'ctx>; 1]>,
    ) {
        self.control_stack.push(ControlFrame::Loop {
            body,
            next,
            loop_body_phis,
            phis,
            stack_size_snapshot: self.stack.len(),
        });
    }

    pub fn push_if(
        &mut self,
        if_then: BasicBlock<'ctx>,
        if_else: BasicBlock<'ctx>,
        next: BasicBlock<'ctx>,
        then_phis: SmallVec<[PhiValue<'ctx>; 1]>,
        else_phis: SmallVec<[PhiValue<'ctx>; 1]>,
        next_phis: SmallVec<[PhiValue<'ctx>; 1]>,
    ) {
        self.control_stack.push(ControlFrame::IfElse {
            if_then,
            if_else,
            next,
            then_phis,
            else_phis,
            next_phis,
            stack_size_snapshot: self.stack.len(),
            if_else_state: IfElseState::If,
        });
    }
}
