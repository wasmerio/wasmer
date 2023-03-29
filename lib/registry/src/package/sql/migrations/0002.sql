PRAGMA foreign_keys=off;

CREATE TABLE wasm_interfaces
(
  id integer primary key,
  interface_name text not null,
  version text not null,
  date_added text not null,
  content text not null,
  CONSTRAINT name_version_unique UNIQUE (interface_name, version)
);

INSERT INTO wasm_interfaces
(
  id,
  interface_name,
  version,
  date_added,
  content
)
  SELECT id, contract_name, version, date_added, content
  FROM wasm_contracts;

DROP TABLE wasm_contracts;

PRAGMA foreign_keys=on;
