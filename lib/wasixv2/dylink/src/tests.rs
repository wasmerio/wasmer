use crate::{BindingMode, BindingTarget, FixupKind, Module, Plan, PlanError};
use wasmparser::SymbolFlags;

fn leb(mut n: u32, out: &mut Vec<u8>) {
    loop {
        let byte = (n & 127) as u8;
        n >>= 7;
        out.push(byte | if n != 0 { 128 } else { 0 });
        if n == 0 {
            break;
        }
    }
}

fn string(s: &str, out: &mut Vec<u8>) {
    leb(s.len() as u32, out);
    out.extend_from_slice(s.as_bytes());
}

fn section(id: u8, body: &[u8], out: &mut Vec<u8>) {
    out.push(id);
    leb(body.len() as u32, out);
    out.extend_from_slice(body);
}

fn bytes(
    wat: &str,
    size: [u32; 4],
    needed: &[&str],
    exports: &[(&str, SymbolFlags)],
    imports: &[(&str, &str, SymbolFlags)],
) -> Vec<u8> {
    let mut info = Vec::new();
    for value in size {
        leb(value, &mut info);
    }
    let mut data = Vec::new();
    string("dylink.0", &mut data);
    section(1, &info, &mut data);
    let mut info = Vec::new();
    leb(needed.len() as u32, &mut info);
    for name in needed {
        string(name, &mut info);
    }
    section(2, &info, &mut data);
    let mut info = Vec::new();
    leb(exports.len() as u32, &mut info);
    for (name, flags) in exports {
        string(name, &mut info);
        leb(flags.bits(), &mut info);
    }
    section(3, &info, &mut data);
    let mut info = Vec::new();
    leb(imports.len() as u32, &mut info);
    for (module, name, flags) in imports {
        string(module, &mut info);
        string(name, &mut info);
        leb(flags.bits(), &mut info);
    }
    section(4, &info, &mut data);
    let mut wasm = wat::parse_str(wat).unwrap();
    section(0, &data, &mut wasm);
    wasm
}

fn module(wat: &str, needed: &[&str]) -> Module {
    Module::parse(&bytes(wat, [16, 2, 2, 0], needed, &[], &[])).unwrap()
}

#[test]
fn rejects_invalid_wasm_missing_and_duplicate_metadata() {
    assert!(Module::parse(b"not wasm").is_err());
    assert!(Module::parse(&wat::parse_str("(module)").unwrap()).is_err());
    let mut wasm = bytes("(module)", [0; 4], &[], &[], &[]);
    let custom = wasm[8..].to_vec();
    wasm.extend(custom);
    assert!(Module::parse(&wasm).is_err());
    assert!(Module::parse(&bytes("(module)", [0, 32, 0, 0], &[], &[], &[])).is_err());
    assert!(
        Module::parse(&bytes(
            "(module)",
            [0; 4],
            &[],
            &[("absent", SymbolFlags::empty())],
            &[]
        ))
        .is_err()
    );
}

#[test]
fn cycles_diamonds_and_layouts_are_deterministic() {
    let modules = [
        module("(module)", &["b", "c"]),
        module("(module)", &["c"]),
        module("(module)", &["a"]),
    ];
    let deps = [vec![1, 2], vec![2], vec![0]];
    let plan = Plan::new(&modules, &deps, 0, &[], BindingMode::Now).unwrap();
    assert_eq!(plan.load_order, [0, 1, 2]);
    assert_eq!(plan.initialization_order, [2, 1, 0]);
    assert_eq!(plan.layouts[0].memory.start, 0);
    assert_eq!(plan.layouts[2].memory, 32..48);
    assert_eq!(plan.layouts[0].table.start, 3);
    assert_eq!(plan.layouts[2].table, 7..9);
    assert!(
        Plan::new(
            &modules,
            &[vec![1], vec![2], vec![0]],
            0,
            &[],
            BindingMode::Now
        )
        .is_err()
    );
    assert!(
        Plan::new(
            &modules,
            &[vec![1, 9], vec![2], vec![0]],
            0,
            &[],
            BindingMode::Now
        )
        .is_err()
    );
}

#[test]
fn layout_catches_wasm32_overflow_and_respects_alignment() {
    let first = Module::parse(&bytes("(module)", [3, 0, 1, 0], &["b"], &[], &[])).unwrap();
    let second = Module::parse(&bytes("(module)", [4, 4, 1, 3], &[], &[], &[])).unwrap();
    let p = Plan::new(
        &[first, second],
        &[vec![1], vec![]],
        0,
        &[],
        BindingMode::Now,
    )
    .unwrap();
    assert_eq!(p.layouts[1].memory, 16..20);
    assert_eq!(p.layouts[1].table, 8..9);
    let huge = Module::parse(&bytes(
        "(module)",
        [u32::MAX, 0, u32::MAX, 0],
        &["b"],
        &[],
        &[],
    ))
    .unwrap();
    assert!(matches!(
        Plan::new(
            &[huge, module("(module)", &[])],
            &[vec![1], vec![]],
            0,
            &[],
            BindingMode::Now
        ),
        Err(PlanError::LayoutOverflow)
    ));
}

#[test]
fn direct_and_got_bindings_reuse_function_slots() {
    let modules = [
        module(
            r#"(module
      (import "env" "f" (func (param i32) (result i32)))
      (import "GOT.func" "f" (global (mut i32)))
      (import "GOT.func" "f" (global (mut i32)))
      (import "GOT.mem" "data" (global (mut i32))))"#,
            &["lib"],
        ),
        module(
            r#"(module (func (export "f") (param i32) (result i32) local.get 0)
        (global (export "data") i32 (i32.const 4)))"#,
            &[],
        ),
    ];
    let p = Plan::new(&modules, &[vec![1], vec![]], 0, &[], BindingMode::Now).unwrap();
    assert_eq!(p.fixups.len(), 4);
    assert_eq!(p.fixups[0].kind, FixupKind::DirectFunction);
    assert_eq!(p.fixups[1].kind, FixupKind::GotFunction);
    assert_eq!(p.fixups[1].target, p.fixups[2].target);
    assert!(matches!(
        p.fixups[1].target,
        BindingTarget::Function { module: 1, .. }
    ));
    assert!(matches!(
        p.fixups[3].target,
        BindingTarget::Data {
            module: 1,
            offset: Some(4),
            ..
        }
    ));
}

#[test]
fn missing_strong_weak_lazy_and_host_imports_are_distinct() {
    let m = Module::parse(&bytes(
        r#"(module
      (import "env" "missing" (func))
      (import "GOT.mem" "weak" (global (mut i32)))
      (import "wasixv2" "dl_open" (func)))"#,
        [0; 4],
        &[],
        &[],
        &[("GOT.mem", "weak", SymbolFlags::BINDING_WEAK)],
    ))
    .unwrap();
    assert!(matches!(
        Plan::new(
            std::slice::from_ref(&m),
            &[vec![]],
            0,
            &[],
            BindingMode::Now
        ),
        Err(PlanError::MissingSymbol { .. })
    ));
    let p = Plan::new(&[m], &[vec![]], 0, &[], BindingMode::Lazy).unwrap();
    assert_eq!(p.fixups[0].target, BindingTarget::Deferred);
    assert_eq!(p.fixups[1].target, BindingTarget::WeakZero);
    assert_eq!(p.fixups[2].target, BindingTarget::Host);
}

#[test]
fn visibility_scope_and_first_definition_follow_musl() {
    let caller = module("(module (import \"env\" \"f\" (func)))", &["local"]);
    let hidden = Module::parse(&bytes(
        "(module (func (export \"f\")))",
        [0; 4],
        &[],
        &[("f", SymbolFlags::VISIBILITY_HIDDEN)],
        &[],
    ))
    .unwrap();
    let local = module("(module (func (export \"f\")))", &[]);
    let weak = Module::parse(&bytes(
        "(module (func (export \"f\")))",
        [0; 4],
        &[],
        &[("f", SymbolFlags::BINDING_WEAK)],
        &[],
    ))
    .unwrap();
    let modules = [caller, local, hidden, weak];
    let deps = [vec![1], vec![], vec![], vec![]];
    let p = Plan::new(&modules, &deps, 0, &[2, 3], BindingMode::Now).unwrap();
    assert!(matches!(
        p.fixups[0].target,
        BindingTarget::Function { module: 3, .. }
    ));
    let p = Plan::new(&modules, &deps, 0, &[], BindingMode::Now).unwrap();
    assert!(matches!(
        p.fixups[0].target,
        BindingTarget::Function { module: 1, .. }
    ));
}

#[test]
fn type_mismatch_is_not_a_missing_lazy_symbol() {
    let modules = [
        module("(module (import \"env\" \"f\" (func (param i32))))", &["b"]),
        module("(module (func (export \"f\")))", &[]),
    ];
    assert!(matches!(
        Plan::new(&modules, &[vec![1], vec![]], 0, &[], BindingMode::Lazy),
        Err(PlanError::TypeMismatch { .. })
    ));
}

#[test]
fn runtime_paths_unknown_sections_and_untrusted_helpers_are_retained() {
    let mut data = Vec::new();
    string("dylink.0", &mut data);
    section(1, &[0; 4], &mut data);
    let mut paths = Vec::new();
    leb(2, &mut paths);
    string("$ORIGIN", &mut paths);
    string("/lib", &mut paths);
    section(5, &paths, &mut data);
    section(99, &[1, 2, 3], &mut data);
    let mut wasm = wat::parse_str(include_str!("fixtures/initialization.wat")).unwrap();
    section(0, &data, &mut wasm);
    let m = Module::parse(&wasm).unwrap();
    assert_eq!(m.runtime_path(), ["$ORIGIN", "/lib"]);
    assert_eq!(m.unknown_subsections(), [(99, vec![1, 2, 3])]);
    assert!(m.start().is_some());
    assert!(m.exports().contains_key("__wasm_init_tls"));
    let p = Plan::new(&[m], &[vec![]], 0, &[], BindingMode::Now).unwrap();
    assert_eq!(p.fixups[0].target, BindingTarget::Host);
}

#[test]
fn caller_and_asyncify_fixtures_do_not_imply_a_runtime_contract() {
    let m = module(include_str!("fixtures/provenance.wat"), &[]);
    assert_eq!(m.exports()["__dso_handle"].offset, None);
    assert_eq!(m.imports()[0].name, "__memory_base");
    let m = module(include_str!("fixtures/asyncify.wat"), &[]);
    assert!(m.exports().contains_key("asyncify_start_unwind"));
    assert!(matches!(
        Plan::new(&[m], &[vec![]], 0, &[], BindingMode::Now),
        Err(PlanError::MissingSymbol { .. })
    ));
}

#[test]
fn rejects_malformed_subsection_flags_and_nonmutable_got() {
    let invalid = SymbolFlags::BINDING_LOCAL | SymbolFlags::BINDING_WEAK;
    assert!(
        Module::parse(&bytes(
            "(module (func (export \"f\")))",
            [0; 4],
            &[],
            &[("f", invalid)],
            &[]
        ))
        .is_err()
    );
    let m = module("(module (import \"GOT.mem\" \"data\" (global i32)))", &[]);
    assert!(Plan::new(&[m], &[vec![]], 0, &[], BindingMode::Lazy).is_err());
    for body in [&[0, 0, 0][..], &[0, 0, 0, 0, 1][..]] {
        let mut data = Vec::new();
        string("dylink.0", &mut data);
        section(1, body, &mut data);
        let mut wasm = wat::parse_str("(module)").unwrap();
        section(0, &data, &mut wasm);
        assert!(
            Module::parse(&wasm).is_err(),
            "accepted a truncated or trailing memory-info field"
        );
    }
}

#[test]
fn dependency_relocations_see_the_local_load_group() {
    let modules = [
        module("(module (func (export \"main_fn\")))", &["child"]),
        module("(module (import \"env\" \"main_fn\" (func)))", &[]),
    ];
    let p = Plan::new(&modules, &[vec![1], vec![]], 0, &[], BindingMode::Now).unwrap();
    assert!(matches!(
        p.fixups[0].target,
        BindingTarget::Function { module: 0, .. }
    ));
}

#[test]
fn tls_absolute_and_deferred_global_values_stay_distinct() {
    let caller = module(
        r#"(module
      (import "GOT.mem" "tls" (global (mut i32)))
      (import "GOT.mem" "absolute" (global (mut i32)))
      (import "GOT.mem" "dynamic" (global (mut i32))))"#,
        &["b"],
    );
    let provider = Module::parse(&bytes(
        r#"(module
      (import "env" "__memory_base" (global i32))
      (global (export "tls") i32 (i32.const 8))
      (global (export "absolute") i32 (i32.const 64))
      (global (export "dynamic") i32 (global.get 0)))"#,
        [16, 0, 0, 0],
        &[],
        &[
            ("tls", SymbolFlags::TLS),
            ("absolute", SymbolFlags::ABSOLUTE),
        ],
        &[],
    ))
    .unwrap();
    let p = Plan::new(
        &[caller, provider],
        &[vec![1], vec![]],
        0,
        &[],
        BindingMode::Now,
    )
    .unwrap();
    assert!(matches!(
        p.fixups[0].target,
        BindingTarget::Data {
            tls: true,
            absolute: false,
            offset: Some(8),
            ..
        }
    ));
    assert!(matches!(
        p.fixups[1].target,
        BindingTarget::Data {
            tls: false,
            absolute: true,
            offset: Some(64),
            ..
        }
    ));
    assert!(matches!(
        p.fixups[2].target,
        BindingTarget::Data { offset: None, .. }
    ));
    assert!(
        p.deferred_checks
            .contains(&crate::DeferredCheck::GlobalValue {
                module: 1,
                index: 3
            })
    );
}

#[test]
fn hidden_definition_binds_locally_even_in_a_preempting_global_scope() {
    let caller = Module::parse(&bytes(
        r#"(module
      (import "GOT.func" "f" (global (mut i32)))
      (func (export "f")))"#,
        [0; 4],
        &[],
        &[("f", SymbolFlags::VISIBILITY_HIDDEN)],
        &[],
    ))
    .unwrap();
    let preemptor = module("(module (func (export \"f\")))", &[]);
    let p = Plan::new(
        &[caller, preemptor],
        &[vec![], vec![]],
        0,
        &[1],
        BindingMode::Now,
    )
    .unwrap();
    assert!(matches!(
        p.fixups[0].target,
        BindingTarget::Function { module: 0, .. }
    ));
    assert!(p.deferred_checks.iter().any(|check| matches!(
        check,
        crate::DeferredCheck::FunctionIdentity { module: 0, .. }
    )));
}

#[test]
fn duplicate_subsections_unknown_flags_and_missing_import_metadata_are_rejected() {
    let mut data = Vec::new();
    string("dylink.0", &mut data);
    section(1, &[0; 4], &mut data);
    section(1, &[0; 4], &mut data);
    let mut wasm = wat::parse_str("(module)").unwrap();
    section(0, &data, &mut wasm);
    assert!(Module::parse(&wasm).is_err());
    let unknown = SymbolFlags::from_bits_retain(1 << 31);
    assert!(
        Module::parse(&bytes(
            "(module (func (export \"f\")))",
            [0; 4],
            &[],
            &[("f", unknown)],
            &[]
        ))
        .is_err()
    );
    assert!(
        Module::parse(&bytes(
            "(module)",
            [0; 4],
            &[],
            &[],
            &[("env", "missing", SymbolFlags::BINDING_WEAK)]
        ))
        .is_err()
    );
}

#[test]
fn graph_indices_and_wasm_indices_have_separate_validation() {
    let m = module("(module)", &[]);
    assert!(
        Plan::new(
            std::slice::from_ref(&m),
            &[vec![]],
            1,
            &[],
            BindingMode::Now
        )
        .is_err()
    );
    assert!(Plan::new(std::slice::from_ref(&m), &[], 0, &[], BindingMode::Now).is_err());
    assert!(Plan::new(&[m], &[vec![]], 0, &[1], BindingMode::Now).is_err());
    // Hand-authored export of an absent function. Validation must fail before metadata indexing.
    let mut wasm = wat::parse_str("(module)").unwrap();
    section(7, &[1, 1, b'f', 0, 99], &mut wasm);
    assert!(Module::parse(&wasm).is_err());
}

#[test]
fn initialization_is_bounded_host_effects_not_guest_execution() {
    let wasm = bytes(
        r#"(module
      (import "env" "__memory_base" (global $base i32))
      (memory 1)
      (data $image "hello")
      (global $tls (mut i32) (i32.const 0))
      (func $init
        global.get $base i32.const 0 i32.const 5 memory.init $image
        data.drop $image)
      (start $init)
      (func (export "__wasm_apply_data_relocs")
        global.get $base i32.const 8 i32.add
        global.get $base i32.const 4 i32.add i32.store)
      (func (export "__wasm_init_tls") (param $block i32)
        local.get $block global.set $tls
        local.get $block i32.const 0 i32.const 5 memory.init $image))"#,
        [16, 0, 0, 0],
        &[],
        &[],
        &[],
    );
    let init = crate::Initialization::parse(&wasm).unwrap();
    let effects = init
        .effects([(0, 128)].into(), &reservation(128..144), None)
        .unwrap();
    assert_eq!(effects.writes[0].bytes, b"hello");
    assert_eq!(effects.writes[1].address, 136);
    assert_eq!(effects.writes[1].bytes, 132u32.to_le_bytes());
    assert!(
        init.effects([(0, 128)].into(), &reservation(129..144), None)
            .is_err()
    );
    assert_eq!(
        init.dropped_segments().iter().copied().collect::<Vec<_>>(),
        [0]
    );
}

#[test]
fn initialization_rejects_import_calls_recursion_loops_and_imported_global_writes() {
    for wat in [
        include_str!("fixtures/initialization.wat"),
        "(module (func $init (call $init)) (start $init))",
        "(module (func $init (loop (br 0))) (start $init))",
        "(module (import \"env\" \"sp\" (global $sp (mut i32))) (func $init i32.const 0 global.set $sp) (start $init))",
    ] {
        assert!(crate::Initialization::parse(&bytes(wat, [0; 4], &[], &[], &[])).is_err());
    }
}

#[test]
fn shared_once_template_is_lifted_without_atomic_wait_or_peer_reinitialization() {
    let wasm = bytes(
        include_str!("fixtures/once-initialization.wat"),
        [16, 0, 0, 0],
        &[],
        &[],
        &[],
    );
    let init = crate::Initialization::parse(&wasm).unwrap();
    let first = init
        .effects([(0, 128)].into(), &reservation(128..144), None)
        .unwrap();
    assert_eq!(first.writes[0].bytes, b"test");
    assert_eq!(first.writes[1].address, 136);
    assert_eq!(first.writes[1].bytes, 2u32.to_le_bytes());
    let peer = init
        .effects([(0, 128)].into(), &reservation(256..272), Some(256))
        .unwrap();
    assert!(peer.writes.is_empty());
    let bad = include_str!("fixtures/once-initialization.wat")
        .replace("br_table 0 1 2", "br_table 0 2 2");
    assert!(crate::Initialization::parse(&bytes(&bad, [16, 0, 0, 0], &[], &[], &[])).is_err());
}

#[test]
fn sanitized_instantiation_has_no_start_or_active_shared_writes() {
    let wasm = bytes(
        r#"(module
      (import "env" "__memory_base" (global $base i32))
      (import "env" "__table_base" (global $table i32))
      (memory 1)
      (table 16 funcref)
      (func $function)
      (data (global.get $base) "new")
      (data $passive "keep")
      (elem (global.get $table) func $function)
      (func $init)
      (start $init))"#,
        [16, 0, 2, 0],
        &[],
        &[],
        &[],
    );
    let init = crate::Initialization::parse(&wasm).unwrap();
    let globals: std::collections::BTreeMap<u32, u64> = [(0, 128), (1, 8)].into();
    assert_eq!(
        init.effects(globals.clone(), &reservation(128..144), None)
            .unwrap()
            .writes[0]
            .bytes,
        b"new"
    );
    assert_eq!(
        init.table_effects(&globals, &reservation(8..10)).unwrap(),
        [crate::InitTable {
            index: 8,
            functions: vec![Some(0)]
        }]
    );
    assert!(init.table_effects(&globals, &reservation(9..11)).is_err());
    let clean = init.instantiate_bytes().unwrap();
    let module = Module::parse(&clean).unwrap();
    assert_eq!(module.start(), None);
    assert!(module.exports().contains_key("__wasix_dylink_fn_0"));
    let mut data = Vec::new();
    for payload in wasmparser::Parser::new(0).parse_all(&clean) {
        match payload.unwrap() {
            wasmparser::Payload::DataSection(section) => {
                for segment in section {
                    let segment = segment.unwrap();
                    assert!(matches!(segment.kind, wasmparser::DataKind::Passive));
                    data.push(segment.data.to_vec());
                }
            }
            wasmparser::Payload::ElementSection(section) => {
                for segment in section {
                    assert!(matches!(
                        segment.unwrap().kind,
                        wasmparser::ElementKind::Declared
                    ));
                }
            }
            _ => {}
        }
    }
    assert_eq!(data, [vec![], b"keep".to_vec()]);
}

#[test]
#[ignore = "requires a source-built lld fixture via WASIXV2_DYLINK_PROOF"]
fn source_built_lld_initialization_contract() {
    let path =
        std::env::var_os("WASIXV2_DYLINK_PROOF").expect("path to source-built lld wasm fixture");
    let bytes = std::fs::read(path).unwrap();
    let metadata = Module::parse(&bytes).unwrap();
    if let Some(symbol) = metadata.exports().get("tls_sym") {
        eprintln!("source-built tls_sym metadata: {symbol:?}");
    }
    let init = crate::Initialization::parse(&bytes).unwrap();
    let mut globals = std::collections::BTreeMap::new();
    let mut index = 0;
    for payload in wasmparser::Parser::new(0).parse_all(&bytes) {
        match payload.unwrap() {
            wasmparser::Payload::ImportSection(section) => {
                for import in section.into_imports() {
                    let import = import.unwrap();
                    if matches!(import.ty, wasmparser::TypeRef::Global(_)) {
                        globals.insert(
                            index,
                            match import.name {
                                "__memory_base" | "tls_sym" => 4096,
                                "__table_base" => 3,
                                _ => 0,
                            },
                        );
                        index += 1;
                    }
                }
            }
            wasmparser::Payload::GlobalSection(section) => {
                for global in section {
                    let global = global.unwrap();
                    let value = match global.init_expr.get_operators_reader().read().unwrap() {
                        wasmparser::Operator::I32Const { value } => u64::from(value as u32),
                        wasmparser::Operator::I64Const { value } => value as u64,
                        wasmparser::Operator::GlobalGet { global_index } => globals[&global_index],
                        _ => panic!("unsupported proof global initializer"),
                    };
                    globals.insert(index, value);
                    index += 1;
                }
            }
            _ => {}
        }
    }
    let first = init
        .effects(globals.clone(), &reservation(0..65536), None)
        .unwrap();
    assert!(!first.writes.is_empty());
    let metadata = Module::parse(&bytes).unwrap();
    let tls_global = metadata.exports()["__tls_base"].index;
    let tls_base = first.globals[&tls_global] as u32;
    assert!(
        first
            .writes
            .iter()
            .any(|write| write.address == tls_base + 8 && write.bytes == tls_base.to_le_bytes()),
        "first TLS block's pointer relocation was not applied"
    );
    // The DSO imports its preemptible TLS symbol; the real resolver supplies the
    // address for this peer, rather than reusing the first thread's GOT input.
    for (index, import) in metadata
        .imports()
        .iter()
        .filter(|import| matches!(import.ty, crate::ExternType::Global(_)))
        .enumerate()
    {
        if import.name == "tls_sym" {
            globals.insert(index as u32, 32768);
        }
    }
    let peer = init
        .effects(globals, &reservation(32768..32800), Some(32768))
        .unwrap();
    assert!(
        peer.writes
            .iter()
            .all(|write| write.address >= 32768 && write.address < 32800)
    );
    assert!(
        peer.writes
            .iter()
            .any(|write| write.address == 32776 && write.bytes == 32768u32.to_le_bytes())
    );
    let rewritten = init.instantiate_bytes().unwrap();
    assert_eq!(Module::parse(&rewritten).unwrap().start(), None);
}

fn reservation(range: std::ops::Range<u64>) -> [std::ops::Range<u64>; 1] {
    [range]
}

#[test]
fn signal_handler_sentinels_are_not_dynamic_table_slots() {
    let m = Module::parse(&bytes(
        "(module (import \"GOT.func\" \"f\" (global (mut i32))) (func (export \"f\")))",
        [0; 4],
        &[],
        &[],
        &[],
    ))
    .unwrap();
    let plan = Plan::new(&[m], &[vec![]], 0, &[], BindingMode::Now).unwrap();
    assert!(plan.layouts[0].table.start >= 3);
    assert!(matches!(
        plan.fixups[0].target,
        BindingTarget::Function {
            slot: Some(3..),
            ..
        }
    ));
}

#[test]
fn dynamic_main_source_preparation_is_explicit_and_non_executing() {
    let static_image = wat::parse_str("(module (func $init) (start $init))").unwrap();
    assert_eq!(crate::prepare_image(&static_image).unwrap(), static_image);
    let native = r#"(module
      (import "wasixv2" "dl_close" (func (param i32) (result i32)))
      (memory 1)
      (func $init)
      (start $init))"#;
    let original = bytes(native, [0; 4], &[], &[], &[]);
    let prepared = crate::prepare_image(&original).unwrap();
    assert_eq!(Module::parse(&prepared).unwrap().start(), None);
    let mut sources = Vec::new();
    for payload in wasmparser::Parser::new(0).parse_all(&prepared) {
        if let wasmparser::Payload::CustomSection(section) = payload.unwrap()
            && section.name() == crate::SOURCE_SECTION
        {
            sources.push(section.data());
        }
    }
    assert_eq!(sources, [original.as_slice()]);
    assert!(crate::prepare_image(&prepared).is_err());
    assert!(crate::prepare_image(&bytes(native, [0; 4], &["lib.so"], &[], &[])).is_err());
    let asyncify = native.replace(
        "(memory 1)",
        "(memory 1) (func (export \"asyncify_start_unwind\"))",
    );
    assert!(crate::prepare_image(&bytes(&asyncify, [0; 4], &[], &[], &[])).is_err());
}

#[test]
fn peer_globals_and_private_engine_handles_preserve_store_identity() {
    let wasm = bytes(
        r#"(module
      (memory 1)
      (tag (param i32))
      (global $state (mut i32) (i32.const 0))
      (func $start i32.const 9 global.set $state)
      (start $start))"#,
        [0; 4],
        &[],
        &[],
        &[],
    );
    let init = crate::Initialization::parse(&wasm).unwrap();
    let effects = init.peer_globals([(0, 0)].into()).unwrap();
    assert_eq!(effects.globals[&0], 9);
    assert!(effects.writes.is_empty());
    let rewritten = Module::parse(&init.instantiate_bytes().unwrap()).unwrap();
    assert_eq!(rewritten.exports()["__wasix_dylink_memory_0"].index, 0);
    assert_eq!(rewritten.exports()["__wasix_dylink_tag_0"].index, 0);
    let writing = bytes(
        "(module (memory 1) (func $s i32.const 8 i32.const 9 i32.store) (start $s))",
        [0; 4],
        &[],
        &[],
        &[],
    );
    assert!(
        crate::Initialization::parse(&writing)
            .unwrap()
            .peer_globals(Default::default())
            .is_err()
    );
}

#[test]
fn peer_tls_pointer_relocations_follow_global_tls_fixups() {
    let wasm = bytes(
        r#"(module
      (memory 1)
      (global $base (mut i32) (i32.const 0))
      (global $got (mut i32) (i32.const 0))
      (func $reloc (export "__wasm_apply_tls_relocs")
        global.get $base global.get $got i32.store)
      (func (export "__wasm_init_tls") (param i32)
        local.get 0 global.set $base
        call $reloc
        global.get $base global.set $got))"#,
        [0; 4],
        &[],
        &[],
        &[],
    );
    let init = crate::Initialization::parse(&wasm).unwrap();
    let effects = init
        .effects([(0, 0), (1, 0)].into(), &reservation(128..132), Some(128))
        .unwrap();
    assert_eq!(effects.writes.last().unwrap().bytes, 128u32.to_le_bytes());
}

#[cfg(feature = "engine-tests")]
#[test]
fn native_staging_never_runs_guest_initializers_and_rolls_back_fresh_effects() {
    use wasmer::{Global, Imports, Memory, MemoryType, Store, Table, TableType, Type, Value};
    let mut store = Store::default();
    let memory = Memory::new(&mut store, MemoryType::new(1, Some(4), true)).unwrap();
    let table = Table::new(
        &mut store,
        TableType::new(Type::FuncRef, 3, Some(64)),
        Value::FuncRef(None),
    )
    .unwrap();
    let base = Global::new(&mut store, Value::I32(128));
    let table_base = Global::new(&mut store, Value::I32(3));
    let mut imports = Imports::new();
    // The production binder supplies exec's normal environment-backed hooks. These
    // typed test hooks are never called while staging and let published code run.
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let ticks = Arc::new(AtomicUsize::new(0));
    fn tick(env: wasmer::FunctionEnvMut<Arc<AtomicUsize>>) -> i32 {
        env.data().fetch_add(1, Ordering::Relaxed);
        65536
    }
    let tick_env = wasmer::FunctionEnv::new(&mut store, ticks.clone());
    let hook = wasmer::Function::new_typed_with_env(&mut store, &tick_env, tick);
    imports.define("wasixv2-exec", "preempt", hook);
    imports.define(
        "wasixv2-exec",
        "unreachable",
        wasmer::Function::new_typed(&mut store, || -> i32 {
            panic!("unreachable guest instruction")
        }),
    );
    imports.define(
        "wasixv2-exec",
        "memory-grow",
        wasmer::Function::new_typed(&mut store, |_: i32| -> i32 { -1 }),
    );
    imports.define(
        "wasixv2-exec",
        "memory-atomic-wait32",
        wasmer::Function::new_typed(&mut store, |_: i32, _: i32, _: i64, _: i32| -> i32 {
            panic!("unexpected wait32")
        }),
    );
    imports.define(
        "wasixv2-exec",
        "memory-atomic-wait64",
        wasmer::Function::new_typed(&mut store, |_: i32, _: i64, _: i64, _: i32| -> i32 {
            panic!("unexpected wait64")
        }),
    );
    imports.define(
        "wasixv2-exec",
        "memory-atomic-notify",
        wasmer::Function::new_typed(&mut store, |_: i32, _: i32, _: i32| -> i32 {
            panic!("unexpected notify")
        }),
    );
    imports.define("env", "memory", memory.clone());
    imports.define("env", "__indirect_function_table", table.clone());
    imports.define("env", "__memory_base", base);
    imports.define("env", "__table_base", table_base);
    let source = bytes(
        r#"(module
      (import "env" "memory" (memory 1 4 shared))
      (import "env" "__indirect_function_table" (table 3 funcref))
      (import "env" "__memory_base" (global $base i32))
      (import "env" "__table_base" (global $table i32))
      (global $state (mut i32) (i32.const 0))
      (func $f (export "f") (result i32) i32.const 42)
      (func $g (result i32) i32.const 43)
      (func (export "busy") (local i32)
        i32.const 70000 local.set 0
        (loop $again local.get 0 i32.const 1 i32.sub local.tee 0 br_if $again))
      (elem (global.get $table) func $f)
      (data (global.get $base) "new")
      (func $init i32.const 7 global.set $state)
      (start $init))"#,
        [16, 0, 1, 0],
        &[],
        &[],
        &[],
    );
    let mut prepared = crate::StagedModule::prepare(
        &mut store,
        &source,
        &imports,
        memory.clone(),
        table.clone(),
        &reservation(128..144),
        &reservation(3..4),
        None,
    )
    .unwrap();
    let mut bytes = [0; 3];
    memory.view(&store).read(128, &mut bytes).unwrap();
    assert_eq!(bytes, [0; 3]);
    assert!(matches!(
        table.get(&mut store, 3),
        Some(Value::FuncRef(None))
    ));
    assert_eq!(
        prepared
            .instance()
            .exports
            .get_global("__wasix_dylink_global_2")
            .unwrap()
            .get(&mut store)
            .i32(),
        Some(0)
    );
    assert!(
        prepared
            .bind_slot(&mut store, 3, 1, &reservation(3..4))
            .is_err()
    );
    prepared
        .bind_slot(&mut store, 3, 0, &reservation(3..4))
        .unwrap();
    assert!(
        prepared
            .bind_slot(&mut store, 2, 0, &reservation(3..4))
            .is_err()
    );
    prepared.apply(&mut store).unwrap();
    assert!(prepared.apply(&mut store).is_err());
    memory.view(&store).read(128, &mut bytes).unwrap();
    assert_eq!(&bytes, b"new");
    assert_eq!(
        prepared
            .instance()
            .exports
            .get_global("__wasix_dylink_global_2")
            .unwrap()
            .get(&mut store)
            .i32(),
        Some(7)
    );
    assert!(matches!(
        table.get(&mut store, 3),
        Some(Value::FuncRef(Some(_)))
    ));
    prepared.rollback(&mut store).unwrap();
    memory.view(&store).read(128, &mut bytes).unwrap();
    assert_eq!(bytes, [0; 3]);
    assert!(matches!(
        table.get(&mut store, 3),
        Some(Value::FuncRef(None))
    ));
    let function = prepared.function(0).unwrap();
    assert!(prepared.publish().is_err());
    // Existing table entries are never overwritten, even by the same function.
    table
        .set(&mut store, 3, Value::FuncRef(Some(function)))
        .unwrap();
    assert!(
        crate::StagedModule::prepare(
            &mut store,
            &source,
            &imports,
            memory.clone(),
            table.clone(),
            &reservation(128..144),
            &reservation(3..4),
            None
        )
        .is_err()
    );
    table.set(&mut store, 3, Value::FuncRef(None)).unwrap();
    let mut prepared = crate::StagedModule::prepare(
        &mut store,
        &source,
        &imports,
        memory,
        table,
        &reservation(128..144),
        &reservation(3..4),
        None,
    )
    .unwrap();
    prepared.apply(&mut store).unwrap();
    let instance = prepared.publish().unwrap();
    let function = instance
        .exports
        .get_typed_function::<(), i32>(&store, "f")
        .unwrap();
    assert_eq!(function.call(&mut store).unwrap(), 42);
    instance
        .exports
        .get_typed_function::<(), ()>(&store, "busy")
        .unwrap()
        .call(&mut store)
        .unwrap();
    assert!(
        ticks.load(Ordering::Relaxed) > 0,
        "side-module loop bypassed exec preemption"
    );
}

#[test]
fn lld_omitted_undefined_weak_annotations_do_not_create_imports() {
    let flags = SymbolFlags::UNDEFINED | SymbolFlags::BINDING_WEAK | SymbolFlags::VISIBILITY_HIDDEN;
    let wasm = bytes(
        "(module)",
        [0; 4],
        &[],
        &[],
        &[("env", "__init_array_start", flags)],
    );
    let module = Module::parse(&wasm).unwrap();
    assert!(module.imports().is_empty());
    assert_eq!(
        module.omitted_weak_imports()[&("env".into(), "__init_array_start".into())],
        flags
    );
    assert!(
        Module::parse(&bytes(
            "(module)",
            [0; 4],
            &[],
            &[],
            &[("env", "strong", SymbolFlags::UNDEFINED)]
        ))
        .is_err()
    );
}

#[test]
fn semantic_weak_import_flags_apply_to_got_and_ambiguous_flags_fail() {
    let flags = SymbolFlags::UNDEFINED | SymbolFlags::BINDING_WEAK;
    let wat = "(module (import \"GOT.mem\" \"weak\" (global (mut i32))))";
    let wasm = bytes(wat, [0; 4], &[], &[], &[("env", "weak", flags)]);
    let module = Module::parse(&wasm).unwrap();
    assert_eq!(module.imports()[0].flags, flags);
    assert!(module.omitted_weak_imports().is_empty());
    let plan = Plan::new(&[module], &[vec![]], 0, &[], BindingMode::Now).unwrap();
    assert_eq!(plan.fixups[0].target, BindingTarget::WeakZero);
    let wasm = bytes(
        wat,
        [0; 4],
        &[],
        &[],
        &[
            ("env", "weak", flags),
            ("GOT.mem", "weak", SymbolFlags::empty()),
        ],
    );
    assert!(Module::parse(&wasm).is_err());
}
