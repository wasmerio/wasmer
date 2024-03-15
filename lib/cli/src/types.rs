use comfy_table::Table;
use wasmer_api::types::{DeployApp, DeployAppVersion, DnsDomain, DnsDomainWithRecords, Namespace};

use crate::utils::render::CliRender;

impl CliRender for DnsDomain {
    fn render_item_table(&self) -> String {
        let mut table = Table::new();
        table.add_rows([vec!["Domain".to_string(), self.name.clone()]]);
        table.to_string()
    }

    fn render_list_table(items: &[Self]) -> String {
        if items.is_empty() {
            return String::new();
        }
        let mut table = Table::new();
        table.set_header(vec!["Domain".to_string(), "Owner".to_string()]);
        table.add_rows(
            items
                .iter()
                .map(|ns| vec![ns.name.clone(), ns.owner.global_name.clone()]),
        );
        table.to_string()
    }
}

impl CliRender for DnsDomainWithRecords {
    fn render_item_table(&self) -> String {
        let mut output = String::new();
        let mut table = Table::new();
        table
            .load_preset(comfy_table::presets::UTF8_FULL_CONDENSED)
            .set_header(vec![
                "Type".to_string(),
                "Name".to_string(),
                "TTL".to_string(),
                "Value".to_string(),
            ]);
        let mut rows: Vec<Vec<String>> = vec![];
        if let Some(ref records) = self.records {
            records.iter().flatten().for_each(|record| {
                rows.push(vec![
                    record.record_type().to_string(),
                    record.name().unwrap_or("<no name>").to_string(),
                    record
                        .ttl()
                        .expect("expected a TTL value for record")
                        .to_string(),
                    record.text().to_string(),
                ]);
            });
        }

        table.add_rows(rows);
        output += &table.to_string();
        output
    }

    fn render_list_table(items: &[Self]) -> String {
        if items.is_empty() {
            return String::new();
        }
        let mut table = Table::new();
        table.set_header(vec!["Domain".to_string()]);
        table.add_rows(items.iter().map(|ns| vec![ns.name.clone()]));
        table.to_string()
    }
}

impl CliRender for Namespace {
    fn render_item_table(&self) -> String {
        let mut table = Table::new();
        table.add_rows([
            vec!["Namespace".to_string(), self.name.clone()],
            vec!["Id".to_string(), self.id.inner().to_string()],
        ]);
        table.to_string()
    }

    fn render_list_table(items: &[Self]) -> String {
        let mut table = Table::new();
        table.set_header(vec!["Namespace".to_string(), "Id".to_string()]);
        table.add_rows(
            items
                .iter()
                .map(|ns| vec![ns.name.clone(), ns.id.inner().to_string()]),
        );
        table.to_string()
    }
}

impl CliRender for DeployApp {
    fn render_item_table(&self) -> String {
        let mut table = Table::new();
        table.add_rows([
            vec![
                "App".to_string(),
                format!("{}/{}", self.owner.global_name, self.name),
            ],
            vec!["Version".to_string(), self.active_version.version.clone()],
            vec![
                "Created".to_string(),
                self.active_version.created_at.0.clone(),
            ],
            vec!["Id".to_string(), self.id.inner().to_string()],
        ]);
        table.to_string()
    }

    fn render_list_table(items: &[Self]) -> String {
        let mut table = Table::new();
        table.set_header(vec![
            "App".to_string(),
            "Version".to_string(),
            "Created".to_string(),
            "Id".to_string(),
        ]);
        table.add_rows(items.iter().map(|app| {
            vec![
                format!("{}/{}", app.owner.global_name, app.name),
                app.active_version.version.clone(),
                app.active_version.created_at.0.clone(),
                app.id.inner().to_string(),
            ]
        }));
        table.to_string()
    }
}

impl CliRender for DeployAppVersion {
    fn render_item_table(&self) -> String {
        let mut table = Table::new();
        table.add_rows([
            vec!["Version name".to_string(), self.version.clone()],
            vec!["Created".to_string(), self.created_at.0.clone()],
            vec!["Id".to_string(), self.id.inner().to_string()],
        ]);
        table.to_string()
    }

    fn render_list_table(items: &[Self]) -> String {
        let mut table = Table::new();
        table.set_header(vec![
            "Version name".to_string(),
            "Created".to_string(),
            "Id".to_string(),
        ]);
        table.add_rows(items.iter().map(|ver| {
            vec![
                ver.version.clone(),
                ver.created_at.0.clone(),
                ver.id.inner().to_string(),
            ]
        }));
        table.to_string()
    }
}
