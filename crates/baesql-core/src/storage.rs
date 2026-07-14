use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use baesql_sql::DataType;

use crate::{Column, Database, DbError, Result, Row, Table, Value};

pub const MAGIC: &[u8; 8] = b"BAESQL01";
pub const FORMAT_VERSION: u32 = 1;

pub fn read_database(path: &Path) -> Result<Database> {
    let mut bytes = Vec::new();
    File::open(path)?.read_to_end(&mut bytes)?;
    let mut reader = Reader::new(&bytes);
    reader.expect_bytes(MAGIC)?;
    let version = reader.read_u32()?;
    if version != FORMAT_VERSION {
        return Err(storage_error(format!(
            "unsupported .bae version {version}; expected {FORMAT_VERSION}"
        )));
    }
    let table_count = reader.read_u32()?;
    let mut tables = std::collections::BTreeMap::new();
    for _ in 0..table_count {
        let name = reader.read_string()?;
        let column_count = reader.read_u32()? as usize;
        let row_count = reader.read_u64()? as usize;
        let mut columns = Vec::with_capacity(column_count);
        for _ in 0..column_count {
            let column_name = reader.read_string()?;
            let data_type = match reader.read_u8()? {
                1 => DataType::Integer,
                2 => DataType::Text,
                3 => DataType::Boolean,
                other => return Err(storage_error(format!("unknown data type tag {other}"))),
            };
            let flags = reader.read_u8()?;
            columns.push(Column {
                name: column_name,
                data_type,
                primary_key: flags & 0b0000_0001 != 0,
                not_null: flags & 0b0000_0010 != 0,
            });
        }
        let mut rows = Vec::with_capacity(row_count);
        for _ in 0..row_count {
            let mut values = Vec::with_capacity(column_count);
            for _ in 0..column_count {
                values.push(reader.read_value()?);
            }
            rows.push(Row { values });
        }
        if tables
            .insert(
                name.clone(),
                Table {
                    name,
                    columns,
                    rows,
                },
            )
            .is_some()
        {
            return Err(storage_error("duplicate table in file"));
        }
    }
    if !reader.is_eof() {
        return Err(storage_error("trailing bytes after database payload"));
    }
    Ok(Database { tables })
}

pub fn write_database(path: &Path, database: &Database) -> Result<()> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(MAGIC);
    write_u32(&mut bytes, FORMAT_VERSION);
    write_u32(
        &mut bytes,
        usize_to_u32(database.tables.len(), "table count")?,
    );
    for table in database.tables.values() {
        write_string(&mut bytes, &table.name)?;
        write_u32(
            &mut bytes,
            usize_to_u32(table.columns.len(), "column count")?,
        );
        write_u64(&mut bytes, usize_to_u64(table.rows.len())?);
        for column in &table.columns {
            write_string(&mut bytes, &column.name)?;
            bytes.push(match column.data_type {
                DataType::Integer => 1,
                DataType::Text => 2,
                DataType::Boolean => 3,
            });
            let mut flags = 0u8;
            if column.primary_key {
                flags |= 0b0000_0001;
            }
            if column.not_null {
                flags |= 0b0000_0010;
            }
            bytes.push(flags);
        }
        for row in &table.rows {
            for value in &row.values {
                write_value(&mut bytes, value)?;
            }
        }
    }
    let tmp = temp_path(path);
    {
        let mut file = File::create(&tmp)?;
        file.write_all(&bytes)?;
        file.sync_data()?;
    }
    fs::rename(&tmp, path)?;
    if let Some(parent) = path.parent()
        && let Ok(dir) = File::open(parent)
    {
        let _ = dir.sync_data();
    }
    Ok(())
}

fn temp_path(path: &Path) -> PathBuf {
    let pid = std::process::id();
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("database.bae");
    path.with_file_name(format!(".{file_name}.{pid}.tmp"))
}

fn write_value(bytes: &mut Vec<u8>, value: &Value) -> Result<()> {
    match value {
        Value::Null => bytes.push(0),
        Value::Integer(value) => {
            bytes.push(1);
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        Value::Text(value) => {
            bytes.push(2);
            write_string(bytes, value)?;
        }
        Value::Boolean(value) => {
            bytes.push(3);
            bytes.push(u8::from(*value));
        }
    }
    Ok(())
}

fn write_string(bytes: &mut Vec<u8>, value: &str) -> Result<()> {
    write_u32(bytes, usize_to_u32(value.len(), "string length")?);
    bytes.extend_from_slice(value.as_bytes());
    Ok(())
}

fn write_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn write_u64(bytes: &mut Vec<u8>, value: u64) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn usize_to_u32(value: usize, label: &str) -> Result<u32> {
    u32::try_from(value).map_err(|_| storage_error(format!("{label} is too large")))
}

fn usize_to_u64(value: usize) -> Result<u64> {
    u64::try_from(value).map_err(|_| storage_error("row count is too large"))
}

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    fn expect_bytes(&mut self, expected: &[u8]) -> Result<()> {
        let actual = self.read_exact(expected.len())?;
        if actual != expected {
            return Err(storage_error("invalid .bae magic bytes"));
        }
        Ok(())
    }

    fn read_value(&mut self) -> Result<Value> {
        match self.read_u8()? {
            0 => Ok(Value::Null),
            1 => Ok(Value::Integer(self.read_i64()?)),
            2 => Ok(Value::Text(self.read_string()?)),
            3 => match self.read_u8()? {
                0 => Ok(Value::Boolean(false)),
                1 => Ok(Value::Boolean(true)),
                other => Err(storage_error(format!("invalid BOOLEAN payload {other}"))),
            },
            other => Err(storage_error(format!("unknown value tag {other}"))),
        }
    }

    fn read_string(&mut self) -> Result<String> {
        let len = self.read_u32()? as usize;
        let bytes = self.read_exact(len)?;
        String::from_utf8(bytes.to_vec()).map_err(|_| storage_error("invalid UTF-8 string"))
    }

    fn read_u8(&mut self) -> Result<u8> {
        Ok(self.read_exact(1)?[0])
    }

    fn read_u32(&mut self) -> Result<u32> {
        let bytes = self.read_exact(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn read_u64(&mut self) -> Result<u64> {
        let bytes = self.read_exact(8)?;
        Ok(u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    fn read_i64(&mut self) -> Result<i64> {
        let bytes = self.read_exact(8)?;
        Ok(i64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    fn read_exact(&mut self, len: usize) -> Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(len)
            .ok_or_else(|| storage_error("file offset overflow"))?;
        if end > self.bytes.len() {
            return Err(storage_error("truncated .bae file"));
        }
        let slice = &self.bytes[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    fn is_eof(&self) -> bool {
        self.pos == self.bytes.len()
    }
}

fn storage_error(message: impl Into<String>) -> DbError {
    DbError::Storage(message.into())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::Engine;

    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        std::env::temp_dir().join(format!("baesql-storage-{name}-{unique}.bae"))
    }

    #[test]
    fn detects_corrupt_file() {
        let path = temp_path("corrupt");
        fs::write(&path, b"not a bae database").expect("write");
        assert!(Engine::open(&path).is_err());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn detects_truncated_file() {
        let path = temp_path("truncated");
        fs::write(&path, &MAGIC[..4]).expect("write");
        assert!(Engine::open(&path).is_err());
        let _ = fs::remove_file(path);
    }
}
