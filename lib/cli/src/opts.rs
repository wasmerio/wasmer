use crate::utils::render::ItemFormat;

/// Formatting options for a single item.
#[derive(clap::Parser, Debug, Default)]
pub struct ItemFormatOpts {
    /// Output format. (yaml, json, table)
    ///
    /// This value is optional instead of using a default value to allow code
    /// to distinguish between the user not specifying a value and a generic
    /// default.
    ///
    /// Code should usually use [`Self::get`] to use the same default format.
    #[clap(short = 'f', long)]
    pub format: Option<ItemFormat>,
}

impl ItemFormatOpts {
    /// Get the output format, defaulting to `ItemFormat::Yaml`.
    pub fn get(&self) -> crate::utils::render::ItemFormat {
        self.format
            .unwrap_or(crate::utils::render::ItemFormat::Table)
    }

    /// Get the output format, defaulting to the given value if not specified.
    pub fn get_with_default(&self, default: ItemFormat) -> crate::utils::render::ItemFormat {
        self.format.unwrap_or(default)
    }
}

/// Formatting options for a single item.
#[derive(clap::Parser, Debug, Default)]
pub struct ItemTableFormatOpts {
    /// Output format. (yaml, json, table)
    #[clap(short = 'f', long, default_value = "table")]
    pub format: crate::utils::render::ItemFormat,
}

/// Formatting options for a list of items.
#[derive(clap::Parser, Debug)]
pub struct ListFormatOpts {
    /// Output format. (yaml, json, table, item-table)
    #[clap(short = 'f', long, default_value = "table")]
    pub format: crate::utils::render::ListFormat,
}
