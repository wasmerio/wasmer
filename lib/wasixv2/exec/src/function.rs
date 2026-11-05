//! Function operations on the calling store, through ordinary syscall dispatch.

use wasixv2_abi::{CallDynamicFlags, GuestPtrMut, ReflectionResult, U32Ptr, Uptr};
use wasixv2_syscalls::{
    Caller, Errno, FunctionOperations, LocalFuture, RawGuestBuffer, RawGuestPointer, SysErr,
    SysResult, UserSlice, write_value,
};
use wasmer::{AsyncFunctionEnvMut, Function, FunctionType, Type, Value};

use crate::{AbiInstance, ExecEnv, GuestUnwind};

/// Source-backed table metadata bound to one store. Custom ABIs may leave this absent.
#[derive(Default)]
pub struct TableMetadata {
    table: Option<wasmer::Table>,
    cacheable: bool,
}

impl TableMetadata {
    pub fn bind(instance: &wasmer::Instance, store: &mut wasmer::Store) -> Self {
        let table = instance
            .exports
            .get_table("__wasix_exec_table")
            .or_else(|_| instance.exports.get_table("__indirect_function_table"))
            .or_else(|_| instance.exports.get_table("__wasix_dylink_table_0"))
            .ok()
            .cloned();
        let cacheable = instance
            .exports
            .get_global("__wasix_exec_table_stable")
            .ok()
            .is_some_and(|global| global.get(store).i32() == Some(1));
        Self { table, cacheable }
    }
    pub fn table(&self) -> Option<&wasmer::Table> {
        self.table.as_ref()
    }
    pub fn cacheable(&self) -> bool {
        self.cacheable
    }
}

/// Buffer conventions selected by the importing ABI; the captured target and signature
/// are shared by all modes. Policies never reach the kernel or alter guest wire types.
#[derive(Clone, Copy)]
pub struct CallBufferPolicy {
    pub strict_results_before_call: bool,
    pub zero_result_tail: bool,
    pub whole_result_fields: bool,
    pub validate_empty_reflection_spans: bool,
}

impl Default for CallBufferPolicy {
    fn default() -> Self {
        Self {
            strict_results_before_call: true,
            zero_result_tail: true,
            whole_result_fields: false,
            validate_empty_reflection_spans: false,
        }
    }
}

/// Engine authority borrowed for one import invocation, with its original-error custody.
pub struct GuestFunctionOperations<'a, S: 'static> {
    env: &'a AsyncFunctionEnvMut<ExecEnv<S>>,
    unwind: &'a GuestUnwind,
    buffers: CallBufferPolicy,
    buffer_fault: std::cell::Cell<bool>,
}

impl<'a, S: 'static> GuestFunctionOperations<'a, S> {
    pub fn new(env: &'a AsyncFunctionEnvMut<ExecEnv<S>>, unwind: &'a GuestUnwind) -> Self {
        Self {
            env,
            unwind,
            buffers: CallBufferPolicy::default(),
            buffer_fault: std::cell::Cell::new(false),
        }
    }

    pub fn with_buffer_policy(mut self, policy: CallBufferPolicy) -> Self {
        self.buffers = policy;
        self
    }

    /// Consume only this invocation's engine-buffer failure. An importing ABI can
    /// restore its own fault numbering after ordinary layers have returned EFAULT.
    pub fn take_buffer_fault(&self) -> bool {
        self.buffer_fault.replace(false)
    }

    fn buffer_result<T, E: Into<SysErr>>(&self, result: Result<T, E>) -> SysResult<T> {
        let result = result.map_err(Into::into);
        if matches!(result, Err(SysErr::Errno(Errno::EFAULT))) {
            self.buffer_fault.set(true);
        }
        result
    }

    async fn abi(&self) -> SysResult<std::sync::Arc<dyn AbiInstance<S>>> {
        self.env
            .read()
            .await
            .data()
            .abi()
            .cloned()
            .ok_or(Errno::ENOSYS.into())
    }

    /// Capture the actual function object before releasing the store; a recycled slot
    /// cannot change this invocation's signature or target afterward.
    async fn function(&self, index: u32) -> SysResult<(Function, FunctionType, bool)> {
        let abi = self.abi().await?;
        let table = abi.function_table().ok_or(Errno::EOPNOTSUPP)?;
        let mut guard = self.env.write().await;
        let (_, store) = guard.data_and_store_mut();
        let function = table
            .get_typed_function(store, index)
            .ok_or(Errno::EINVAL)?;
        let ty = function.ty(store);
        Ok((function, ty, abi.function_cacheable(index)))
    }
}

impl<S: 'static> FunctionOperations for GuestFunctionOperations<'_, S> {
    fn call_dynamic<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        index: u32,
        values: RawGuestBuffer,
        results: RawGuestBuffer,
        flags: CallDynamicFlags,
    ) -> LocalFuture<'a, SysResult<i32>> {
        Box::pin(async move {
            let (function, ty, _) = self.function(index).await?;
            let input_size = packed_size(ty.params())?;
            let output_size = packed_size(ty.results())?;
            if flags.0 & !CallDynamicFlags::CALL_DYNAMIC_STRICT.0 != 0 {
                return Err(Errno::EINVAL.into());
            }
            let strict = flags.contains(CallDynamicFlags::CALL_DYNAMIC_STRICT);
            if strict
                && (values.len != input_size as u64
                    || (self.buffers.strict_results_before_call
                        && results.len != output_size as u64))
            {
                return Err(Errno::EINVAL.into());
            }
            // JS cannot pass SIMD through Reflect.apply. The typed Wasm packet path
            // must handle it; a scalar-number conversion would lose the payload.
            #[cfg(feature = "js")]
            if ty.params().contains(&Type::V128) || ty.results().contains(&Type::V128) {
                return Err(Errno::EOPNOTSUPP.into());
            }
            // A partial field is skipped without consuming or touching its bytes.
            // A later narrower field can still consume the same prefix.
            let consumed = consumed_input(
                ty.params(),
                usize::try_from(values.len).unwrap_or(usize::MAX),
            )?;
            let bytes = if consumed == 0 {
                Vec::new()
            } else {
                let addr = self.buffer_result(values.prefix(caller.memory(), consumed).await)?;
                let mut bytes = vec![0; consumed];
                self.buffer_result(caller.memory().read(addr, &mut bytes).await)?;
                bytes
            };
            let args = unpack(ty.params(), &bytes)?;
            let returned = match crate::call_guest(self.env, &function, &args).await {
                Ok(returned) => returned,
                Err(error) => return self.unwind.raise(error),
            };
            let bytes = pack(&returned)?;
            let (output, written) = output_bytes(
                &returned,
                &bytes,
                usize::try_from(results.len).unwrap_or(usize::MAX),
                self.buffers.whole_result_fields,
            )?;
            if !output.is_empty() {
                let addr =
                    self.buffer_result(results.prefix(caller.memory(), output.len()).await)?;
                self.buffer_result(caller.memory().write(addr, &output).await)?;
            }
            if strict && !self.buffers.strict_results_before_call && written as u64 != results.len {
                return Err(Errno::EINVAL.into());
            }
            let mut offset = written as u64;
            let zeros = [0; 4096];
            while self.buffers.zero_result_tail && offset < results.len {
                let len = (results.len - offset).min(zeros.len() as u64);
                let addr = u32::try_from(results.addr.checked_add(offset).ok_or(Errno::EFAULT)?)
                    .map_err(|_| Errno::EFAULT)?;
                caller.memory().write(addr, &zeros[..len as usize]).await?;
                offset += len;
            }
            Ok(0)
        })
    }

    fn reflect_signature<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        index: u32,
        argument_types: RawGuestBuffer,
        result_types: RawGuestBuffer,
        result: RawGuestPointer,
    ) -> LocalFuture<'a, SysResult<i32>> {
        Box::pin(async move {
            let (_, ty, cacheable) = match self.function(index).await {
                Ok(function) => function,
                Err(error) => {
                    let abi = self.abi().await?;
                    let cacheable = if let Some(table) = abi.function_table() {
                        let guard = self.env.read().await;
                        let (_, store) = guard.data_and_store();
                        index < table.size(&store) && abi.function_cacheable(index)
                    } else {
                        false
                    };
                    self.buffer_result(
                        write_value(
                            caller.memory(),
                            GuestPtrMut::new(self.buffer_result(result.narrow())?),
                            &ReflectionResult {
                                cacheable: u8::from(cacheable),
                                arguments: 0,
                                results: 0,
                            },
                        )
                        .await,
                    )?;
                    return Err(error);
                }
            };
            let reflection = ReflectionResult {
                cacheable: u8::from(cacheable),
                arguments: u16::try_from(ty.params().len()).map_err(|_| Errno::EOVERFLOW)?,
                results: u16::try_from(ty.results().len()).map_err(|_| Errno::EOVERFLOW)?,
            };
            self.buffer_result(
                write_value(
                    caller.memory(),
                    GuestPtrMut::new(self.buffer_result(result.narrow())?),
                    &reflection,
                )
                .await,
            )?;
            if ty.params().len() as u64 > argument_types.len
                || ty.results().len() as u64 > result_types.len
            {
                return Err(Errno::EOVERFLOW.into());
            }
            let arguments = types(ty.params())?;
            let results = types(ty.results())?;
            if self.buffers.validate_empty_reflection_spans {
                // Legacy constructs both actual spans before writing either type buffer.
                self.buffer_result(
                    argument_types
                        .check_prefix(caller.memory(), arguments.len())
                        .await,
                )?;
                self.buffer_result(
                    result_types
                        .check_prefix(caller.memory(), results.len())
                        .await,
                )?;
                if !arguments.is_empty() {
                    let address = self.buffer_result(
                        argument_types
                            .prefix(caller.memory(), arguments.len())
                            .await,
                    )?;
                    self.buffer_result(caller.memory().write(address, &arguments).await)?;
                }
                if !results.is_empty() {
                    let address = self
                        .buffer_result(result_types.prefix(caller.memory(), results.len()).await)?;
                    self.buffer_result(caller.memory().write(address, &results).await)?;
                }
            } else {
                if !arguments.is_empty() {
                    let addr = argument_types
                        .prefix(caller.memory(), arguments.len())
                        .await?;
                    caller.memory().write(addr, &arguments).await?;
                }
                if !results.is_empty() {
                    let addr = result_types.prefix(caller.memory(), results.len()).await?;
                    caller.memory().write(addr, &results).await?;
                }
            }
            Ok(0)
        })
    }

    fn closure_allocate<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        closure: U32Ptr,
    ) -> LocalFuture<'a, SysResult<i32>> {
        Box::pin(async move {
            self.abi()
                .await?
                .closure_allocate(self.env, caller, closure)
                .await
        })
    }

    fn closure_prepare<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        backing: u32,
        closure: u32,
        argument_types: UserSlice,
        result_types: UserSlice,
        env_: Uptr,
    ) -> LocalFuture<'a, SysResult<i32>> {
        Box::pin(async move {
            self.abi()
                .await?
                .closure_prepare(
                    self.env,
                    caller,
                    backing,
                    closure,
                    argument_types,
                    result_types,
                    env_,
                )
                .await
        })
    }

    fn closure_free<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        closure: u32,
    ) -> LocalFuture<'a, SysResult<i32>> {
        Box::pin(async move {
            self.abi()
                .await?
                .closure_free(self.env, caller, closure)
                .await
        })
    }
}

fn output_bytes(
    values: &[Value],
    packed: &[u8],
    limit: usize,
    whole_fields: bool,
) -> SysResult<(Vec<u8>, usize)> {
    if !whole_fields {
        let len = packed.len().min(limit);
        return Ok((packed[..len].to_vec(), len));
    }
    let mut output = Vec::new();
    for value in values {
        let bytes = pack(std::slice::from_ref(value))?;
        if output.len() + bytes.len() <= limit {
            output.extend(bytes);
        }
    }
    let len = output.len();
    Ok((output, len))
}

fn types(types: &[Type]) -> SysResult<Vec<u8>> {
    types
        .iter()
        .map(|ty| {
            Ok(match ty {
                Type::I32 => 0,
                Type::I64 => 1,
                Type::F32 => 2,
                Type::F64 => 3,
                Type::V128 => 4,
                _ => return Err(Errno::EINVAL.into()),
            })
        })
        .collect()
}

fn packed_size(types: &[Type]) -> SysResult<usize> {
    types.iter().try_fold(0usize, |size, ty| {
        size.checked_add(match ty {
            Type::I32 | Type::F32 => 4,
            Type::I64 | Type::F64 => 8,
            Type::V128 => 16,
            _ => return Err(Errno::EINVAL.into()),
        })
        .ok_or(Errno::EOVERFLOW.into())
    })
}

fn consumed_input(types: &[Type], limit: usize) -> SysResult<usize> {
    types.iter().try_fold(0usize, |offset, ty| {
        let end = offset
            .checked_add(packed_size(std::slice::from_ref(ty))?)
            .ok_or(Errno::EOVERFLOW)?;
        Ok(if end <= limit { end } else { offset })
    })
}

fn unpack(types: &[Type], bytes: &[u8]) -> SysResult<Vec<Value>> {
    let mut offset = 0;
    macro_rules! value {
        ($ty:ty, $variant:ident, $size:expr) => {{
            if let Some(bytes) = bytes.get(offset..offset + $size) {
                offset += $size;
                Value::$variant(<$ty>::from_le_bytes(bytes.try_into().unwrap()))
            } else {
                Value::$variant(<$ty>::default())
            }
        }};
    }
    let values = types
        .iter()
        .map(|ty| {
            Ok(match ty {
                Type::I32 => value!(i32, I32, 4),
                Type::I64 => value!(i64, I64, 8),
                Type::F32 => value!(f32, F32, 4),
                Type::F64 => value!(f64, F64, 8),
                Type::V128 => value!(u128, V128, 16),
                _ => return Err(Errno::EINVAL.into()),
            })
        })
        .collect();
    let _ = offset;
    values
}

fn pack(values: &[Value]) -> SysResult<Vec<u8>> {
    let mut bytes = Vec::new();
    for value in values {
        match value {
            Value::I32(value) => bytes.extend(value.to_le_bytes()),
            Value::I64(value) => bytes.extend(value.to_le_bytes()),
            Value::F32(value) => bytes.extend(value.to_le_bytes()),
            Value::F64(value) => bytes.extend(value.to_le_bytes()),
            Value::V128(value) => bytes.extend(value.to_le_bytes()),
            _ => return Err(Errno::EINVAL.into()),
        }
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalar_packets_preserve_integer_and_float_bits() {
        let values = [
            Value::I32(-3),
            Value::I64(i64::MIN + 7),
            Value::F32(f32::from_bits(0x7fc00023)),
            Value::F64(f64::from_bits(0xfff8000000000042)),
            Value::V128(u128::MAX - 9),
        ];
        let signature = [Type::I32, Type::I64, Type::F32, Type::F64, Type::V128];
        let bytes = pack(&values).unwrap();
        assert_eq!(bytes.len(), 40);
        assert_eq!(pack(&unpack(&signature, &bytes).unwrap()).unwrap(), bytes);
    }

    #[test]
    fn incomplete_argument_does_not_consume_bytes_for_the_next_value() {
        let values = unpack(&[Type::I64, Type::I32], &7i32.to_le_bytes()).unwrap();
        assert_eq!(values[0].i64(), Some(0));
        assert_eq!(values[1].i32(), Some(7));
        assert_eq!(consumed_input(&[Type::I64], 4).unwrap(), 0);
        assert_eq!(consumed_input(&[Type::I32, Type::I64], 6).unwrap(), 4);
        assert_eq!(consumed_input(&[Type::I64, Type::I32], 6).unwrap(), 4);
        assert!(packed_size(&[Type::FuncRef]).is_err());
        assert!(types(&[Type::ExternRef]).is_err());
    }
}
