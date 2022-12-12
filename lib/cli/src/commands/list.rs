use clap::Parser;

/// Subcommand for listing packages
#[derive(Debug, Copy, Clone, Parser)]
pub struct List {}

impl List {
    /// execute [List]
    pub fn execute(&self) -> Result<(), anyhow::Error> {
        use prettytable::{format, row, Table};

        let rows = wasmer_registry::get_all_local_packages()
            .into_iter()
            .filter_map(|pkg| {
                let package_root_path = pkg.path;
                let (manifest, _) =
                    wasmer_registry::get_executable_file_from_path(&package_root_path, None)
                        .ok()?;
                let commands = manifest
                    .command
                    .unwrap_or_default()
                    .iter()
                    .map(|c| c.get_name())
                    .collect::<Vec<_>>()
                    .join(" \r\n");

                Some(row![pkg.registry, pkg.name, pkg.version, commands])
            })
            .collect::<Vec<_>>();

        let empty_table = rows.is_empty();
        let mut table = Table::init(rows);
        table.set_titles(row!["Registry", "Package", "Version", "Commands"]);
        table.set_format(*format::consts::FORMAT_NO_LINESEP_WITH_TITLE);
        table.set_format(*format::consts::FORMAT_NO_COLSEP);
        if empty_table {
            table.add_empty_row();
        }
        table.printstd();

        Ok(())
    }
}
