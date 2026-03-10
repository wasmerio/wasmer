package main

import "C"
import "unsafe"

func main() {
}

//export wasmer_export_func_params
func wasmer_export_func_params(function unsafe.Pointer, parameters unsafe.Pointer, parametersLength int) unsafe.Pointer {
	return nil
}

//export wasmer_export_func_params_arity
func wasmer_export_func_params_arity(function unsafe.Pointer, result unsafe.Pointer) unsafe.Pointer {
	return nil
}

//export wasmer_export_func_returns_arity
func wasmer_export_func_returns_arity(function unsafe.Pointer, result unsafe.Pointer) unsafe.Pointer {
	return nil
}

//export wasmer_export_kind
func wasmer_export_kind(export unsafe.Pointer) unsafe.Pointer {
	return nil
}

//export wasmer_export_name
func wasmer_export_name(export unsafe.Pointer) unsafe.Pointer {
	return nil
}

//export wasmer_export_to_func
func wasmer_export_to_func(export unsafe.Pointer) unsafe.Pointer {
	return nil
}

//export wasmer_export_to_memory
func wasmer_export_to_memory(export unsafe.Pointer, memory unsafe.Pointer) unsafe.Pointer {
	return nil
}

//export wasmer_exports_destroy
func wasmer_exports_destroy(exports unsafe.Pointer) {
}

//export wasmer_exports_get
func wasmer_exports_get(exports unsafe.Pointer, index int) unsafe.Pointer {
	return nil
}

//export wasmer_exports_len
func wasmer_exports_len(exports unsafe.Pointer) int {
	return 0
}

//export wasmer_import_func_destroy
func wasmer_import_func_destroy(function unsafe.Pointer) {
}

//export wasmer_import_func_new
func wasmer_import_func_new(
	function unsafe.Pointer,
	parametersSignature unsafe.Pointer,
	parametersLength int,
	resultsSignature unsafe.Pointer,
	resultsLength int,
) unsafe.Pointer {
	return nil
}

//export wasmer_instance_call
func wasmer_instance_call(
	instance unsafe.Pointer,
	name unsafe.Pointer,
	parameters unsafe.Pointer,
	parametersLength int,
	results unsafe.Pointer,
	resultsLength int,
) int {
	return 0
}

//export wasmer_instance_context_get
func wasmer_instance_context_get(instance unsafe.Pointer) unsafe.Pointer {
	return nil
}

//export wasmer_instance_context_data_get
func wasmer_instance_context_data_get(instanceContext unsafe.Pointer) unsafe.Pointer {
	return nil
}

//export wasmer_instance_context_data_set
func wasmer_instance_context_data_set(instance unsafe.Pointer, dataPointer unsafe.Pointer) {
}

//export wasmer_instance_context_memory
func wasmer_instance_context_memory(instanceContext unsafe.Pointer) unsafe.Pointer {
	return nil
}

//export wasmer_instance_reset
func wasmer_instance_reset(instance unsafe.Pointer) unsafe.Pointer {
	return nil
}

//export wasmer_instance_destroy
func wasmer_instance_destroy(instance unsafe.Pointer) {
}

//export wasmer_instance_exports
func wasmer_instance_exports(instance unsafe.Pointer, exports unsafe.Pointer) {
}

//export wasmer_instance_cache
func wasmer_instance_cache(instance unsafe.Pointer, cacheBytes unsafe.Pointer, cacheLen int) unsafe.Pointer {
	return nil
}

//export wasmer_instance_from_cache
func wasmer_instance_from_cache(instance unsafe.Pointer, cacheBytes unsafe.Pointer, cacheLen int, options unsafe.Pointer) unsafe.Pointer {
	return nil
}

//export wasmer_import_object_cache_from_imports
func wasmer_import_object_cache_from_imports(imports unsafe.Pointer, importsLength int) unsafe.Pointer {
	return nil
}

//export wasmer_instance_get_points_used
func wasmer_instance_get_points_used(instance unsafe.Pointer) uint64 {
	return 0
}

//export wasmer_instance_set_breakpoint_value
func wasmer_instance_set_breakpoint_value(instance unsafe.Pointer, value uint64) {
}

//export wasmer_instance_get_breakpoint_value
func wasmer_instance_get_breakpoint_value(instance unsafe.Pointer) uint64 {
	return 0
}

//export wasmer_instance_is_function_imported
func wasmer_instance_is_function_imported(instance unsafe.Pointer, name unsafe.Pointer) bool {
	return false
}

//export wasmer_instantiate_with_options
func wasmer_instantiate_with_options(
	instance unsafe.Pointer,
	wasmBytes unsafe.Pointer,
	wasmBytesLength int,
	options unsafe.Pointer,
) unsafe.Pointer {
	return nil
}

//export wasmer_last_error_length
func wasmer_last_error_length() int {
	return 0
}

//export wasmer_last_error_message
func wasmer_last_error_message(buffer unsafe.Pointer, length int) int {
	return 0
}

//export wasmer_memory_data
func wasmer_memory_data(memory unsafe.Pointer) unsafe.Pointer {
	return nil
}

//export wasmer_memory_data_length
func wasmer_memory_data_length(memory unsafe.Pointer) int {
	return 0
}

//export wasmer_memory_grow
func wasmer_memory_grow(memory unsafe.Pointer, numberOfPages int) unsafe.Pointer {
	return nil
}

//export wasmer_instance_get_runtime_breakpoint_value
func wasmer_instance_get_runtime_breakpoint_value(instance unsafe.Pointer) uint64 {
	return 0
}

//export wasmer_instance_enable_rkyv
func wasmer_instance_enable_rkyv() {
}

//export wasmer_instance_disable_rkyv
func wasmer_instance_disable_rkyv() {
}

//export wasmer_set_sigsegv_passthrough
func wasmer_set_sigsegv_passthrough() {
}

//export wasmer_force_install_sighandlers
func wasmer_force_install_sighandlers() {
}

//export wasmer_instance_set_points_used
func wasmer_instance_set_points_used(instance unsafe.Pointer, points uint64) {
}

//export wasmer_instance_set_points_limit
func wasmer_instance_set_points_limit(instance unsafe.Pointer, gasLimit uint64) {
}

//export wasmer_set_opcode_costs
func wasmer_set_opcode_costs(opcodeCostArray unsafe.Pointer) {
}

//export wasmer_instance_set_runtime_breakpoint_value
func wasmer_instance_set_runtime_breakpoint_value(instance unsafe.Pointer, value uint64) {
}

//export wasmer_memory_destroy
func wasmer_memory_destroy(memory unsafe.Pointer) {
}
