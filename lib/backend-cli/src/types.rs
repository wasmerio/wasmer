use comfy_table::Table;
use wasmer_api::types::{DeployApp, DeployAppVersion, Namespace};

use crate::util::render::CliRender;

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
