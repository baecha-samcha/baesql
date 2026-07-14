# BaeSQL

BaeSQL is an experimental independent relational database management system written in Rust.

Warning: BaeSQL 0.1 is experimental software. Do not use it for important, irreplaceable, or production data.

It is not a wrapper around MariaDB, PostgreSQL, SQLite, or any other database engine. The `.bae` file format and execution engine are implemented directly in this repository.

## Build

```bash
cargo build --workspace
```

## Run

Interactive REPL:

```bash
cargo run -p baesql-cli -- database.bae
```

Run one SQL string:

```bash
cargo run -p baesql-cli -- database.bae --execute "SELECT * FROM users;"
```

Run a SQL file:

```bash
cargo run -p baesql-cli -- database.bae --file script.sql
```

## Supported SQL

BaeSQL 0.1 supports:

- `CREATE TABLE`
- `DROP TABLE`
- `INSERT INTO`
- `SELECT`
- `UPDATE`
- `DELETE FROM`
- `BEGIN`
- `COMMIT`
- `ROLLBACK`

Supported SQL elements:

- `INTEGER`, `TEXT`, `BOOLEAN`, `NULL`
- `PRIMARY KEY`, `NOT NULL`
- `WHERE`
- `AND`, `OR`, `NOT`
- `=`, `!=`, `<>`, `<`, `<=`, `>`, `>=`
- `IS NULL`, `IS NOT NULL`
- `SELECT *`
- explicit column selection
- case-insensitive SQL keywords
- single-quoted strings with `''` escaping
- basic SQL three-valued logic for `NULL`

## CLI Meta Commands

- `.tables`
- `.schema <table>`
- `.status`
- `.help`
- `.exit`

## Example

```sql
CREATE TABLE users (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL,
  active BOOLEAN
);

INSERT INTO users VALUES (1, 'Bae', TRUE);
SELECT * FROM users WHERE active = TRUE;
```

## Not Supported

BaeSQL 0.1 does not support joins, indexes, foreign keys, views, triggers, network access, authentication, concurrent writers, query optimization, `ALTER TABLE`, `ORDER BY`, `GROUP BY`, `LIMIT`, arithmetic expressions, or SQL functions.
