#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ItemFormat {
    Json,
    Yaml,
    #[default]
    Table,
}

impl ItemFormat {
    pub fn render<T: CliRender>(self, item: &T) -> String {
        CliRender::render_item(item, self)
    }
}

impl std::str::FromStr for ItemFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "json" => Ok(ItemFormat::Json),
            "yaml" => Ok(ItemFormat::Yaml),
            "table" => Ok(ItemFormat::Table),
            other => Err(format!("Unknown output format: '{other}'")),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ListFormat {
    Json,
    Yaml,
    #[default]
    Table,
    ItemTable,
}

impl ListFormat {
    pub fn render<T: CliRender>(self, items: &[T]) -> String {
        CliRender::render_list(items, self)
    }
}

impl std::str::FromStr for ListFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "json" => Ok(ListFormat::Json),
            "yaml" => Ok(ListFormat::Yaml),
            "table" => Ok(ListFormat::Table),
            "item-table" => Ok(ListFormat::ItemTable),
            other => Err(format!("Unknown output format: '{other}'")),
        }
    }
}

pub trait CliRender: serde::Serialize + Sized {
    fn render_item(&self, format: ItemFormat) -> String {
        match format {
            ItemFormat::Json => serde_json::to_string_pretty(self).unwrap(),
            ItemFormat::Yaml => serde_yaml::to_string(self).unwrap(),
            ItemFormat::Table => self.render_item_table(),
        }
    }

    fn render_item_table(&self) -> String;

    fn render_list(items: &[Self], format: ListFormat) -> String {
        match format {
            ListFormat::Json => serde_json::to_string_pretty(items).unwrap(),
            ListFormat::Yaml => serde_yaml::to_string(items).unwrap(),
            ListFormat::Table => Self::render_list_table(items),
            ListFormat::ItemTable => {
                let mut out = String::new();

                for item in items {
                    out.push_str(&item.render_item_table());
                    out.push_str("\n\n");
                }

                out
            }
        }
    }

    fn render_list_table(items: &[Self]) -> String;
}
