CREATE TABLE wasm_contracts
(
  id integer primary key,
  contract_name text not null,
  version text not null,
  date_added text not null,
  content text not null,
  CONSTRAINT name_version_unique UNIQUE (contract_name, version)
);
