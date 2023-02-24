//! This generator is run when regenerate.sh is executed and fixes a couple
//! of issues that wit-bindgen currently doesn't support.
//!
//! Eventually this functionality should be upstreamed into wit-bindgen,
//! see issue [#3177](https://github.com/wasmerio/wasmer/issues/3177).

use std::{
    path::{Path, PathBuf},
    process::ExitCode,
};

use anyhow::{bail, Context};
use convert_case::{Case, Casing};
use quote::quote;
use wai_bindgen_gen_core::{Files, Generator};
use wai_parser::{Interface, TypeDefKind};

fn main() -> Result<(), ExitCode> {
    match run() {
        Ok(_) => {
            eprintln!("All bindings generated successfully");
            Ok(())
        }
        Err(err) => {
            eprintln!("Generation failed!");
            dbg!(err);
            Err(ExitCode::FAILURE)
        }
    }
}

fn run() -> Result<(), anyhow::Error> {
    let root = std::env::var("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .context("Could not read env var CARGO_MANIFEST_DIR")?
        .canonicalize()?
        .parent()
        .context("could not detect parent path")?
        .to_owned();

    generate_wasi(&root)?;
    generate_wasix_wasmer(&root)?;
    generate_wasix_http_client(&root)?;

    // Format code.
    let code = std::process::Command::new("cargo")
        .arg("fmt")
        .current_dir(&root)
        .spawn()?
        .wait()?;
    if !code.success() {
        bail!("Rustfmt failed");
    }

    Ok(())
}

fn generate_wasix_wasmer(root: &Path) -> Result<(), anyhow::Error> {
    eprintln!("Generating wasix Wasmer bindings...");

    let modules = ["wasix_http_client_v1"];
    let schema_dir = root.join("schema/wasix");
    let out_dir = root
        .parent()
        .context("could not find root dir")?
        .join("wasi/src/bindings");

    let opts = wai_bindgen_gen_wasmer::Opts {
        rustfmt: true,
        tracing: true,
        async_: wai_bindgen_gen_wasmer::Async::None,
        custom_error: false,
    };

    for module in modules {
        let wai_path = schema_dir.join(module).with_extension("wai");
        eprintln!("Reading {}...", wai_path.display());
        let wai = std::fs::read_to_string(&wai_path)?;
        let interface = Interface::parse(module, &wai)?;

        let mut gen = opts.clone().build();
        let mut files = Files::default();
        gen.generate_all(&[], &[interface], &mut files);

        assert_eq!(files.iter().count(), 1);
        let (_name, contents_raw) = files.iter().next().unwrap();
        let contents_str = std::str::from_utf8(&contents_raw)?;
        let contents_fixed = wasmer_bindings_fixup(contents_str)?;

        let out_path = out_dir.join(module).with_extension("rs");
        eprintln!("Writing {}...", out_path.display());
        std::fs::write(&out_path, contents_fixed)?;
    }

    eprintln!("Wasix bindings generated");
    Ok(())
}

fn generate_wasix_http_client(root: &Path) -> Result<(), anyhow::Error> {
    eprintln!("Generating wasix http client guest bindings...");

    let modules = ["wasix_http_client_v1"];
    let schema_dir = root.join("schema/wasix");
    let out_dir = root
        .parent()
        .context("Could not get root parent directory")?
        .join("wasix/wasix-http-client/src");

    let opts = wai_bindgen_gen_rust_wasm::Opts {
        rustfmt: true,
        multi_module: false,
        unchecked: false,
        symbol_namespace: String::new(),
        standalone: true,
        force_generate_structs: false,
    };

    for module in modules {
        let wai_path = schema_dir.join(module).with_extension("wai");
        eprintln!("Reading {}...", wai_path.display());
        let wai = std::fs::read_to_string(&wai_path)?;
        let interface = Interface::parse(module, &wai)?;

        let mut gen = opts.clone().build();
        let mut files = Files::default();
        gen.generate_all(&[interface], &[], &mut files);

        assert_eq!(files.iter().count(), 1);
        let (_name, contents_raw) = files.iter().next().unwrap();
        let contents_str = std::str::from_utf8(&contents_raw)?;
        // let contents_fixed = wasmer_bindings_fixup(contents_str)?;
        let contents_fixed = contents_str;

        let out_path = out_dir.join(module).with_extension("rs");
        eprintln!("Writing {}...", out_path.display());
        std::fs::write(&out_path, contents_fixed)?;
    }

    eprintln!("Wasix http client bindings generated");
    Ok(())
}

fn wasmer_bindings_fixup(code: &str) -> Result<String, anyhow::Error> {
    let file = syn::parse_str::<syn::File>(code)?;

    // Strip wrapper module.
    assert_eq!(file.items.len(), 1);
    let first_item = file.items.into_iter().next().unwrap();
    let module = match first_item {
        syn::Item::Mod(m) => m,
        other => {
            bail!("Invalid item: {other:?}");
        }
    };
    let items = module.content.unwrap_or_default().1;

    // Remove bad import
    let final_items = items.into_iter().filter_map(|item| match item {
        syn::Item::Use(use_) => {
            let raw = quote! { #use_ }.to_string();

            // Remove spurious import.
            // Causes problems with ambiguous imports and breaks the dependency
            // tree.
            if raw.trim()
                == "# [allow (unused_imports)] use wai_bindgen_wasmer :: { anyhow , wasmer } ;"
            {
                None
            } else {
                Some(syn::Item::Use(use_))
            }
        }
        syn::Item::Fn(mut func) => {
            if func.sig.ident == "add_to_imports" {
                let store_arg = func
                    .sig
                    .inputs
                    .iter_mut()
                    .find_map(|arg| match arg {
                        syn::FnArg::Receiver(_) => None,
                        syn::FnArg::Typed(pat) => match pat.pat.as_ref() {
                            syn::Pat::Ident(id) if id.ident == "store" => Some(pat),
                            _ => None,
                        },
                    })
                    .expect("Could not find add_to_imports() argument 'store'");

                let new_ty = syn::parse_str("&mut impl wasmer::AsStoreMut").unwrap();

                store_arg.ty = Box::new(new_ty);

                Some(syn::Item::Fn(func))
            } else {
                Some(syn::Item::Fn(func))
            }
        }
        other => Some(other),
    });

    let output = quote::quote! {
        #( #final_items )*
    }
    .to_string();

    Ok(output)
}

fn generate_wasi(root: &Path) -> Result<(), anyhow::Error> {
    eprintln!("Generating wasi bindings...");
    let out_path = root.join("src/wasi/bindings.rs");

    let schema_dir = root.join("schema");
    let schema_dir_wasi = schema_dir.join("wasi");
    let schema_dir_wasix = schema_dir.join("wasix");

    if !schema_dir_wasi.is_dir() || !schema_dir_wasix.is_dir() {
        bail!("Must be run in the same directory as schema/{{wasi/wasix}}");
    }

    // Load wasi.

    let wasi_paths = ["typenames.wit", "wasi_unstable.wit"];
    let wasi_schema_raw = wasi_paths
        .iter()
        .map(|path| {
            eprintln!("Loading {path}...");
            std::fs::read_to_string(schema_dir_wasi.join(path))
        })
        .collect::<Result<Vec<_>, _>>()?
        .join("\n\n");
    let wasi_schema = wai_parser::Interface::parse("output.wai", &wasi_schema_raw)
        .context("Could not parse wasi wai schema")?;

    let opts = wai_bindgen_gen_rust_wasm::Opts {
        rustfmt: false,
        multi_module: false,
        unchecked: false,
        symbol_namespace: String::new(),
        standalone: true,
        force_generate_structs: true,
    };
    let mut gen = opts.build();
    let mut files = wai_bindgen_gen_core::Files::default();
    gen.generate_all(&[wasi_schema.clone()], &[], &mut files);

    assert_eq!(files.iter().count(), 1);
    let (_path, output_raw) = files.iter().next().unwrap();
    let output_basic = std::str::from_utf8(&output_raw).expect("valid utf8 Rust code");

    let output_fixed = bindings_fixup(output_basic, &wasi_schema)?;

    eprintln!("Writing output to {}...", out_path.display());
    std::fs::write(&out_path, output_fixed)?;

    eprintln!("Wasi bindings generated!");
    Ok(())
}

// const WIT_1: &str = include_str!("../../wit-clean/output.wit");
// const BINDINGS_RS: &str = include_str!("../../src/wasi/bindings.rs");

fn replace_in_string(s: &str, id: &str, ty: &str) -> String {
    let parts = s.split(&format!("impl {id} {{")).collect::<Vec<_>>();
    if parts.len() == 1 {
        return s.to_string();
    }
    let replaced = parts[1].replacen(
        "from_bits_preserve(bits: u8)",
        &format!("from_bits_preserve(bits: {ty})"),
        1,
    );
    format!("{}impl {id} {{ {replaced}", parts[0])
}

fn find_attr_by_name_mut<'a>(
    mut attrs: impl Iterator<Item = &'a mut syn::Attribute>,
    name: &str,
) -> Option<&'a mut syn::Attribute> {
    attrs.find(|attr| {
        if let Some(ident) = attr.path.get_ident() {
            ident.to_string() == name
        } else {
            false
        }
    })
}

struct Types(Vec<syn::Path>);

impl syn::parse::Parse for Types {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let result =
            syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated(input)?;
        let items = result.into_iter().collect();
        Ok(Self(items))
    }
}

/// Fix up type definitions for bindings.
fn visit_item(item: &mut syn::Item) {
    match item {
        syn::Item::Enum(enum_) => {
            let name = enum_.ident.to_string();

            {
                // Fix integer representation size for enums.
                let repr_attr = find_attr_by_name_mut(enum_.attrs.iter_mut(), "repr");

                // Change enum repr type.
                match name.as_str() {
                    "Clockid" | "Snapshot0Clockid" | "BusErrno" => {
                        repr_attr.unwrap().tokens = quote!((u32));
                    }
                    "Errno" | "Socktype" | "Addressfamily" | "Sockproto" => {
                        repr_attr.unwrap().tokens = quote!((u16));
                    }
                    _ => {}
                }
            }

            // Add additional derives.

            match name.as_str() {
                "Clockid" | "Signal" | "Snapshot0Clockid" => {
                    let attr = find_attr_by_name_mut(enum_.attrs.iter_mut(), "derive").unwrap();
                    let mut types = attr.parse_args::<Types>().unwrap().0;

                    let prim = syn::parse_str::<syn::Path>("num_enum::TryFromPrimitive").unwrap();
                    types.push(prim);

                    let hash = syn::parse_str::<syn::Path>("Hash").unwrap();
                    types.push(hash);

                    attr.tokens = quote!( ( #( #types ),* ) );
                }
                _ => {}
            }
        }
        // syn::Item::Struct(struct_) => {}
        syn::Item::Mod(module) => {
            if let Some((_delimiter, children)) = &mut module.content {
                children.iter_mut().for_each(visit_item);
            }
        }
        _ => {}
    }
}

// Fix up generated bindings code.
fn bindings_fixup(code: &str, interface: &Interface) -> Result<String, anyhow::Error> {
    // FIXME: move from string patch up to syn AST modifications (as is done below).
    let mut code = code
        .replace("#[allow(clippy::all)]", "")
        .replace("pub struct Rights: u8 {", "pub struct Rights: u64 {")
        .replace("pub struct Lookup: u8 {", "pub struct Lookup: u32 {")
        .replace("pub struct Oflags: u8 {", "pub struct Oflags: u16 {")
        .replace(
            "pub struct Subclockflags: u8 {",
            "pub struct Subclockflags: u16 {",
        )
        .replace(
            "pub struct Eventrwflags: u8 {",
            "pub struct Eventrwflags: u16 {",
        )
        .replace("pub struct Fstflags: u8 {", "pub struct Fstflags: u16 {")
        .replace("pub struct Fdflags: u8 {", "pub struct Fdflags: u16 {");

    code = replace_in_string(&code, "Oflags", "u16");
    code = replace_in_string(&code, "Subclockflags", "u16");
    code = replace_in_string(&code, "Eventrwflags", "u16");
    code = replace_in_string(&code, "Fstflags", "u16");
    code = replace_in_string(&code, "Fdflags", "u16");
    code = replace_in_string(&code, "Lookup", "u32");
    code = replace_in_string(&code, "Rights", "u64");

    // Fix enum types.
    let mut bindings_file = syn::parse_str::<syn::File>(&code)
        .map_err(|e| dbg!(e))
        .context("Could not parse Rust code")?;
    bindings_file.items.iter_mut().for_each(visit_item);
    let bindings_rs = quote!(#bindings_file).to_string();

    let target_path = env!("CARGO_MANIFEST_DIR");
    let path = std::path::Path::new(&target_path)
        .parent()
        .unwrap()
        .join("src")
        .join("wasi")
        .join("extra.rs");
    let mut contents = format!(
        "
        use std::mem::MaybeUninit;
        use wasmer::ValueType;
        // TODO: Remove once bindings generate wai_bindgen_rust::bitflags::bitflags!  (temp hack)
        use wai_bindgen_rust as wit_bindgen_rust;

        {bindings_rs}

    "
    )
    .replace("        ", "");

    println!("output to {}", path.display());

    let excluded_from_impl_valuetype = ["Prestat"];

    for (_, i) in interface.types.iter() {
        let name = i.name.clone().unwrap_or_default().to_case(Case::Pascal);
        if name.is_empty() {
            eprintln!(
                "WARNING: skipping extra trait generation for type without name: {:?}",
                i
            );
            continue;
        }

        match i.kind {
            TypeDefKind::Tuple(_) => {
                eprintln!("Skipping extra trait generation for tupe type {:?}", i);
                continue;
            }
            | TypeDefKind::Record(_)
            | TypeDefKind::Flags(_)
            | TypeDefKind::Variant(_)
            | TypeDefKind::Enum(_)
            | TypeDefKind::Option(_)
            | TypeDefKind::Expected(_)
            | TypeDefKind::Union(_)
            | TypeDefKind::List(_)
            | TypeDefKind::Future(_)
            | TypeDefKind::Stream(_)
            // | TypeDefKind::Type(_)
            => {
                if excluded_from_impl_valuetype.iter().any(|s| *s == name.as_str()) {
                    continue;
                }
                contents.push_str(&format!("
                    // TODO: if necessary, must be implemented in wit-bindgen
                    unsafe impl ValueType for {name} {{
                        #[inline]
                        fn zero_padding_bytes(&self, _bytes: &mut [MaybeUninit<u8>]) {{ }}
                    }}

                ").replace("                    ", ""))
            },
            _ => { }
        }

        if let TypeDefKind::Enum(e) = &i.kind {
            contents.push_str(
                &format!(
                    "
            unsafe impl wasmer::FromToNativeWasmType for {name} {{
                type Native = i32;

                fn to_native(self) -> Self::Native {{
                    self as i32
                }}

                fn from_native(n: Self::Native) -> Self {{
                    match n {{\n"
                )
                .replace("                ", ""),
            );

            for (i, case) in e.cases.iter().enumerate() {
                contents.push_str(&format!(
                    "            {i} => Self::{},\n",
                    case.name.to_case(Case::Pascal)
                ));
            }
            contents.push_str(
                &format!(
                    "
                        q => todo!(\"could not serialize number {{q}} to enum {name}\"),
                    }}
                }}

                fn is_from_store(&self, _store: &impl wasmer::AsStoreRef) -> bool {{ false }}
            }}
            "
                )
                .replace("                ", ""),
            );
        }
    }

    Ok(contents)
}
