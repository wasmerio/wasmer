---
name: wasmer-edge
description: Deploy and maintain apps on Wasmer Edge with the wasmer CLI and the registry.wasmer.io GraphQL API. Covers app.yaml, deploys and rollbacks, secrets, managed MySQL and Postgres, volumes, cron jobs, custom domains, logs, metrics, and autobuild from Git. Use when asked to deploy an app to Wasmer, set up a cronjob or database, read app logs, roll back a version, attach a domain, or debug production behavior such as unknown_domain responses, 502 errors on cold start, failed deploys, or secrets that do not apply.
license: MIT
metadata:
  author: wasmerio
---

# Deploy and maintain apps on Wasmer Edge

This skill is for apps that run on Wasmer Edge. For local runtime use, read
the `wasmer-local` skill. Facts here are verified against wasmer CLI 7.2.1
and the live production API. When this file and `wasmer <cmd> --help`
disagree, trust `--help`.

For GraphQL query and mutation signatures, read
[references/graphql.md](references/graphql.md).

## Credential rules

WARNING: Never ask a user to paste a token or secret value into the
conversation. Conversation logs persist. The user runs `wasmer login`
(browser flow) in their own terminal. Headless alternative: the user
exports `WASMER_TOKEN` and you reference `"$WASMER_TOKEN"` without
printing it. Never echo, log, or commit a secret value. If a secret leaks,
say so immediately and instruct rotation.

Make sure that `wasmer whoami` shows the expected user and registry before
any deploy. Dev machines often point at the wasmer.wtf dev registry.
Production is `https://registry.wasmer.io/graphql`.

## Deploy an app

1. If the app does not exist, create it from a template:
   `wasmer app create --template <name-or-github-url> --name <app>
   --owner <ns> [--deploy]`. Templates: https://wasmer.io/templates
   (for example `astro-starter`, `php-starter`, `js-worker`).
2. For an existing project, write an `app.yaml` (next section) and run:

```bash
wasmer deploy --owner <owner> --non-interactive
```

- `wasmer deploy` is an alias of `wasmer app deploy`. It reads `app.yaml`
  in the current directory. With `package: .` and a `wasmer.toml`
  present, it publishes the package first.
- For autobuild projects (no wasmer.toml), add `--build-remote`. Wasmer
  detects PHP, Python, static sites, Next.js, and Astro, and builds
  remotely. No Dockerfile is necessary.
- Each deploy creates a new app version (`dav_...`) and activates it.
  Deploy with `--no-default` to publish a canary version without
  activation. The canary gets its own URL. Promote it later (see
  Versions and rollback).
- Git-linked deploys: connect the GitHub repo from the app dashboard.
  After that, a push to the production branch deploys. Before the link
  exists, a push does nothing — keep using `wasmer deploy`.
- App identifier for all `wasmer app` commands: `owner/name`, an alias,
  or a `da_` ID. When omitted, the CLI reads `app.yaml` in the cwd.

## app.yaml

```yaml
kind: wasmer.io/App.v0
name: my-app                  # lowercase, digits, inner hyphens only
owner: my-namespace
package: .                    # or a registry package like wasmer/my-app
env:
  MY_VAR: "value"
locality:
  regions: [fr-roub1]         # default: all regions
capabilities:
  database:
    engine: postgres          # or mysql; omitted means mysql
  instaboot:
    requests:
      - path: /
volumes:
  - name: data
    mount: /data
jobs:
  - name: nightly
    trigger: '0 0 * * *'
    action:
      execute:
        command: bash
        cli_args: ["-lc", "date -u"]
```

Other keys: `description`, `cli_args`, `debug: true` (detailed error
responses), `enable_email: true` (provides `sendmail`),
`redirect: { force_https: false }` (default true), `health_checks`
(HTTP checks; failures restart the instance),
`scaling: { mode: single_concurrency }` (one request per instance, for
single-threaded runtimes), `capabilities.cdn_cache: { enabled: true }`,
`capabilities.ssh`. The deploy writes `app_id` back into the file.

`app.yaml` is not part of the package hash. A config-only change still
creates a new app version.

## Secrets

```bash
wasmer app secrets create MY_SECRET "$MY_SECRET" --app <owner>/<app> --redeploy
wasmer app secrets list                 # names only, never values
wasmer app secrets reveal MY_SECRET     # or: reveal --all (.env format)
wasmer app secrets import --from-file .env --update-existing --redeploy
```

- Pass values as arguments from an env var reference. Piping a value in
  stores the literal pipe path as the value.
- CAUTION: A created or updated secret reaches the app on the next
  deploy only. Pass `--redeploy` to apply it immediately.
- Database credentials appear as secrets named `DB_*`.

## Database

- Declare `capabilities.database.engine` and exactly one database-capable
  region (`fr-roub1` or `ca-beau1`). One database per app. The engine is
  immutable after creation.
- The platform injects exactly five env vars: `DB_HOST`, `DB_PORT`,
  `DB_NAME`, `DB_USERNAME`, `DB_PASSWORD`. There is no `DATABASE_URL`.
  Ports are not standard — never hardcode 3306 or 5432.
- TLS is required, but the certificate chain is from a private CA.
  Configure clients for TLS without chain verification:
  `sslmode=require` for psql, `ssl: { rejectUnauthorized: false }` for
  node-pg. Do not turn TLS off.
- Connect locally: `wasmer app database list --with-password`, then
  `psql "postgresql://<u>:<p>@<host>:<port>/<db>?sslmode=require"`.
- Instances multiply connections. Keep per-instance pools at 1 to 3.

## Volumes

- Declare `volumes` in app.yaml and deploy to create them. Volumes are
  persistent, S3-accessible, region-locked disks.
- WARNING: Do not rename or remove a volume in app.yaml unless data loss
  is intended. The next deploy deletes the volume's data unrecoverably.
- Access files with S3 credentials:
  `wasmer app volume credentials --format=rclone`, then
  `rclone copy ./file <target>:<volume>/`. Rotate with
  `wasmer app volume rotate-secrets`.
- Writes propagate to other instances with a delay of some seconds.
  Retry before other diagnosis.

## Cron jobs

- Two action types. `execute` starts an instance of the app package and
  runs a command with the app's volumes and secrets present. `fetch`
  sends an HTTP request to the app. Prefer `execute` for your own code —
  it needs no endpoint and no auth token.
- If `execute` has no `command`, the job boots the app's default
  long-lived server command. Set `command` explicitly.
- Schedules must have a uniform interval: `*/15 * * * *` is valid,
  `0 9 * * MON-FRI` is rejected. Minimum interval: 5 minutes.
  `pre-deployment` and `post-deployment` triggers run once per deploy.
- Jobs have two sources. `CONFIG` jobs come from app.yaml and propagate
  on deploy. `API` jobs come from GraphQL mutations. Never define one
  job in both — it runs twice.
- The job `timeout` is not strictly enforced. Missed runs never catch
  up. Per-minute fetch jobs defeat scale-to-zero and create a cost floor.
- Per-run status and stdout are only in GraphQL: query `cronJobs` →
  `invocations` → `logs` (see references/graphql.md).
- Free diagnostic: a temporary fast-schedule `execute` job with
  `command: bash`, `cli_args: ["-lc", "env | cut -d= -f1; ls /data"]`
  shows the exec context. Print env names, never values.

## Logs and observability

```bash
wasmer app logs <owner>/<app> --from 10m --max 1000 \
  --streams stdout --streams stderr --watch -f json
```

- `--from`/`--until` accept RFC3339, dates, unix timestamps, and
  positive relative forms (`10m`, `1d1h`). A negative form like `-20m`
  parses as a flag — do not use it. Default window: last 10 minutes.
- An empty result or a 504 from the log pipeline does not prove the app
  is down. Retry and widen the window before conclusions.
- WCGI apps use stdout for the HTTP response. They can only log to
  stderr. Proxy-mode apps capture both streams.
- Build and rollout logs are separate: `wasmer app deployment list`,
  then `wasmer app deployment logs <ID>`. Autobuild errors appear only
  there, not in app logs.
- Quick status: `wasmer app get -f json` shows `state` and
  `activeVersion.disabledAt/disabledReason`. Response headers
  `x-edge-request-outcome`, `x-edge-app-version-id`, and `x-edge-region`
  identify what served a request.
- Interactive shell on Edge: `wasmer ssh -a <owner>/<app>`.

## Versions and rollback

```bash
wasmer app version list <owner>/<app>
wasmer app version activate <dav_...>
```

`activate` takes the `dav_` version ID, not the version name `v3`.
The safe upgrade path: deploy with `--no-default`, test the version's
own URL, then activate it.

## Domains

- Dashboard: app → Settings → Domains → Add Domain. CLI:
  `wasmer domain list|get|get-zone-file|sync-zone-file|register`.
- The API returns the exact DNS records to create
  (`expectedDnsRecords`). Show these to the user — do not guess record
  values. If the user is on Cloudflare, disable the proxy during
  validation.
- After a domain change, deploy a new version. Old default domains can
  stay claimed by the router until a new version regenerates the config.

## Runtime semantics

Instances are stateless and ephemeral. Edge starts them on demand and
stops them after some minutes of idle time. Instance lifetime is bounded
(about one hour) — design jobs to be idempotent and request-bounded.
A new package hash can cause a slow first boot per node while it
compiles. `capabilities.instaboot` removes most of that cost by
snapshotting a warmed-up instance. `wasmer app purge-cache` resets the
snapshots.

## Investigate failures

1. `wasmer app get -f json`: read `state`, `disabledAt`, and
   `disabledReason` first. An `unknown_domain` response means router lag
   (seconds), a banned app, a usage-disabled app, or a deleted app — not
   only DNS.
2. A 502 or empty 500 during cold start is usually compile or boot time.
   Retry, then read `--streams stderr` and the RUNTIME log stream.
3. A failed deploy with a rollout that dies in seconds is a config
   error. Read `wasmer app deployment logs <ID>`.
4. A name or owner mismatch rejects the deploy: `app.yaml` `name`,
   `owner`, and `app_id` must match the existing app.
5. For "my change is not live": secrets and volumes apply on the next
   deploy; running instances keep old env until replaced.

## Gotchas

- Avoid GraphQL `redeployActiveVersion` for config repair. It clones the
  frozen generated config. A real deploy regenerates from your app.yaml.
- Deleting an app does not always stop its cron jobs at once. Make sure
  that invocations stopped.
- `capabilities.database.engine: psql` is rejected. An omitted engine
  silently means MySQL.
- Trust `DeployApp.url` and `permalink` from the API over a constructed
  `<name>.wasmer.app` URL. Aliases are globally unique and can differ.
- `wasmer deploy` rejects a changed `name`. Rename via GraphQL
  `renameApp`, then update app.yaml to match, or attach a custom domain
  and keep the name.
- Some `wasmer app` help descriptions are wrong (`database` and
  `deployment` say "volume management"). The commands work as named.
