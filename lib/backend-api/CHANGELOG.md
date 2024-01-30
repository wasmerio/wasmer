# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## 0.0.1 (2023-04-27)

* Removed legacy API implementation in favour of the new cynic client
* Fixed log querying
* Added methods for retrieving DeployApp/Version by unique id

## 0.1.0-alpha.1 (2023-03-29)

<csr-id-7195702c618c0fb0937244034ee4600531b97034/>
<csr-id-b8b9af7429a8c9b0880be9bf72f1b8a732d05d75/>
<csr-id-a40f5127796316069aa2ac646ea5c3817f7a29fa/>
<csr-id-065513b5f6ee350b9f2607d5aa7003df352f5cbc/>
<csr-id-38336ebe12a85b142871a4c0fa50541df815a441/>
<csr-id-8febcde88d1d7df8a7a86c79afcf960c2cb868a3/>
<csr-id-33f822d9701e87c0f0564574a76adef83ccf72a5/>
<csr-id-d968694ade1191afff80e6d141598b9c10b2f3c7/>
<csr-id-b1245bfa194e05d49809973b716e2565689bfccf/>

### Refactor (BREAKING)

 - <csr-id-7195702c618c0fb0937244034ee4600531b97034/> Rename wasmer-deploy-core to wasmer-deploy-schema
   -schema is a more sensible / expressive name for the crate,
   since it just holds type definitions.
   
   Done in preparation for publishing the crate, since it will need to be
   used by downstream consumers like the Wasmer repo

### Chore

 - <csr-id-bbc3105d5f04bc4c35dad794443f53980106981e/> Add description to wasmer-api Cargo.toml
   Required for releasing.

### Other

 - <csr-id-b8b9af7429a8c9b0880be9bf72f1b8a732d05d75/> Dependency cleanup
   * Lift some dependencies to workspace.dependencies to avoid duplication
   * Remove a bunch of unused dependencies
 - <csr-id-a40f5127796316069aa2ac646ea5c3817f7a29fa/> Add crate metadata and prepare for first CLI release
 - <csr-id-065513b5f6ee350b9f2607d5aa7003df352f5cbc/> "app list" filters
   Extend the "app list" command to query either a namespace, a users apps,
   or all apps accessible by a user.
   (--namepsace X, --all)
 - <csr-id-38336ebe12a85b142871a4c0fa50541df815a441/> Add a webc app config fetch tests
 - <csr-id-8febcde88d1d7df8a7a86c79afcf960c2cb868a3/> Make serde_json a workspace dependency
   To avoid duplication...
 - <csr-id-33f822d9701e87c0f0564574a76adef83ccf72a5/> Lift serde to be a workspace dependency
   Easier version management...
 - <csr-id-d968694ade1191afff80e6d141598b9c10b2f3c7/> Lift anyhow, time and clap to workspace dependnecies
   Less version management...

### Bug Fixes

 - <csr-id-07a199f0bcccd3178e8f773ae96d100febfb88d0/> Use token for webc fetching
   If the api is configured with a token, use the token for fetching webcs.
   Previously it just used anonymous access.
 - <csr-id-c6ad494a45968bb4a69455f3c292b6fcf9631770/> Update deployment config generation to backend changes
   The generateDeployConfig GraphQL API has changed
   
   * Takes a DeployConfigVersion id instead of DeployConfig id
* Returns a DeployConfigVersion

### New Features

 - <csr-id-f96944cd8097aff11d51e8e0c3f6fa1efc6b1ec6/> Add generate_deploy_token to new cynic GQL api client
   Will be needed for various commands
 - <csr-id-33e469b921256a241866a5a972278665905dcdf4/> Add getPackage GQL query
 - <csr-id-4783d6a5c53875724bbea3ed5a65b13e5056c001/> Add query for DeployAppVersion
 - <csr-id-b4aa770fb388970c837f2e5429caa5803eef64bf/> Add new namespace and app commands
 - <csr-id-63ec5f98dca19c417df0e42a5bfbbd963c9b19c2/> Add a CapabilityLoggingV1 config
   Allows to configure the logging behaviour of workloads.
   
   Will be used very soon to implement instance log forwarding.

### Documentation

 - <csr-id-98b9313b34e93c7973b778ef0867f042bf7aed57/> add some changelogs
 - <csr-id-845a8b3ebece96d5fff941110a425ab19a3a2eed/> Add REAMDE to Cargo.toml of to-be-published crates

### Chore

 - <csr-id-b1245bfa194e05d49809973b716e2565689bfccf/> Remove download_url from WebcPackageIdentifierV1
   Not needed anymore, since now we have a deployment config registry.

<csr-unknown>
Needed to update relevant callsites<csr-unknown/>
<csr-unknown/>

