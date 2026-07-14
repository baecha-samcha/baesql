use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_path(name: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    std::env::temp_dir().join(format!("baesql-cli-{name}-{unique}.bae"))
}

#[test]
fn cli_execute_runs_sql() {
    let path = temp_path("execute");
    let output = Command::new(env!("CARGO_BIN_EXE_baesql"))
        .arg(&path)
        .arg("--execute")
        .arg(
            "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);
             INSERT INTO users VALUES (1, 'cli');
             SELECT name FROM users;",
        )
        .output()
        .expect("run cli");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("cli"));
    let _ = fs::remove_file(path);
}

#[test]
fn cli_uses_default_database_from_env() {
    let dir = temp_path("default-dir");
    fs::create_dir_all(&dir).expect("create temp dir");
    let output = Command::new(env!("CARGO_BIN_EXE_baesql"))
        .env("BAESQL_DATA_DIR", &dir)
        .arg("--execute")
        .arg("CREATE TABLE t (id INTEGER PRIMARY KEY); INSERT INTO t VALUES (1);")
        .output()
        .expect("run cli");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(dir.join("main.bae").exists());
    let _ = fs::remove_file(dir.join("main.bae"));
    let _ = fs::remove_dir(dir);
}
