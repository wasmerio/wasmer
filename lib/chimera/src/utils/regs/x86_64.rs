#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Reg {
    // General purpose registers.
    Rax = 0,
    Rdx = 1,
    Rcx = 2,
    Rbx = 3,
    Rsi = 4,
    Rdi = 5,

    // Frame pointer register.
    Rbp = 6,
    // Stack pointer register.
    Rsp = 7,

    // Extended integer registers.
    R8 = 8,
    R9 = 9,
    R10 = 10,
    R11 = 11,
    R12 = 12,
    R13 = 13,
    R14 = 14,
    R15 = 15,

    // This isn't actually a register, but is `[rsp + 0]`.
    RetAddr = 16,

    // SSE registers.
    Xmm0 = 17,
    Xmm1 = 18,
    Xmm2 = 19,
    Xmm3 = 20,
    Xmm4 = 21,
    Xmm5 = 22,
    Xmm6 = 23,
    Xmm7 = 24,

    // Extended SSE registers.
    Xmm8 = 25,
    Xmm9 = 26,
    Xmm10 = 27,
    Xmm11 = 28,
    Xmm12 = 29,
    Xmm13 = 30,
    Xmm14 = 31,
    Xmm15 = 32,

    // Floating point registers.
    St0 = 33,
    St1 = 34,
    St2 = 35,
    St3 = 36,
    St4 = 37,
    St5 = 38,
    St6 = 39,
    St7 = 40,

    // MMX registers.
    Mm0 = 41,
    Mm1 = 42,
    Mm2 = 43,
    Mm3 = 44,
    Mm4 = 45,
    Mm5 = 46,
    Mm6 = 47,
    Mm7 = 48,
}

impl From<u16> for Reg {
    fn from(regnum: u16) -> Reg {
        match regnum {
            0 => Reg::Rax,
            1 => Reg::Rdx,
            2 => Reg::Rcx,
            3 => Reg::Rbx,
            4 => Reg::Rsi,
            5 => Reg::Rdi,
            6 => Reg::Rbp,
            7 => Reg::Rsp,
            8 => Reg::R8,
            9 => Reg::R9,
            10 => Reg::R10,
            11 => Reg::R11,
            12 => Reg::R12,
            13 => Reg::R13,
            14 => Reg::R14,
            15 => Reg::R15,
            16 => Reg::RetAddr,
            17 => Reg::Xmm0,
            18 => Reg::Xmm1,
            19 => Reg::Xmm2,
            20 => Reg::Xmm3,
            21 => Reg::Xmm4,
            22 => Reg::Xmm5,
            23 => Reg::Xmm6,
            24 => Reg::Xmm7,
            25 => Reg::Xmm8,
            26 => Reg::Xmm9,
            27 => Reg::Xmm10,
            28 => Reg::Xmm11,
            29 => Reg::Xmm12,
            30 => Reg::Xmm13,
            31 => Reg::Xmm14,
            32 => Reg::Xmm15,
            33 => Reg::St0,
            34 => Reg::St1,
            35 => Reg::St2,
            36 => Reg::St3,
            37 => Reg::St4,
            38 => Reg::St5,
            39 => Reg::St6,
            40 => Reg::St7,
            41 => Reg::Mm0,
            42 => Reg::Mm1,
            43 => Reg::Mm2,
            44 => Reg::Mm3,
            45 => Reg::Mm4,
            46 => Reg::Mm5,
            47 => Reg::Mm6,
            48 => Reg::Mm7,
            _ => unreachable!(),
        }
    }
}
