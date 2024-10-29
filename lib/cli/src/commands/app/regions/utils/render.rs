use crate::utils::render::CliRender;
use comfy_table::{Cell, Table};
use wasmer_backend_api::types::AppRegion;

impl CliRender for AppRegion {
    fn render_item_table(&self) -> String {
        let mut table = Table::new();
        let AppRegion {
            name,
            city,
            country,
            ..
        }: &AppRegion = self;

        table.load_preset(comfy_table::presets::NOTHING);
        table.set_content_arrangement(comfy_table::ContentArrangement::Dynamic);

        table.add_rows([
            vec![
                Cell::new("Name".to_string()).add_attribute(comfy_table::Attribute::Bold),
                Cell::new("City".to_string()).add_attribute(comfy_table::Attribute::Bold),
                Cell::new("Country code".to_string()).add_attribute(comfy_table::Attribute::Bold),
            ],
            vec![
                Cell::new(name.to_string()).add_attribute(comfy_table::Attribute::Bold),
                Cell::new(city.to_string()),
                Cell::new(country.to_string()),
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
        //table.set_content_arrangement(comfy_table::ContentArrangement::Dynamic);

        table.set_header(vec![
            Cell::new("Name".to_string()).add_attribute(comfy_table::Attribute::Bold),
            Cell::new("City".to_string()).add_attribute(comfy_table::Attribute::Bold),
            Cell::new("Country code".to_string()).add_attribute(comfy_table::Attribute::Bold),
        ]);
        table.add_rows(items.iter().map(|s| {
            vec![
                Cell::new(s.name.to_string()).add_attribute(comfy_table::Attribute::Bold),
                Cell::new(s.city.to_string()),
                Cell::new(s.country.to_string()),
            ]
        }));
        table.to_string()
    }
}
