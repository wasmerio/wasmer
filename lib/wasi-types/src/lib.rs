#![doc(html_favicon_url = "https://wasmer.io/images/icons/favicon-32x32.png")]
#![doc(html_logo_url = "https://github.com/wasmerio.png?size=200")]

pub mod asyncify;
pub mod types;
pub mod wasi;
pub mod wasix;

// TODO: re-enable once WAI bindings generator generates correct code.
// Currently we need manual fixes.
// #[cfg(test)]
// mod tests {
//     use std::{
//         collections::HashMap,
//         path::{Path, PathBuf},
//     };

//     // Prevent the CI from passing if the wasi/bindings.rs is not
//     // up to date with the output.wit file
//     #[test]
//     #[cfg(feature = "sys")]
//     fn fail_if_wit_files_arent_up_to_date() {
//         let root_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
//         let src_dir = root_dir.join("src");
//         let gen_dir = root_dir.join("wasi-types-generator-extra");

//         let current_files = load_file_tree(&src_dir).unwrap();

//         let code = std::process::Command::new("cargo")
//             .arg("run")
//             .current_dir(gen_dir)
//             .spawn()
//             .unwrap()
//             .wait()
//             .unwrap();
//         assert!(code.success());

//         let new_files = load_file_tree(&src_dir).unwrap();

//         assert_eq!(
//             current_files, new_files,
//             "generated bindings files have changed - current bindings not up to date!"
//         );
//     }

//     fn load_file_tree(path: &Path) -> Result<HashMap<PathBuf, String>, std::io::Error> {
//         let mut map = HashMap::new();
//         load_file_tree_recursive(path, &mut map)?;
//         Ok(map)
//     }

//     fn load_file_tree_recursive(
//         path: &Path,
//         map: &mut HashMap<PathBuf, String>,
//     ) -> Result<(), std::io::Error> {
//         for res in std::fs::read_dir(path)? {
//             let entry = res?;
//             let entry_path = entry.path();
//             let ty = entry.file_type()?;
//             if ty.is_dir() {
//                 load_file_tree_recursive(&entry_path, map)?;
//             } else if ty.is_file() {
//                 let content = std::fs::read_to_string(&entry_path)?;
//                 map.insert(entry_path, content);
//             }
//         }
//         Ok(())
//     }
// }
