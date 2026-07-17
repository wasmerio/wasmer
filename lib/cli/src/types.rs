use colored::Colorize;
use comfy_table::Table;
use wasmer_backend_api::types::{
    CronJob, CronJobInvocation, CronJobInvocationResult, CronJobKind, CronJobLog, CronJobTarget,
    DeployApp, DeployAppVersion, Deployment, DnsDomain, DnsDomainWithRecords, LogStream, Namespace,
    SearchPackageVersion,
};

use crate::utils::render::CliRender;

/// Render the full name (`namespace/name`) of a package version's package.
fn package_full_name(pv: &SearchPackageVersion) -> String {
    match &pv.package.namespace {
        Some(ns) => format!("{ns}/{}", pv.package.package_name),
        None => pv.package.package_name.clone(),
    }
}

fn cron_job_target_summary(target: &CronJobTarget) -> String {
    match target {
        CronJobTarget::FetchCronJobTarget(target) => {
            format!("{} {}", target.method, target.path)
        }
        CronJobTarget::ExecuteCronJobTarget(target) => {
            let package = target.package_name.as_deref().unwrap_or("-");
            let command = target.command.as_deref().unwrap_or("-");
            format!("{package} {command}")
        }
        CronJobTarget::Unknown => "unknown".to_string(),
    }
}

fn cron_job_kind(kind: CronJobKind) -> &'static str {
    match kind {
        CronJobKind::Fetch => "fetch",
        CronJobKind::Execute => "execute",
    }
}

fn cron_invocation_result_summary(result: &Option<CronJobInvocationResult>) -> String {
    match result {
        Some(CronJobInvocationResult::ExecuteCronJobInvocationResult(result)) => result
            .exit_code
            .map(|code| format!("exit_code={code}"))
            .unwrap_or_else(|| "execute".to_string()),
        Some(CronJobInvocationResult::FetchCronJobInvocationResult(result)) => result
            .status_code
            .map(|code| format!("status_code={code}"))
            .unwrap_or_else(|| "fetch".to_string()),
        Some(CronJobInvocationResult::Unknown) => "unknown".to_string(),
        None => "-".to_string(),
    }
}

impl CliRender for CronJob {
    fn render_item_table(&self) -> String {
        let mut table = Table::new();
        table.add_rows([
            vec!["Name".to_string(), self.name.clone()],
            vec!["Id".to_string(), self.id.inner().to_string()],
            vec!["Kind".to_string(), cron_job_kind(self.kind).to_string()],
            vec!["Schedule".to_string(), self.schedule.clone()],
            vec!["Enabled".to_string(), self.enabled.to_string()],
            vec!["Managed".to_string(), self.is_managed.to_string()],
            vec!["Target".to_string(), cron_job_target_summary(&self.target)],
            vec![
                "Max retries".to_string(),
                self.max_retries
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "-".to_string()),
            ],
            vec![
                "Timeout".to_string(),
                self.timeout.clone().unwrap_or_else(|| "-".to_string()),
            ],
            vec!["Created".to_string(), self.created_at.0.clone()],
            vec!["Updated".to_string(), self.updated_at.0.clone()],
        ]);
        table.to_string()
    }

    fn render_list_table(items: &[Self]) -> String {
        let mut table = Table::new();
        table.set_header(vec![
            "Name".to_string(),
            "Kind".to_string(),
            "Schedule".to_string(),
            "Enabled".to_string(),
            "Target".to_string(),
            "Id".to_string(),
        ]);
        table.add_rows(items.iter().map(|job| {
            vec![
                job.name.clone(),
                cron_job_kind(job.kind).to_string(),
                job.schedule.clone(),
                job.enabled.to_string(),
                cron_job_target_summary(&job.target),
                job.id.inner().to_string(),
            ]
        }));
        table.to_string()
    }
}

impl CliRender for CronJobInvocation {
    fn render_item_table(&self) -> String {
        let mut table = Table::new();
        table.add_rows([
            vec!["Id".to_string(), self.id.inner().to_string()],
            vec!["Edge job id".to_string(), self.edge_job_id.clone()],
            vec![
                "Status".to_string(),
                self.status
                    .map(|status| format!("{status:?}"))
                    .unwrap_or_else(|| "-".to_string()),
            ],
            vec![
                "Scheduled".to_string(),
                self.scheduled_at
                    .as_ref()
                    .map(|value| value.0.clone())
                    .unwrap_or_else(|| "-".to_string()),
            ],
            vec![
                "Started".to_string(),
                self.started_at
                    .as_ref()
                    .map(|value| value.0.clone())
                    .unwrap_or_else(|| "-".to_string()),
            ],
            vec![
                "Finished".to_string(),
                self.finished_at
                    .as_ref()
                    .map(|value| value.0.clone())
                    .unwrap_or_else(|| "-".to_string()),
            ],
            vec![
                "Duration ms".to_string(),
                self.duration_ms
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "-".to_string()),
            ],
            vec![
                "Result".to_string(),
                cron_invocation_result_summary(&self.result),
            ],
            vec![
                "Error".to_string(),
                self.error_summary
                    .clone()
                    .unwrap_or_else(|| "-".to_string()),
            ],
        ]);
        table.to_string()
    }

    fn render_list_table(items: &[Self]) -> String {
        let mut table = Table::new();
        table.set_header(vec![
            "Id".to_string(),
            "Status".to_string(),
            "Scheduled".to_string(),
            "Started".to_string(),
            "Duration ms".to_string(),
            "Result".to_string(),
        ]);
        table.add_rows(items.iter().map(|invocation| {
            vec![
                invocation.id.inner().to_string(),
                invocation
                    .status
                    .map(|status| format!("{status:?}"))
                    .unwrap_or_else(|| "-".to_string()),
                invocation
                    .scheduled_at
                    .as_ref()
                    .map(|value| value.0.clone())
                    .unwrap_or_else(|| "-".to_string()),
                invocation
                    .started_at
                    .as_ref()
                    .map(|value| value.0.clone())
                    .unwrap_or_else(|| "-".to_string()),
                invocation
                    .duration_ms
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "-".to_string()),
                cron_invocation_result_summary(&invocation.result),
            ]
        }));
        table.to_string()
    }
}

impl CliRender for CronJobLog {
    fn render_item_table(&self) -> String {
        let mut table = Table::new();
        table.add_rows([
            vec!["Timestamp".to_string(), self.datetime.0.clone()],
            vec!["Message".to_string(), self.message.clone()],
        ]);
        table.to_string()
    }

    fn render_list_table(items: &[Self]) -> String {
        let mut table = Table::new();
        table.load_preset(comfy_table::presets::NOTHING);
        table.set_content_arrangement(comfy_table::ContentArrangement::Dynamic);

        for item in items {
            let message = match item.stream {
                Some(LogStream::Stderr) => item.message.clone().yellow(),
                Some(LogStream::Runtime) => item.message.clone().cyan(),
                Some(LogStream::Stdout) | None => item.message.clone().bold(),
            };
            table.add_row([
                comfy_table::Cell::new(format!("[{}]", item.datetime.0))
                    .set_alignment(comfy_table::CellAlignment::Right),
                comfy_table::Cell::new(message),
            ]);
        }
        table.to_string()
    }
}

impl CliRender for SearchPackageVersion {
    fn render_item_table(&self) -> String {
        let mut table = Table::new();
        table.add_rows([
            vec!["Package".to_string(), package_full_name(self)],
            vec!["Version".to_string(), self.version.clone()],
            vec!["Created".to_string(), self.created_at.0.clone()],
        ]);
        table.to_string()
    }

    fn render_list_table(items: &[Self]) -> String {
        let mut table = Table::new();
        table.set_header(vec![
            "Package".to_string(),
            "Version".to_string(),
            "Created".to_string(),
        ]);
        table.add_rows(items.iter().map(|pv| {
            vec![
                package_full_name(pv),
                pv.version.clone(),
                pv.created_at.0.clone(),
            ]
        }));
        table.to_string()
    }
}

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
            vec!["Updated".to_string(), self.updated_at.0.clone()],
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
            "Updated".to_string(),
            "Id".to_string(),
        ]);
        table.add_rows(items.iter().map(|app| {
            vec![
                format!("{}/{}", app.owner.global_name, app.name),
                app.active_version
                    .as_ref()
                    .map_or_else(|| "n/a".to_string(), |v| v.version.clone()),
                app.created_at.0.clone(),
                app.updated_at.0.clone(),
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

pub(crate) fn format_disk_size_opt(value: Option<wasmer_backend_api::types::BigInt>) -> String {
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
