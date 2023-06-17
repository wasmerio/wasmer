# wasmer-registry

This crate provides integration with the Wasmer package registry GraphQL API.

## Development

### Updating the GraphQL Schema

The GraphQL API schema used for generating queries and mutations is located at
`./graphql/schema.graphql`.

To update it to a deployed version of the backend, run:

```bash
npx get-graphql-schema https://registry.wasmer.wtf/graphql > graphql/schema.graphql
```

### Formatting GraphQL Files

To format the GraphQL query and mutation files, run:

```bash
npx prettier --write ./graphql/**/*.graphql
```
