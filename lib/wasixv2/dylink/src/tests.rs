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
    assert_eq!(plan.layouts[0].table.start, 1);
    assert_eq!(plan.layouts[2].table, 5..7);
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
