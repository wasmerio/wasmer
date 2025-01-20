//! Generates JSON schemas for AppConfig schema

fn main() {
    codegen::generate_schemas();
}

mod codegen {
    use indexmap::IndexMap;

    use std::path::Path;

    pub fn generate_schemas() {
        eprintln!("Generating schemas...");

        let dir = schema_dir();

        // JSON Schema
        let json_dir = dir.join("jsonschema");
        generate_jsonschema(&json_dir);

        eprintln!("Schema-generation complete");
    }

    fn generate_jsonschema(dir: &Path) {
        // Generate .schema.json files.

        let types_dir = dir.join("types");
        eprintln!("Writing .json.schema files to '{}'", types_dir.display());

        std::fs::create_dir_all(&types_dir).unwrap();
        let schemas = build_jsonschema_map();
        for (filename, content) in schemas {
            std::fs::write(types_dir.join(filename), content).unwrap();
        }

        // Markdown docs.

        eprintln!("JSON schema generation complete");
    }

    /// Returns a map of filename to serialized JSON schema.
    fn build_jsonschema_map() -> IndexMap<String, String> {
        let mut map = IndexMap::new();

        fn add_schema<T: schemars::JsonSchema>(map: &mut IndexMap<String, String>, name: &str) {
            let gen =
                schemars::gen::SchemaGenerator::new(schemars::gen::SchemaSettings::draft2019_09());
            map.insert(
                format!("{name}.schema.json"),
                serde_json::to_string_pretty(&gen.into_root_schema_for::<T>()).unwrap(),
            );
        }
        add_schema::<wasmer_config::app::AppConfigV1>(&mut map, "AppConfigV1");
        map
    }

    /// Get the local path to the directory where generated schemas are stored.
    fn schema_dir() -> std::path::PathBuf {
        let crate_dir =
            std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR env var to be set");
        let root_dir = std::path::Path::new(&crate_dir)
            .parent()
            .unwrap()
            .parent()
            .unwrap();

        let schema_dir = root_dir.join("docs/schema/generated");
        if !schema_dir.is_dir() {
            panic!("Expected the {} directory to exist", schema_dir.display());
        }

        schema_dir
    }

    /// Tests that the generated schemas are still up to date.
    #[cfg(not(target_os = "windows"))]
    #[test]
    fn test_generated_schemas_up_to_date() {
        let dir = schema_dir();

        let jsonschema = build_jsonschema_map();
        let json_dir = dir.join("jsonschema").join("types");

        for (filename, jsonschema) in &jsonschema {
            let path = json_dir.join(filename);
            let contents = std::fs::read_to_string(&path).unwrap();

            if contents != *jsonschema {
                panic!(
                    "Auto-generated OpenAPI schema at '{}' is not up to date!\n",
                    path.display()
                );
            }
        }

        for res in std::fs::read_dir(&json_dir).unwrap() {
            let entry = res.unwrap();

            let file_name = entry
                .file_name()
                .to_str()
                .expect("non-utf8 filename")
                .to_string();

            if !jsonschema.contains_key(&file_name) {
                panic!(
                    "Found unexpected file in the json schemas directory: '{}' - delete it!",
                    entry.path().display(),
                );
            }
        }
    }
}
