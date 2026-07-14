# BaeSQL Architecture

BaeSQL 0.1 is split into three Rust crates.

## `baesql-sql`

This crate owns SQL text handling:

- lexer
- parser
- AST definitions

The lexer treats SQL keywords case-insensitively. Identifiers are normalized to lowercase. String literals use single quotes and support SQL-style `''` escaping.

## `baesql-core`

This crate owns database behavior:

- table schemas
- row values
- constraint validation
- expression evaluation
- CRUD execution
- transaction state
- `.bae` binary storage

The engine keeps a complete in-memory `Database` while a process is running. Mutating statements outside a transaction are persisted immediately. `BEGIN` clones the current database into a transaction workspace; `COMMIT` replaces the durable state and writes it; `ROLLBACK` discards it.

Statement-level atomicity for `UPDATE` and `DELETE` is implemented by validating changes against cloned table state before replacing the live table.

## `baesql-cli`

This crate provides:

- interactive REPL
- `--execute` SQL execution
- `--file` SQL script execution
- result table printing
- meta commands

The CLI does not start a network server.

## Storage Model

BaeSQL 0.1 stores the entire database in one `.bae` file using a custom binary format. Writes go to a temporary file in the same directory, are synced, and are then installed with atomic rename.

This is simple and appropriate for the 0.1 scope, but it is not a high-concurrency or large-database architecture.
