//! The unchanged v1 libc loader imports and their historical conventions.
use crate::{
    abi::Errno,
    errno::V1Result,
    shim::{self, Env, Frame},
};
use wasixv2_abi::{DlFlags, DlHandle, GuestPtrMut};
use wasixv2_exec::GuestDynamicLinker;
use wasixv2_syscalls::{Control, Syscalls, UserSlice, UserSliceMut};
use wasmer::RuntimeError;

pub(crate) struct Policy;
impl wasixv2_dylink::LoadPolicy for Policy {
    fn paths(
        &self,
        name: &[u8],
        search: Option<&[u8]>,
        runtime: &[String],
        origin: &[u8],
    ) -> Result<Vec<Vec<u8>>, String> {
        let candidates = if name.contains(&b'/') {
            vec![name.to_vec()]
        } else {
            let directory = origin
                .iter()
                .rposition(|b| *b == b'/')
                .map_or(&b"."[..], |index| &origin[..index]);
            let mut candidates = Vec::new();
            // v1 locator: environment, RUNPATH, then its system directories.
            for path in search
                .into_iter()
                .flat_map(|path| path.split(|b| *b == b':'))
            {
                let mut path = path.to_vec();
                if !path.is_empty() {
                    path.push(b'/');
                }
                path.extend_from_slice(name);
                candidates.push(path);
            }
            for path in runtime
                .iter()
                .flat_map(|path| path.as_bytes().split(|b| *b == b':'))
                .chain(b"/lib:/usr/lib:/usr/local/lib".split(|b| *b == b':'))
            {
                let mut path = if let Some(tail) = path
                    .strip_prefix(b"$ORIGIN")
                    .or_else(|| path.strip_prefix(b"${ORIGIN}"))
                {
                    if origin.is_empty() {
                        return Err("library guest origin is unavailable".into());
                    }
                    [directory, tail].concat()
                } else {
                    path.to_vec()
                };
                if !path.is_empty() {
                    path.push(b'/');
                }
                path.extend_from_slice(name);
                candidates.push(path);
            }
            candidates
        };
        let mut paths = Vec::new();
        for path in candidates {
            paths.push(path.clone());
            if let Some(stem) = path.strip_suffix(b".so") {
                paths.push([stem, b".wasm"].concat());
            }
        }
        Ok(paths)
    }
    fn scope(&self, handle: u32, count: usize, _: Vec<usize>) -> Option<Vec<usize>> {
        if handle == 0 {
            return Some((0..count).collect());
        }
        let index = handle.checked_sub(1)? as usize;
        (index < count).then_some(vec![index])
    }
    fn startup_search(&self, environment: Option<&[u8]>) -> Option<Vec<u8>> {
        environment.map(<[u8]>::to_vec)
    }
    fn open_runtime<'a>(&self, _: &'a [String]) -> &'a [String] {
        &[]
    }
    fn reuse_by_name(&self) -> bool {
        true
    }
    fn dependency_first_handles(&self) -> bool {
        true
    }
    fn main_tag(&self, module: &str, name: &str) -> Option<&'static [wasmer::Type]> {
        (module == "env" && matches!(name, "__c_longjmp" | "__cpp_exception"))
            .then_some(&[wasmer::Type::I32])
    }
    fn preloads(&self) -> bool {
        false
    }
    fn host_constructors(&self) -> bool {
        true
    }
    fn resident_handles(&self) -> bool {
        true
    }
    fn mapped_memory(&self) -> bool {
        false
    }
    fn memory_limit(&self) -> u64 {
        u64::from(crate::memory::SCRATCH_BASE)
    }
    fn main_global(&self, name: &str, stack: u32, end: u32) -> Option<u32> {
        match name {
            "__stack_high" => Some(stack),
            // The common loader's main_stack is 0..stack; protection is separate.
            "__stack_low" => Some(0),
            "__heap_base" => end.checked_add(15).map(|end| end & !15),
            _ => None,
        }
    }
}

async fn string<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    ptr: i32,
    len: i32,
) -> V1Result<wasixv2_abi::Cstr> {
    if len as u32 >= crate::memory::SCRATCH_SIZE {
        return Err(wasixv2_syscalls::Errno::ENOMEM.into());
    }
    let bytes = UserSlice::new(ptr as u32, len as u32)
        .copy_in(frame.mem.guest(), crate::memory::SCRATCH_SIZE as usize - 1)
        .await?;
    std::str::from_utf8(&bytes).map_err(|_| crate::errno::Error::V1(Errno::INVAL))?;
    // The native C-string boundary must never resolve a different prefix target.
    if bytes.contains(&0) {
        return shim::fail(Errno::INVAL);
    }
    Ok(frame.mem.alloc_cstr(&bytes)?)
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn dlopen<S: Syscalls + Control + 'static>(
    env: Env<S>,
    path: i32,
    path_len: i32,
    _flags: i32,
    err: i32,
    err_len: i32,
    search: i32,
    search_len: i32,
    handle: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let null_path = path == 0;
        let path = if null_path {
            wasixv2_abi::Cstr::new(0)
        } else {
            string(frame, path, path_len).await?
        };
        let search = if null_path {
            frame.mem.alloc_cstr(b"")?
        } else {
            string(frame, search, search_len).await?
        };
        let linker = GuestDynamicLinker::new_with_unwind(frame.env, frame.unwind());
        let caller = frame.caller().with_dynamic_linker(&linker);
        // Old dlopen accepted every flag but did not interpret any of them.
        let result = frame
            .syscalls
            .dl_open(
                &caller,
                path,
                DlFlags::RTLD_LAZY | DlFlags::RTLD_GLOBAL,
                search,
                0,
                GuestPtrMut::new(handle as u32),
                UserSliceMut::new(err as u32, err_len as u32),
            )
            .await?;
        if result < 0 {
            return shim::fail(Errno::UNKNOWN);
        }
        Ok(())
    })
    .await
}

pub(crate) async fn dlsym<S: Syscalls + Control + 'static>(
    env: Env<S>,
    handle: i32,
    name: i32,
    name_len: i32,
    err: i32,
    err_len: i32,
    output: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let name = string(frame, name, name_len).await?;
        let linker = GuestDynamicLinker::new(frame.env);
        let caller = frame.caller().with_dynamic_linker(&linker);
        let result = frame
            .syscalls
            .dl_sym(
                &caller,
                DlHandle(handle as u32),
                name,
                0,
                GuestPtrMut::new(output as u32),
                UserSliceMut::new(err as u32, err_len as u32),
            )
            .await?;
        if result < 0 {
            return shim::fail(Errno::UNKNOWN);
        }
        Ok(())
    })
    .await
}

pub(crate) async fn dl_invalid_handle<S: Syscalls + Control + 'static>(
    env: Env<S>,
    handle: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let valid = match frame.dynamic_handle_valid(DlHandle(handle as u32)).await {
            Ok(valid) => valid,
            Err(wasixv2_syscalls::SysErr::Errno(wasixv2_syscalls::Errno::ENOSYS)) => false,
            Err(error) => return Err(error.into()),
        };
        if !valid {
            return shim::fail(Errno::NOEXEC);
        }
        Ok(())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasixv2_dylink::LoadPolicy;

    #[test]
    fn legacy_paths_keep_environment_literal_and_expand_only_runpath_origin() {
        let runtime = vec!["${ORIGIN}/runtime".into()];
        let paths = Policy
            .paths(
                b"x.so",
                Some(b"$ORIGIN/env:/first"),
                &runtime,
                b"/plugins/source.so",
            )
            .unwrap();
        assert_eq!(
            &paths[..6],
            &[
                b"$ORIGIN/env/x.so".to_vec(),
                b"$ORIGIN/env/x.wasm".to_vec(),
                b"/first/x.so".to_vec(),
                b"/first/x.wasm".to_vec(),
                b"/plugins/runtime/x.so".to_vec(),
                b"/plugins/runtime/x.wasm".to_vec(),
            ]
        );
        assert!(Policy.paths(b"x.so", None, &runtime, b"").is_err());
        assert_eq!(
            Policy.paths(b"./x.so", None, &[], b"").unwrap(),
            [b"./x.so".to_vec(), b"./x.wasm".to_vec()]
        );
        assert_eq!(
            Policy.paths(b"x.so", Some(b""), &[], b"").unwrap()[0],
            b"x.so"
        );
    }

    #[test]
    fn old_default_and_explicit_handles_have_distinct_scopes() {
        assert_eq!(Policy.scope(0, 3, vec![0]), Some(vec![0, 1, 2]));
        assert_eq!(Policy.scope(1, 3, vec![0, 1, 2]), Some(vec![0]));
        assert_eq!(Policy.scope(2, 3, vec![1, 2]), Some(vec![1]));
        assert_eq!(Policy.scope(4, 3, vec![0]), None);
    }
}
