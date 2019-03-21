use crate::codegen::CodegenError;
use dynasmrt::DynamicLabel;
use wasmparser::Type as WpType;

/*#[repr(u8)]
#[derive(Copy, Clone, Debug)]
pub enum RegisterName {
    RDI,
    RSI,
    RDX,
    RCX,
    R8,
    R9,
    R10,
    R11,
    RBX,
    R12,
    R13,
    R14,
    R15,
    Invalid,
}*/

#[derive(Debug, Copy, Clone)]
pub enum IfElseState {
    None,
    If(DynamicLabel),
    Else,
}

#[derive(Debug)]
pub struct ControlFrame {
    pub label: DynamicLabel,
    pub loop_like: bool,
    pub if_else: IfElseState,
    pub returns: Vec<WpType>,
    pub value_stack_depth_before: usize,
}

#[derive(Debug)]
pub struct ControlStack {
    pub frames: Vec<ControlFrame>,
}

#[derive(Debug)]
pub struct ValueStack {
    pub num_regs: u8,
    pub values: Vec<ValueInfo>,
}

#[derive(Copy, Clone, Debug)]
pub struct ValueInfo {
    pub ty: WpType,
    pub location: ValueLocation,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ValueLocation {
    Register(ScratchRegister),
    Stack,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ScratchRegister(u8);

impl ScratchRegister {
    pub fn raw_id(&self) -> u8 {
        self.0
    }
}

impl ValueLocation {
    pub fn is_register(&self) -> bool {
        if let ValueLocation::Register(_) = *self {
            true
        } else {
            false
        }
    }

    pub fn get_register(&self) -> Result<ScratchRegister, CodegenError> {
        if let ValueLocation::Register(id) = *self {
            Ok(id)
        } else {
            Err(CodegenError {
                message: "not a register location",
            })
        }
    }
}

impl ValueStack {
    pub fn new(num_regs: u8) -> ValueStack {
        ValueStack {
            num_regs: num_regs,
            values: vec![],
        }
    }

    fn next_location(&self, loc: &ValueLocation) -> ValueLocation {
        match *loc {
            ValueLocation::Register(ScratchRegister(x)) => {
                if x >= self.num_regs - 1 {
                    ValueLocation::Stack
                } else {
                    ValueLocation::Register(ScratchRegister(x + 1))
                }
            }
            ValueLocation::Stack => ValueLocation::Stack,
        }
    }

    pub fn push(&mut self, ty: WpType) -> ValueLocation {
        let loc = self
            .values
            .last()
            .map(|x| self.next_location(&x.location))
            .unwrap_or(ValueLocation::Register(ScratchRegister(0)));
        self.values.push(ValueInfo {
            ty: ty,
            location: loc,
        });
        loc
    }

    pub fn pop(&mut self) -> Result<ValueInfo, CodegenError> {
        match self.values.pop() {
            Some(x) => Ok(x),
            None => Err(CodegenError {
                message: "no value on top of stack",
            }),
        }
    }

    pub fn pop2(&mut self) -> Result<(ValueInfo, ValueInfo), CodegenError> {
        if self.values.len() < 2 {
            Err(CodegenError {
                message: "less than 2 values on top of stack",
            })
        } else {
            let v2 = self.values.pop().unwrap();
            let v1 = self.values.pop().unwrap();
            Ok((v1, v2))
        }
    }

    pub fn reset_depth(&mut self, target_depth: usize) {
        self.values.truncate(target_depth);
    }
}

impl ControlStack {
    pub fn new(label: DynamicLabel, returns: Vec<WpType>) -> ControlStack {
        ControlStack {
            frames: vec![ControlFrame {
                label: label,
                loop_like: false,
                if_else: IfElseState::None,
                returns: returns,
                value_stack_depth_before: 0,
            }],
        }
    }
}
