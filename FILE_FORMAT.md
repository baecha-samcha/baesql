# BaeSQL `.bae` File Format

BaeSQL 0.1 uses a custom little-endian binary file format. It does not store the database as JSON, TOML, YAML, or an automatic serialization format.

## Header

| Field | Size | Description |
| --- | ---: | --- |
| magic | 8 bytes | ASCII `BAESQL01` |
| version | `u32` | format version, currently `1` |
| table_count | `u32` | number of tables |

Files with wrong magic bytes, unsupported versions, truncated payloads, invalid tags, invalid UTF-8, duplicate table entries, or trailing bytes are rejected.

## Table

For each table:

| Field | Size | Description |
| --- | ---: | --- |
| name | string | table name |
| column_count | `u32` | number of columns |
| row_count | `u64` | number of rows |
| columns | variable | column descriptors |
| rows | variable | row values |

## String

| Field | Size | Description |
| --- | ---: | --- |
| byte_len | `u32` | UTF-8 byte length |
| bytes | variable | UTF-8 payload |

## Column

| Field | Size | Description |
| --- | ---: | --- |
| name | string | column name |
| type_tag | `u8` | `1` integer, `2` text, `3` boolean |
| flags | `u8` | bit 0 primary key, bit 1 not null |

## Value

| Tag | Payload | Description |
| ---: | --- | --- |
| `0` | none | `NULL` |
| `1` | `i64` | `INTEGER` |
| `2` | string | `TEXT` |
| `3` | `u8` | `BOOLEAN`, `0` false and `1` true |

## Write Safety

On write, BaeSQL serializes the full database to a temporary file in the same directory, calls `sync_data`, renames the temporary file over the target `.bae` file, and attempts to sync the parent directory.
