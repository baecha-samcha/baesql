use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use baesql_core::{Column, Engine, QueryResult, Value};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), String> {
    let args = Args::parse(env::args().skip(1).collect())?;
    if args.uses_default_database {
        ensure_database_parent(&args.database)?;
    }
    let mut engine = Engine::open(&args.database).map_err(|error| error.to_string())?;
    match args.mode {
        Mode::Repl => repl(&mut engine),
        Mode::Execute(sql) => execute_and_print(&mut engine, &sql),
        Mode::File(path) => {
            let sql = fs::read_to_string(&path)
                .map_err(|error| format!("failed to read '{path}': {error}"))?;
            execute_and_print(&mut engine, &sql)
        }
    }
}

struct Args {
    database: PathBuf,
    mode: Mode,
    uses_default_database: bool,
}

enum Mode {
    Repl,
    Execute(String),
    File(String),
}

impl Args {
    fn parse(args: Vec<String>) -> Result<Self, String> {
        let mut database = None;
        let mut mode = Mode::Repl;
        let mut index = 0usize;
        while index < args.len() {
            match args[index].as_str() {
                "--execute" => {
                    index += 1;
                    let sql = args.get(index).ok_or_else(usage)?;
                    mode = Mode::Execute(sql.clone());
                }
                "--file" => {
                    index += 1;
                    let path = args.get(index).ok_or_else(usage)?;
                    mode = Mode::File(path.clone());
                }
                "--help" | "-h" => return Err(usage()),
                other if database.is_none() => database = Some(PathBuf::from(other)),
                other => return Err(format!("unknown argument '{other}'\n{}", usage())),
            }
            index += 1;
        }
        let (database, uses_default_database) = match database {
            Some(path) => (path, false),
            None => (default_database_path(), true),
        };
        Ok(Self {
            database,
            mode,
            uses_default_database,
        })
    }
}

fn usage() -> String {
    "usage: baesql [database.bae] [--execute SQL | --file script.sql]".to_string()
}

fn default_database_path() -> PathBuf {
    if let Some(data_dir) = env::var_os("BAESQL_DATA_DIR") {
        return PathBuf::from(data_dir).join(DEFAULT_DATABASE);
    }
    if let Some(config) = read_config(Path::new("/etc/baesql/config.toml")) {
        let database = config
            .default_database
            .filter(|name| is_safe_database_file_name(name))
            .unwrap_or_else(|| DEFAULT_DATABASE.to_string());
        return config.data_dir.join(database);
    }
    user_default_data_dir().join(DEFAULT_DATABASE)
}

const DEFAULT_DATABASE: &str = "main.bae";

struct Config {
    data_dir: PathBuf,
    default_database: Option<String>,
}

fn read_config(path: &Path) -> Option<Config> {
    let contents = fs::read_to_string(path).ok()?;
    let mut data_dir = None;
    let mut default_database = None;
    for line in contents.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let Some(value) = parse_toml_string(value.trim()) else {
            continue;
        };
        match key.trim() {
            "data_dir" => data_dir = Some(PathBuf::from(value)),
            "default_database" => default_database = Some(value),
            _ => {}
        }
    }
    data_dir.map(|data_dir| Config {
        data_dir,
        default_database,
    })
}

fn parse_toml_string(value: &str) -> Option<String> {
    let value = value.strip_prefix('"')?.strip_suffix('"')?;
    let mut output = String::new();
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next()? {
                '"' => output.push('"'),
                '\\' => output.push('\\'),
                'n' => output.push('\n'),
                't' => output.push('\t'),
                _ => return None,
            }
        } else {
            output.push(ch);
        }
    }
    Some(output)
}

fn is_safe_database_file_name(name: &str) -> bool {
    let path = Path::new(name);
    !name.is_empty()
        && name.ends_with(".bae")
        && !path.is_absolute()
        && path.components().count() == 1
}

fn user_default_data_dir() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".local")
        .join("share")
        .join("baesql")
}

fn ensure_database_parent(database: &Path) -> Result<(), String> {
    let parent = database
        .parent()
        .ok_or_else(|| format!("database path '{}' has no parent", database.display()))?;
    fs::create_dir_all(parent).map_err(|error| {
        format!(
            "failed to create data directory '{}': {error}",
            parent.display()
        )
    })
}

fn repl(engine: &mut Engine) -> Result<(), String> {
    let stdin = io::stdin();
    let mut buffer = String::new();
    loop {
        if buffer.is_empty() {
            print!("baesql> ");
        } else {
            print!("   ...> ");
        }
        io::stdout()
            .flush()
            .map_err(|error| format!("failed to flush stdout: {error}"))?;
        let mut line = String::new();
        let read = stdin
            .read_line(&mut line)
            .map_err(|error| format!("failed to read stdin: {error}"))?;
        if read == 0 {
            return Ok(());
        }
        let trimmed = line.trim();
        if buffer.is_empty() && trimmed.starts_with('.') {
            if handle_meta(engine, trimmed)? {
                return Ok(());
            }
            continue;
        }
        buffer.push_str(&line);
        if trimmed.ends_with(';') {
            execute_and_print(engine, &buffer)?;
            buffer.clear();
        }
    }
}

fn handle_meta(engine: &Engine, command: &str) -> Result<bool, String> {
    let mut parts = command.split_whitespace();
    match parts.next() {
        Some(".tables") => {
            for table in engine.table_names() {
                println!("{table}");
            }
        }
        Some(".schema") => {
            let table = parts
                .next()
                .ok_or_else(|| ".schema requires a table name".to_string())?;
            print_schema(
                table,
                &engine.schema(table).map_err(|error| error.to_string())?,
            );
        }
        Some(".status") => {
            println!("database: {}", engine.path().display());
            println!("tables: {}", engine.table_names().len());
            println!(
                "transaction: {}",
                if engine.in_transaction() {
                    "active"
                } else {
                    "none"
                }
            );
        }
        Some(".help") => print_help(),
        Some(".exit") => return Ok(true),
        Some(other) => return Err(format!("unknown meta command '{other}'")),
        None => {}
    }
    Ok(false)
}

fn execute_and_print(engine: &mut Engine, sql: &str) -> Result<(), String> {
    let results = engine
        .execute_script(sql)
        .map_err(|error| error.to_string())?;
    for result in results {
        print_result(&result);
    }
    Ok(())
}

fn print_result(result: &QueryResult) {
    match result {
        QueryResult::Message(message) => println!("{message}"),
        QueryResult::Rows { columns, rows } => print_rows(columns, rows),
    }
}

fn print_rows(columns: &[String], rows: &[Vec<Value>]) {
    if columns.is_empty() {
        println!("(0 columns)");
        return;
    }
    let mut widths: Vec<usize> = columns.iter().map(String::len).collect();
    for row in rows {
        for (index, value) in row.iter().enumerate() {
            widths[index] = widths[index].max(value.display_sql().len());
        }
    }
    print_line(columns, &widths);
    let separator: Vec<String> = widths.iter().map(|width| "-".repeat(*width)).collect();
    print_line(&separator, &widths);
    for row in rows {
        let cells: Vec<String> = row.iter().map(Value::display_sql).collect();
        print_line(&cells, &widths);
    }
    println!("({} rows)", rows.len());
}

fn print_line(cells: &[String], widths: &[usize]) {
    for (index, cell) in cells.iter().enumerate() {
        if index > 0 {
            print!(" | ");
        }
        print!("{cell:<width$}", width = widths[index]);
    }
    println!();
}

fn print_schema(table: &str, columns: &[Column]) {
    println!("CREATE TABLE {table} (");
    for (index, column) in columns.iter().enumerate() {
        let mut line = format!("  {} {}", column.name, data_type_name(column));
        if column.primary_key {
            line.push_str(" PRIMARY KEY");
        } else if column.not_null {
            line.push_str(" NOT NULL");
        }
        if index + 1 != columns.len() {
            line.push(',');
        }
        println!("{line}");
    }
    println!(");");
}

fn data_type_name(column: &Column) -> &'static str {
    match column.data_type {
        baesql_sql::DataType::Integer => "INTEGER",
        baesql_sql::DataType::Text => "TEXT",
        baesql_sql::DataType::Boolean => "BOOLEAN",
    }
}

fn print_help() {
    println!(".tables          list tables");
    println!(".schema <table>  show CREATE TABLE shape");
    println!(".status          show database path and transaction state");
    println!(".help            show this help");
    println!(".exit            exit");
}
