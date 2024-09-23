/// Formatting options for a single item.
#[derive(clap::Parser, Debug, Default)]
pub struct ItemFormatOpts {
    /// Output format. (yaml, json, table)
    #[clap(short = 'f', long, default_value = "yaml")]
    pub format: crate::utils::render::ItemFormat,
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
