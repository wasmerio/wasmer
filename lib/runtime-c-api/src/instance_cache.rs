use crate::{
    error::{update_last_error, CApiError},
    instance::{wasmer_instance_t, wasmer_compilation_options_t, CompilationOptions, prepare_middleware_chain_generator, get_compiler},
    wasmer_result_t,
};

use rkyv::{
    Deserialize as RkyvDeserialize,
    ser::Serializer,
    ser::serializers::AllocSerializer,
};

use wasmer_runtime_core::{
    cache::{Artifact, Error as CacheError},
    import::ImportObject,
};
use std::slice;
use crate::import::GLOBAL_IMPORT_OBJECT;

#[cfg(not(feature = "cranelift-backend"))]
use wasmer_middleware_common::metering;

#[cfg(feature = "singlepass-backend")]
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub unsafe extern "C" fn wasmer_instance_enable_rkyv() {
    wasmer_singlepass_backend::USE_RKYV_SERIALIZATION = true;
}

#[cfg(feature = "singlepass-backend")]
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub unsafe extern "C" fn wasmer_instance_disable_rkyv() {
    wasmer_singlepass_backend::USE_RKYV_SERIALIZATION = false;
}

#[cfg(feature = "singlepass-backend")]
pub unsafe fn is_rkyv_enabled() -> bool {
    wasmer_singlepass_backend::USE_RKYV_SERIALIZATION
}

#[cfg(not(feature = "singlepass-backend"))]
pub unsafe fn is_rkyv_enabled() -> bool {
    false
}


#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub unsafe extern "C" fn wasmer_instance_cache(
    instance: *mut wasmer_instance_t,
    cache_bytes: *mut *const u8,
    cache_len: *mut u32,
) -> wasmer_result_t {
    if instance.is_null() {
        update_last_error(CApiError {
            msg: "null instance".to_string(),
        });
        return wasmer_result_t::WASMER_ERROR;
    }

    let instance = &mut *(instance as *mut wasmer_runtime::Instance);
    let module = instance.module();

    match module.cache() {
        Err(error) => {
            update_last_error(CApiError {
                msg: format!("wasmer_instance_cache: artifact creation failed: {:?}", error),
            });
            return wasmer_result_t::WASMER_ERROR;
        }
        Ok(artifact) => {
            match serialize_artifact(artifact) {
                Err(error) => {
                    update_last_error(CApiError {
                        msg: format!("wasmer_instance_cache: artifact serialization failed: {:?}", error),
                    });
                    return wasmer_result_t::WASMER_ERROR;
                }
                Ok(bytes) => {
                    *cache_bytes = bytes.as_ptr();
                    *cache_len = bytes.len() as u32;
                    std::mem::forget(bytes);
                }
            }
        }
    };

    wasmer_result_t::WASMER_OK
}

#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub unsafe extern "C" fn wasmer_instance_from_cache(
    instance: *mut *mut wasmer_instance_t,
    cache_bytes: *mut u8,
    cache_len: u32,
    options: *const wasmer_compilation_options_t,
) -> wasmer_result_t {
    if cache_bytes.is_null() {
        update_last_error(CApiError {
            msg: "cache bytes ptr is null".to_string(),
        });
        return wasmer_result_t::WASMER_ERROR;
    }

    let bytes: &[u8] = slice::from_raw_parts(cache_bytes, cache_len as usize);
    let options: &CompilationOptions = &*(options as *const CompilationOptions);
    let compiler_chain_generator = prepare_middleware_chain_generator(&options);
    let compiler = get_compiler(compiler_chain_generator);

    let artifact = match deserialize_artifact(bytes) {
        Ok(deserialized_artifact) => deserialized_artifact,
        Err(_) => {
            update_last_error(CApiError {
                msg: "wasmer_instance_from_cache: artifact deserialization failed".to_string(),
            });
            return wasmer_result_t::WASMER_ERROR;
        }
    };

    let new_module = match wasmer_runtime_core::load_cache_with(artifact, &compiler) {
        Ok(deserialized_module) => {
            deserialized_module
        }
        Err(_) => {
            update_last_error(CApiError {
                msg: "wasmer_instance_from_cache: artifact instantiation into module failed".to_string(),
            });
            return wasmer_result_t::WASMER_ERROR;
        }
    };

    let import_object: &mut ImportObject = &mut *(GLOBAL_IMPORT_OBJECT as *mut ImportObject);
    let result_instantiation = new_module.instantiate(&import_object);
    let mut new_instance = match result_instantiation {
        Ok(instance) => instance,
        Err(error) => {
            update_last_error(error);
            return wasmer_result_t::WASMER_ERROR;
        }
    };
    metering::set_points_limit(&mut new_instance, options.gas_limit);
    *instance = Box::into_raw(Box::new(new_instance)) as *mut wasmer_instance_t;
    wasmer_result_t::WASMER_OK
}

#[cfg(feature = "singlepass-backend")]
fn serialize_artifact(artifact: Artifact) -> Result<Box<[u8]>, CacheError> {
    let serializer = match unsafe { is_rkyv_enabled() } {
        true => serialize_artifact_with_rkyv,
        false => serialize_artifact_with_serde,
    };

    serializer(artifact).into()
}

#[cfg(not(feature = "singlepass-backend"))]
fn serialize_artifact(artifact: Artifact) -> Result<Box<[u8]>, CacheError> {
    serialize_artifact_with_serde(artifact).into()
}

#[cfg(feature = "singlepass-backend")]
fn serialize_artifact_with_rkyv(artifact: Artifact) -> Result<Box<[u8]>, CacheError> {
    let mut serializer = AllocSerializer::<4096>::default();
    serializer.serialize_value(&artifact).unwrap();
    let serialized = serializer.into_serializer().into_inner().into_boxed_slice();
    if serialized.is_empty() {
        return Err(CacheError::SerializeError("rkyv serialization failed".to_string()));
    }

    Ok(serialized)
}

fn serialize_artifact_with_serde(artifact: Artifact) -> Result<Box<[u8]>, CacheError> {
    match artifact.serialize() {
        Ok(serialized) => Ok(serialized.into_boxed_slice()),
        Err(error) => Err(error),
    }
}

#[cfg(feature = "singlepass-backend")]
fn deserialize_artifact(bytes: &[u8]) -> Result<Artifact, CacheError> {
    let deserializer = match unsafe { is_rkyv_enabled() } {
        true => deserialize_artifact_with_rkyv,
        false => deserialize_artifact_with_serde,
    };

    deserializer(bytes)
}

#[cfg(not(feature = "singlepass-backend"))]
fn deserialize_artifact(bytes: &[u8]) -> Result<Artifact, CacheError> {
    deserialize_artifact_with_serde(bytes)
}

#[cfg(feature = "singlepass-backend")]
fn deserialize_artifact_with_rkyv(bytes: &[u8]) -> Result<Artifact, CacheError> {
    let archived = unsafe { rkyv::archived_root::<Artifact>(&bytes[..]) };
    let artifact: Artifact = RkyvDeserialize::<Artifact, _>::deserialize(archived, &mut rkyv::Infallible).unwrap();
    Ok(artifact)
}

fn deserialize_artifact_with_serde(bytes: &[u8]) -> Result<Artifact, CacheError> {
    Artifact::deserialize(bytes)
}
