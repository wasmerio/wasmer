# wasmer-backend-api

GraphQL API client for the [Wasmer](https://wasmer.io) backend.

## Development

This client is built on the [cynic][cynic-api-docs] crate,
a GraphQL client library that allows tight integration between Rust and
GraphQL types.

It was chosen over other implementations like `graphql-client` because it
significantly reduces boilerplate and improves the development experience.

The downside is that the underlying GraphQL queries are much less obvious when
looking at the code. This can be remedied with some strategies mentioned below.

Consult the Cynic docs at [cynic-rs.dev][cynic-website] for more
information.

### Backend GraphQL Schema

The GraphQL schema for the backend is stored in `./schema.graphql`.

To update the schema, simply download the latest version and replace the local
file.

It can be retrieved from
https://github.com/wasmerio/backend/blob/main/backend/graphql/schema.graphql.

### Writing/Updating Queries

You can use the [Cynic web UI][cynic-web-ui] to easily create the types for new
queries.

Simply upload the local schema from `./schema.graphql` and use the UI to build
your desired query.

NOTE: Where possible, do not duplicate types that are already defined,
and instead reuse/extend them where possible.

This is not always sensible though, depending on which nested data you want to
fetch.

[cynic-api-docs]: https://docs.rs/cynic/latest/cynic/
[cynic-web-ui]: https://generator.cynic-rs.dev/
[cynic-website]: https://cynic-rs.dev
