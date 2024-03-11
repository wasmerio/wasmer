CREATE TABLE personal_keys
(
  id integer primary key,
  active integer not null,
  public_key_id text not null UNIQUE,
  public_key_value text not null UNIQUE,
  private_key_location text UNIQUE,
  key_type_identifier text not null,
  date_added text not null
);

CREATE TABLE wapm_users
(
  id integer primary key,
  name text not null UNIQUE
);

CREATE TABLE wapm_public_keys
(
  id integer primary key,
  public_key_id text not null UNIQUE,
  user_key integer not null,
  public_key_value text not null UNIQUE,
  key_type_identifier text not null,
  date_added text not null,
  FOREIGN KEY(user_key) REFERENCES wapm_users(id)
);
