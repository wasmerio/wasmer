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

#[derive(Debug)]
pub struct ControlFrame {
    pub label: DynamicLabel,
    pub loop_like: bool,
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

#[derive(Copy, Clone, Debug)]
pub enum ValueLocation {
    Register(u8),
    Stack,
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
            ValueLocation::Register(x) => {
                if x >= self.num_regs - 1 {
                    ValueLocation::Stack
                } else {
                    ValueLocation::Register(x + 1)
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
            .unwrap_or(ValueLocation::Register(0));
        self.values.push(ValueInfo {
            ty: ty,
            location: loc,
        });
        loc
    }

    pub fn pop(&mut self) -> Option<ValueInfo> {
        self.values.pop()
    }

    pub fn pop2(&mut self) -> Option<(ValueInfo, ValueInfo)> {
        if self.values.len() < 2 {
            None
        } else {
            let v2 = self.values.pop().unwrap();
            let v1 = self.values.pop().unwrap();
            Some((v1, v2))
        }
    }

    pub fn peek(&self) -> Option<ValueInfo> {
        self.values.last().cloned()
    }

    pub fn reset_depth(&mut self, target_depth: usize) {
        self.values.truncate(target_depth);
    }
}
