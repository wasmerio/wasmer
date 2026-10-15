//! The initial process block `startup_get` writes: what Linux puts on a new process's stack.

use wasixv2_abi::AuxvType;
use wasixv2_syscalls::Errno;

use crate::{ID, mm::PAGE_SIZE};

const WORD: usize = 4;
const RANDOM_LEN: usize = 16;
/// `sysconf(_SC_CLK_TCK)`, as on every Linux architecture musl supports.
const CLOCK_TICKS: u32 = 100;

/// The auxiliary vector, except `AT_RANDOM`, whose value is an address in the block, and the
/// terminating `AT_NULL`.
const AUXV: &[(AuxvType, u32)] = &[
    (AuxvType::AT_PAGESZ, PAGE_SIZE as u32),
    (AuxvType::AT_CLKTCK, CLOCK_TICKS),
    (AuxvType::AT_HWCAP, 0),
    (AuxvType::AT_UID, ID as u32),
    (AuxvType::AT_EUID, ID as u32),
    (AuxvType::AT_GID, ID as u32),
    (AuxvType::AT_EGID, ID as u32),
    (AuxvType::AT_SECURE, 0),
];

/// Lays out argc, argv, envp, auxv, the `AT_RANDOM` bytes and the strings.
pub(crate) struct Startup<'a> {
    args: &'a [String],
    env: &'a [String],
}

impl<'a> Startup<'a> {
    pub(crate) fn new(args: &'a [String], env: &'a [String]) -> Self {
        Self { args, env }
    }

    fn header_len(&self) -> usize {
        let pointers = 1 + self.args.len() + 1 + self.env.len() + 1;
        let auxv = (AUXV.len() + 2) * 2;
        (pointers + auxv) * WORD
    }

    fn strings(&self) -> impl Iterator<Item = &'a String> {
        self.args.iter().chain(self.env)
    }

    /// Size of the block, a multiple of 16; `E2BIG` if it does not fit the address space.
    pub(crate) fn size(&self) -> Result<u32, Errno> {
        let strings: usize = self.strings().map(|s| s.len() + 1).sum();
        let size = (self.header_len() + RANDOM_LEN + strings).next_multiple_of(16);
        u32::try_from(size).map_err(|_| Errno::E2BIG)
    }

    /// The block as it must appear at guest address `base`.
    pub(crate) fn build(&self, base: u32, random: [u8; RANDOM_LEN]) -> Result<Vec<u8>, Errno> {
        let mut block = vec![0; self.size()? as usize];
        let random_at = self.header_len();
        block[random_at..][..RANDOM_LEN].copy_from_slice(&random);

        let mut words = Vec::new();
        words.push(self.args.len() as u32);
        let mut string_at = random_at + RANDOM_LEN;
        for list in [self.args, self.env] {
            for s in list {
                words.push(base + string_at as u32);
                block[string_at..][..s.len()].copy_from_slice(s.as_bytes());
                string_at += s.len() + 1;
            }
            words.push(0);
        }
        for &(key, value) in AUXV {
            words.extend([key.0, value]);
        }
        words.extend([AuxvType::AT_RANDOM.0, base + random_at as u32]);
        words.extend([AuxvType::AT_NULL.0, 0]);

        for (word, out) in words.iter().zip(block.chunks_exact_mut(WORD)) {
            out.copy_from_slice(&word.to_le_bytes());
        }
        Ok(block)
    }
}
