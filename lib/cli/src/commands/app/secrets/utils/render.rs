use super::{BackendSecretWrapper, Secret};
use crate::utils::render::CliRender;
use colored::Colorize;
use comfy_table::{Cell, Table};
use time::OffsetDateTime;
use wasmer_backend_api::types::{DateTime, Secret as BackendSecret};

impl CliRender for Secret {
    fn render_item_table(&self) -> String {
        let mut table = Table::new();
        let Secret { name, value }: &Secret = self;

        table.load_preset(comfy_table::presets::NOTHING);
        table.set_content_arrangement(comfy_table::ContentArrangement::Dynamic);

        let value = sanitize_value(value);
        table.add_rows([
            vec![
                Cell::new("Name".to_string()).add_attribute(comfy_table::Attribute::Bold),
                Cell::new("Value".to_string()).add_attribute(comfy_table::Attribute::Bold),
            ],
            vec![Cell::new(name.to_string()), Cell::new(format!("'{value}'"))],
        ]);
        table.to_string()
    }

    fn render_list_table(items: &[Self]) -> String {
        if items.is_empty() {
            return String::new();
        }
        let mut table = Table::new();
        table.load_preset(comfy_table::presets::NOTHING);
        table.set_content_arrangement(comfy_table::ContentArrangement::Dynamic);

        table.set_header(vec![
            Cell::new("Name".to_string()).add_attribute(comfy_table::Attribute::Bold),
            Cell::new("Value".to_string()).add_attribute(comfy_table::Attribute::Bold),
        ]);
        table.add_rows(items.iter().map(|s| {
            vec![
                Cell::new(s.name.clone()),
                Cell::new(format!("'{}'", sanitize_value(&s.value))),
            ]
        }));
        table.to_string()
    }
}

impl CliRender for BackendSecretWrapper {
    fn render_item_table(&self) -> String {
        let mut table = Table::new();
        let BackendSecret {
            name, updated_at, ..
        }: &BackendSecret = &self.0;
        let last_updated = last_updated_to_human(updated_at.clone())
            .unwrap()
            .to_string();
        table.add_rows([
            vec!["Name".to_string(), name.to_string()],
            vec![
                "Last updated".to_string(),
                format!("{last_updated} ago").dimmed().to_string(),
            ],
        ]);
        table.to_string()
    }

    fn render_list_table(items: &[Self]) -> String {
        if items.is_empty() {
            return String::new();
        }
        let mut table = Table::new();
        table.load_preset(comfy_table::presets::NOTHING);
        table.set_content_arrangement(comfy_table::ContentArrangement::Dynamic);

        table.set_header(vec![
            Cell::new("Name".to_string()).add_attribute(comfy_table::Attribute::Bold),
            Cell::new("Last updated".to_string()).add_attribute(comfy_table::Attribute::Bold),
        ]);
        table.add_rows(items.iter().map(|s| {
            let last_updated = last_updated_to_human(s.0.updated_at.clone())
                .unwrap()
                .to_string();
            vec![
                Cell::new(s.0.name.clone()),
                Cell::new(format!("{last_updated} ago").dimmed().to_string()),
            ]
        }));
        table.to_string()
    }
}

fn last_updated_to_human(last_update: DateTime) -> anyhow::Result<humantime::Duration> {
    let last_update: OffsetDateTime = last_update.try_into()?;
    let elapsed: std::time::Duration = (OffsetDateTime::now_utc() - last_update).try_into()?;
    Ok(humantime::Duration::from(std::time::Duration::from_secs(
        elapsed.as_secs(),
    )))
}

pub(crate) fn sanitize_value(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_ascii() {
                let c = c as u8;
                std::ascii::escape_default(c).to_string()
            } else {
                c.to_string()
            }
        })
        .collect::<Vec<String>>()
        .join("")
}
