//! Guest ABI conventions, separate from graph publication and store ownership.
use super::*;

/// Conventions supplied by an ABI composing the common dynamic loader.
pub trait LoadPolicy: Send + Sync + 'static {
    /// Candidate guest VFS paths, in search order. `origin` is the successful
    /// guest path of the requesting image, never a host filesystem path.
    fn paths(
        &self,
        name: &[u8],
        search: Option<&[u8]>,
        runtime: &[String],
        origin: &[u8],
    ) -> std::result::Result<Vec<Vec<u8>>, String>;

    /// The module indices searched by this ABI's public symbol handle.
    fn scope(&self, handle: u32, count: usize, native: Vec<usize>) -> Option<Vec<usize>>;

    /// Startup environment policy; `None` distinguishes no variable from an
    /// explicitly empty search path in guest ABIs that give it meaning.
    fn startup_search(&self, environment: Option<&[u8]>) -> Option<Vec<u8>> {
        let mut search = Vec::new();
        if let Some(path) = environment.filter(|path| !path.is_empty()) {
            search.extend_from_slice(path);
            search.push(b':');
        }
        search.extend_from_slice(b"/lib:/usr/local/lib:/usr/lib");
        Some(search)
    }
    fn open_runtime<'a>(&self, runtime: &'a [String]) -> &'a [String] {
        runtime
    }
    fn preloads(&self) -> bool {
        true
    }
    /// Whether the unchanged guest needs the host to call side constructors.
    fn host_constructors(&self) -> bool {
        false
    }
    /// Resident handles remain valid independently of native reference counts.
    fn resident_handles(&self) -> bool {
        false
    }
    /// Whether a request's exact guest name, rather than inode identity, is its
    /// resident-library key. Independent from native global/local scopes.
    fn reuse_by_name(&self) -> bool {
        false
    }
    /// Number newly loaded dependencies before their request root.
    fn dependency_first_handles(&self) -> bool {
        false
    }
    /// Missing infrastructure tags this ABI explicitly supplies to its main.
    fn main_tag(&self, _module: &str, _name: &str) -> Option<&'static [Type]> {
        None
    }
    /// Whether the ABI uses explicit Linux heap/VMA layout rather than a
    /// linear memory.grow heap. Existing native images use the mapped layout.
    fn mapped_memory(&self) -> bool {
        true
    }
    fn memory_limit(&self) -> u64 {
        1 << 32
    }
    /// ABI-owned absolute infrastructure globals, if any.
    fn main_global(&self, _name: &str, _stack: u32, _data_end: u32) -> Option<u32> {
        None
    }
}

pub(super) struct NativePolicy;
impl LoadPolicy for NativePolicy {
    fn paths(
        &self,
        name: &[u8],
        search: Option<&[u8]>,
        runtime: &[String],
        _: &[u8],
    ) -> std::result::Result<Vec<Vec<u8>>, String> {
        if name.contains(&b'/') {
            return Ok(vec![name.to_vec()]);
        }
        runtime
            .iter()
            .flat_map(|p| p.as_bytes().split(|b| *b == b':'))
            .chain(search.unwrap_or_default().split(|b| *b == b':'))
            .map(|path| {
                if path.contains(&b'$') {
                    return Err("runtime path expansion is unsupported".into());
                }
                let mut candidate = path.to_vec();
                if !candidate.is_empty() {
                    candidate.push(b'/');
                }
                candidate.extend_from_slice(name);
                Ok(candidate)
            })
            .collect()
    }
    fn scope(&self, handle: u32, count: usize, native: Vec<usize>) -> Option<Vec<usize>> {
        handle
            .checked_sub(1)
            .filter(|index| (*index as usize) < count)?;
        Some(native)
    }
}

/// Compose a prepared dynamic main with an ordinary imports/bind ABI, such as
/// Wasix1. The base must not have an independent pre-instantiation loader. Bound
/// entry, signal, return and teardown behavior stay with the base instance.
pub fn compose<S: Syscalls + Control + Send + Sync + 'static>(
    module: &Module,
    base: Arc<dyn Abi<S>>,
    policy: Arc<dyn LoadPolicy>,
) -> Option<Arc<dyn Abi<S>>> {
    let source = module.custom_sections(SOURCE_SECTION).next()?.to_vec();
    let root = Root::with_policy(source, policy);
    if let Err(error) = verify_compiled_source(module) {
        let _ = root.error.set(error.to_string());
    }
    Some(Arc::new(Image { root, base }))
}
