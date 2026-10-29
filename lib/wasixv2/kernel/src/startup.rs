//! The initial process block `startup_get` writes: what Linux puts on a new process's stack.

use wasixv2_abi::AuxvType;
use wasixv2_syscalls::Errno;

use crate::{ident::ExecAux, mm::PAGE_SIZE};

const WORD: usize = 4;
const RANDOM_LEN: usize = 16;
/// `sysconf(_SC_CLK_TCK)`, as on every Linux architecture musl supports.
const CLOCK_TICKS: u32 = 100;

/// The fixed part of the auxiliary vector. The identity entries, `AT_RANDOM`, whose value is an
/// address in the block, and the terminating `AT_NULL` follow it.
const AUXV: &[(AuxvType, u32)] = &[
    (AuxvType::AT_PAGESZ, PAGE_SIZE as u32),
    (AuxvType::AT_CLKTCK, CLOCK_TICKS),
    (AuxvType::AT_HWCAP, 0),
    (
        AuxvType::AT_RSEQ_FEATURE_SIZE,
        crate::rseq::RSEQ_FEATURE_SIZE,
    ),
    (AuxvType::AT_RSEQ_ALIGN, crate::rseq::RSEQ_ALIGN),
];
/// The identity entries, `AT_FLAGS`, the random bytes and `AT_NULL`.
const AUXV_EXTRA: usize = 8;
/// The wasm architecture's fixed mm->saved_auxv storage (kernel/sys.c:prctl_get_auxv).
pub(crate) const SAVED_AUXV_SIZE: usize = (AUXV.len() + AUXV_EXTRA) * 2 * WORD;

/// Who the process runs as: `AT_UID`, `AT_EUID`, `AT_GID`, `AT_EGID` and `AT_SECURE`.
#[derive(Clone, Copy)]
pub(crate) struct Identity {
    pub(crate) uid: u32,
    pub(crate) euid: u32,
    pub(crate) gid: u32,
    pub(crate) egid: u32,
    pub(crate) secure: bool,
}

/// Lays out argc, argv, envp, auxv, the `AT_RANDOM` bytes and the strings.
pub(crate) struct Startup<'a> {
    args: &'a [Vec<u8>],
    env: &'a [Vec<u8>],
    identity: Identity,
    exec: ExecAux,
}

impl<'a> Startup<'a> {
    pub(crate) fn new(
        args: &'a [Vec<u8>],
        env: &'a [Vec<u8>],
        identity: Identity,
        exec: ExecAux,
    ) -> Self {
        Self {
            args,
            env,
            identity,
            exec,
        }
    }

    fn header_len(&self) -> usize {
        let pointers = 1 + self.args.len() + 1 + self.env.len() + 1;
        let execfd = usize::from(self.exec.execfd.is_some());
        let auxv = (AUXV.len() + AUXV_EXTRA + execfd) * 2;
        (pointers + auxv) * WORD
    }

    fn strings(&self) -> impl Iterator<Item = &'a Vec<u8>> {
        self.args.iter().chain(self.env)
    }

    /// Size of the block, a multiple of 16; `E2BIG` if it does not fit the address space.
    pub(crate) fn size(&self) -> Result<u32, Errno> {
        let strings: usize = self.strings().map(|s| s.len() + 1).sum();
        let size = (self.header_len() + RANDOM_LEN + strings).next_multiple_of(16);
        u32::try_from(size).map_err(|_| Errno::E2BIG)
    }

    /// The auxiliary vector of `block`, which [`Startup::build`] made: its pairs of words up to
    /// and with `AT_NULL`.
    pub(crate) fn auxv(&self, block: &[u8]) -> Vec<u8> {
        let pointers = 1 + self.args.len() + 1 + self.env.len() + 1;
        let start = pointers * WORD;
        let len = (AUXV.len() + AUXV_EXTRA) * 2 * WORD;
        block.get(start..start + len).unwrap_or_default().to_vec()
    }

    /// The block as it must appear at guest address `base`.
    pub(crate) fn build(&self, base: u32, random: [u8; RANDOM_LEN]) -> Result<Vec<u8>, Errno> {
        // A block that would wrap past the end of the address space is a bad pointer.
        base.checked_add(self.size()?).ok_or(Errno::EFAULT)?;
        let mut block = vec![0; self.size()? as usize];
        let random_at = self.header_len();
        block[random_at..][..RANDOM_LEN].copy_from_slice(&random);

        let mut words = Vec::new();
        words.push(self.args.len() as u32);
        let mut string_at = random_at + RANDOM_LEN;
        for list in [self.args, self.env] {
            for s in list {
                words.push(base + string_at as u32);
                block[string_at..][..s.len()].copy_from_slice(s);
                string_at += s.len() + 1;
            }
            words.push(0);
        }
        for &(key, value) in AUXV {
            words.extend([key.0, value]);
        }
        let id = self.identity;
        words.extend([AuxvType::AT_UID.0, id.uid, AuxvType::AT_EUID.0, id.euid]);
        words.extend([AuxvType::AT_GID.0, id.gid, AuxvType::AT_EGID.0, id.egid]);
        words.extend([AuxvType::AT_SECURE.0, id.secure as u32]);
        words.extend([AuxvType::AT_FLAGS.0, self.exec.flags]);
        words.extend([AuxvType::AT_RANDOM.0, base + random_at as u32]);
        // `create_elf_tables`: the binary binfmt_misc handed over.
        if let Some(fd) = self.exec.execfd {
            words.extend([AuxvType::AT_EXECFD.0, fd]);
        }
        words.extend([AuxvType::AT_NULL.0, 0]);

        for (word, out) in words.iter().zip(block.chunks_exact_mut(WORD)) {
            out.copy_from_slice(&word.to_le_bytes());
        }
        Ok(block)
    }
}
