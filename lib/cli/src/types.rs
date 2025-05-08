use comfy_table::Table;
use wasmer_backend_api::types::{
    DeployApp, DeployAppVersion, Deployment, DnsDomain, DnsDomainWithRecords, Namespace,
};

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
            vec![
                "Version".to_string(),
                self.active_version
                    .as_ref()
                    .map_or_else(|| "n/a".to_string(), |v| v.version.clone()),
            ],
            vec!["Created".to_string(), self.created_at.0.clone()],
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
                app.active_version
                    .as_ref()
                    .map_or_else(|| "n/a".to_string(), |v| v.version.clone()),
                app.created_at.0.clone(),
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

impl CliRender for wasmer_backend_api::types::AppVersionVolume {
    fn render_item_table(&self) -> String {
        let mut table = Table::new();
        table.add_rows([
            vec!["Name".to_string(), self.name.clone()],
            vec![
                "Used size".to_string(),
                format_disk_size_opt(self.used_size.clone()),
            ],
        ]);
        table.to_string()
    }

    fn render_list_table(items: &[Self]) -> String {
        let mut table = Table::new();
        table.set_header(vec!["Name".to_string(), "Used size".to_string()]);
        table.add_rows(items.iter().map(|vol| {
            vec![
                vol.name.clone(),
                format_disk_size_opt(vol.used_size.clone()),
            ]
        }));
        table.to_string()
    }
}

impl CliRender for wasmer_backend_api::types::AppDatabase {
    fn render_item_table(&self) -> String {
        let mut table = Table::new();
        table.add_rows([
            vec!["Name".to_string(), self.name.clone()],
            vec!["Host".to_string(), self.host.clone()],
            vec!["Port".to_string(), self.port.clone()],
            vec!["Username".to_string(), self.username.clone()],
            vec![
                "Password".to_string(),
                self.password.clone().unwrap_or_else(|| "n/a".to_string()),
            ],
            vec![
                "UI".to_string(),
                self.db_explorer_url
                    .clone()
                    .unwrap_or_else(|| "n/a".to_string()),
            ],
        ]);
        table.to_string()
    }

    fn render_list_table(items: &[Self]) -> String {
        let mut table = Table::new();
        table.set_header(vec![
            "Name".to_string(),
            "Host".to_string(),
            "Port".to_string(),
            "UI".to_string(),
            "Password".to_string(),
        ]);
        table.add_rows(items.iter().map(|vol| {
            vec![
                vol.name.clone(),
                vol.host.clone(),
                vol.port.clone(),
                vol.db_explorer_url
                    .clone()
                    .unwrap_or_else(|| "n/a".to_string()),
                vol.password.clone().unwrap_or_else(|| "n/a".to_string()),
            ]
        }));
        table.to_string()
    }
}

fn format_disk_size_opt(value: Option<wasmer_backend_api::types::BigInt>) -> String {
    let value = value.and_then(|x| {
        let y: Option<u64> = x.0.try_into().ok();
        y
    });

    if let Some(v) = value {
        let s = bytesize::ByteSize(v);
        s.to_string()
    } else {
        "n/a".to_string()
    }
}

impl CliRender for Deployment {
    fn render_item_table(&self) -> String {
        match self {
            Deployment::NakedDeployment(naked) => naked.render_item_table(),
            Deployment::AutobuildRepository(build) => build.render_item_table(),
            Deployment::Other => "unknown deployment type".to_string(),
        }
    }

    fn render_list_table(items: &[Self]) -> String {
        let mut table = Table::new();
        table.set_header(vec![
            "Id".to_string(),
            "Type".to_string(),
            "Created at".to_string(),
            "Status".to_string(),
            "App version".to_string(),
        ]);

        let rows = items
            .iter()
            .map(|item| match item {
                Deployment::NakedDeployment(naked) => {
                    vec![
                        naked.id.inner().to_string(),
                        "Manual".to_string(),
                        naked.created_at.0.clone(),
                        String::new(),
                        naked
                            .app_version
                            .as_ref()
                            .map_or_else(|| "n/a".to_string(), |x| x.version.clone()),
                    ]
                }
                Deployment::AutobuildRepository(build) => {
                    vec![
                        build.id.inner().to_string(),
                        "Autobuild".to_string(),
                        build.status.as_str().to_string(),
                        build.created_at.0.clone(),
                    ]
                }
                Deployment::Other => vec![
                    String::new(),
                    "Unknown".to_string(),
                    String::new(),
                    String::new(),
                ],
            })
            .collect::<Vec<_>>();
        table.add_rows(rows);

        table.to_string()
    }
}

impl CliRender for wasmer_backend_api::types::NakedDeployment {
    fn render_item_table(&self) -> String {
        let mut table = Table::new();
        table.add_rows([
            vec!["Id".to_string(), self.id.clone().into_inner()],
            vec!["Created at".to_string(), self.created_at.0.clone()],
            vec![
                "App version".to_string(),
                self.app_version
                    .as_ref()
                    .map_or_else(|| "n/a".to_string(), |x| x.version.clone()),
            ],
        ]);
        table.to_string()
    }

    fn render_list_table(items: &[Self]) -> String {
        let mut table = Table::new();
        table.set_header(vec![
            "Id".to_string(),
            "Created at".to_string(),
            "App version".to_string(),
        ]);
        table.add_rows(items.iter().map(|item| {
            vec![
                item.id.clone().into_inner(),
                item.created_at.0.clone(),
                item.app_version
                    .as_ref()
                    .map_or_else(|| "n/a".to_string(), |x| x.version.clone()),
            ]
        }));
        table.to_string()
    }
}

impl CliRender for wasmer_backend_api::types::AutobuildRepository {
    fn render_item_table(&self) -> String {
        let mut table = Table::new();
        table.add_rows([
            vec!["Id".to_string(), self.id.clone().into_inner()],
            vec!["Status".to_string(), self.status.as_str().to_string()],
            vec!["Created at".to_string(), self.created_at.0.clone()],
        ]);
        table.to_string()
    }

    fn render_list_table(items: &[Self]) -> String {
        let mut table = Table::new();
        table.set_header(vec![
            "Id".to_string(),
            "Status".to_string(),
            "Created at".to_string(),
        ]);
        table.add_rows(items.iter().map(|item| {
            vec![
                item.id.clone().into_inner(),
                item.status.as_str().to_string(),
                item.created_at.0.clone(),
            ]
        }));
        table.to_string()
    }
}
