# GraphQL cookbook for registry.wasmer.io

Endpoint: `https://registry.wasmer.io/graphql`. A GraphiQL IDE runs at the
same URL. Introspection is public — when unsure about a field, introspect
instead of guessing.

```bash
curl -s https://registry.wasmer.io/graphql \
  -H "Authorization: Bearer $WASMER_TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"query":"query { viewer { username } }"}'
```

- Auth scheme is `Bearer` (the backend also accepts `JWT`). There is no
  `Token` scheme.
- Token prefixes: `wap_` personal API token, `dt_` deploy token, `wott_`
  one-time token (revoked on first use — never reuse one).
- Pagination is Relay style: `(first, after, last, before, offset)` with
  `pageInfo { hasNextPage endCursor }`. `CronJob.invocations` is
  forward-only.
- `node(id:)` resolves global IDs: `da_` app, `dav_` app version, `u_`
  user, `daa_` alias, `avol_` volume, `appdb_` database, `ro_` rollout.
- Timestamp quirk: `Log.timestamp` is a Float in nanoseconds. The
  `logs(startingFrom:, until:)` arguments are Floats in epoch seconds.
  The `startingFromISO` argument takes an ISO 8601 DateTime.
- These names do not exist — do not invent them: `deployApp` (mutation),
  `activateApp`, `getDeployAppByAlias`.
- `getDeployApp` on a private or banned app returns `null`, not an error.

## Read an app

```graphql
query {
  getDeployApp(name: "my-app", owner: "my-ns") {
    id state url urls permalink adminUrl
    activeVersion { id version disabledAt disabledReason }
    versions(first: 10) { edges { node { id version isActive url } } }
    envVars(first: 50) { edges { node { name value sensitive } } }
    databases { name username host port password engine }
    volumes { volumeId mountPath s3Url }
    domains { edges { node { name state } } }
  }
}
```

Also: `getAppByGlobalAlias(alias:)`, `getDeployAppVersion(name:, owner:,
version:)`, `viewer { apps(first: 20) { ... } }`.

## Rollback / promote a version

```graphql
mutation {
  markAppVersionAsActive(input: { appVersion: "dav_..." }) {
    app { activeVersion { version } }
  }
}
```

## Env vars and secrets

`updateEnvVars` is the current API. The older `upsertAppSecret(s)` and
`deleteAppSecret` mutations are deprecated but functional.

```graphql
mutation {
  updateEnvVars(input: { appId: "da_...", envVars: [
    { name: "MY_SECRET", value: "v", sensitive: true },
    { name: "OLD_VAR", delete: true }
  ] }) { success }
}
```

Changes apply on the next deploy, the same as the CLI.

## Cron jobs

Kind (`FETCH` or `EXECUTE`) is implied by which block you pass.
`timeout` and `maxScheduleDrift` are strings like `"10m"`.

```graphql
mutation {
  createCronJob(input: {
    appId: "da_...", name: "my-job", schedule: "*/15 * * * *",
    enabled: true, timeout: "10m", maxRetries: 2,
    execute: { command: "bash", cliArgs: ["-lc", "date -u"] }
    # or: fetch: { path: "/cron", method: "POST",
    #              expectStatusCodes: [200] }
  }) { cronJob { id } }
}
```

Also: `updateCronJob(input: { cronJobId, ... })`,
`toggleCronJob(input: { cronJobId, enabled })`,
`deleteCronJob(input: { cronJobId })`. `CONFIG`-sourced jobs are not
editable via the API — edit app.yaml and deploy.

Per-run readback (the only path to job stdout):

```graphql
query { getDeployApp(name: "x", owner: "y") {
  cronJobs(first: 10) { edges { node {
    id name schedule enabled source kind
    invocations(first: 5) { edges { node {
      status scheduledAt durationMs errorSummary
      result {
        ... on ExecuteCronJobInvocationResult { exitCode instanceId }
        ... on FetchCronJobInvocationResult { statusCode responseBody }
      }
      logs(first: 100) { edges { node { datetime message stream } } }
    } } }
  } } } } }
```

`status` is one of `PENDING`, `RUNNING`, `SUCCESS`, `FAILURE`.

## Logs

```graphql
query { getDeployAppVersion(name: "x", owner: "y") {
  logs(startingFromISO: "2026-08-14T00:00:00Z",
       streams: [STDOUT, STDERR, RUNTIME], first: 100)
    { edges { node { datetime message stream instanceId } } }
} }
```

The `RUNTIME` stream carries platform messages: instance starts, stops,
and errors.

## Metrics

```graphql
query { getDeployApp(name: "x", owner: "y") {
  usageMetrics(variant: no_of_requests) { ... }
  groupedMetrics(startAt: "...", endAt: "...", groupedBy: DAY) { ... }
} }
```

Variants are lowercase: `cpu_time`, `memory_time`, `network_egress`,
`network_ingress`, `no_of_requests`, `no_of_failed_requests`, `cost`.

## Domains

```graphql
mutation { registerDomain(input: { name: "example.com",
                                   namespace: "my-ns" }) { ... } }
mutation { upsertAppDomain(input: { appId: "da_...",
    name: "www.example.com" }) {
  # returns the alias with state and the DNS records to create:
  # expectedDnsRecords { recordType host value }
} }
mutation { verifyAppDomain(input: { domainId: "...", kind: QUICK }) { ... } }
```

Alias `state`: `UNVERIFIED`, `VERIFIED`, `APEX_WITHOUT_REDIRECTION`.

## Autobuild from a repo

```graphql
mutation { deployViaAutobuild(input: {
  repoUrl: "https://github.com/me/repo", branch: "main",
  appName: "my-app", owner: "my-ns",
  enableDatabase: true, dbEngine: POSTGRES
}) { success buildId appToken } }
```

Poll `autobuildDeploymentStatus(buildId:)` for
`QUEUED | WORKING | RUNNING | SUCCESS | FAILED | TIMEOUT`. Build logs
exist only in the `fetchBuildLogs(buildId:)` subscription stream, not in
app logs.

## Other maintenance mutations

- `deleteApp(input: { id: "da_..." })`
- `renameApp(input: { id: "da_...", name: "new-name" })` — CAUTION:
  failures return `{"renameApp": null}` with no GraphQL error.
- `rotateCredentialsForAppDb` — rotates username and password. The app
  receives new values without a new deploy.
- `purgeCacheForAppVersion(input: { id: "dav_..." })` — reset InstaBoot.
- `migrateAppRegion(input: { appId, regionName })`
- `runEdgeCommand(input: { appId, command, timeoutSeconds })
  { stdout stderr exitCode }` — one-shot remote exec.
- `generateDeployToken(input: { deployConfigVersionId: "<dav_ id>" })` —
  scoped CI tokens.
