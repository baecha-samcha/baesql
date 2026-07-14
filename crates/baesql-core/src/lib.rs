mod storage;

use std::collections::{BTreeMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};

use baesql_sql::{BinaryOp, DataType, Expr, Literal, Projection, Statement, UnaryOp, parse_script};

pub use storage::{FORMAT_VERSION, MAGIC};

#[derive(Debug)]
pub enum DbError {
    Sql(String),
    Storage(String),
    Execution(String),
}

impl fmt::Display for DbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sql(message) => write!(f, "SQL error: {message}"),
            Self::Storage(message) => write!(f, "storage error: {message}"),
            Self::Execution(message) => write!(f, "execution error: {message}"),
        }
    }
}

impl std::error::Error for DbError {}

impl From<baesql_sql::ParseError> for DbError {
    fn from(value: baesql_sql::ParseError) -> Self {
        Self::Sql(value.to_string())
    }
}

impl From<std::io::Error> for DbError {
    fn from(value: std::io::Error) -> Self {
        Self::Storage(value.to_string())
    }
}

pub type Result<T> = std::result::Result<T, DbError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Null,
    Integer(i64),
    Text(String),
    Boolean(bool),
}

impl Value {
    #[must_use]
    pub fn display_sql(&self) -> String {
        match self {
            Self::Null => "NULL".to_string(),
            Self::Integer(value) => value.to_string(),
            Self::Text(value) => value.clone(),
            Self::Boolean(value) => value.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    pub name: String,
    pub data_type: DataType,
    pub primary_key: bool,
    pub not_null: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub values: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    pub name: String,
    pub columns: Vec<Column>,
    pub rows: Vec<Row>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Database {
    pub tables: BTreeMap<String, Table>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryResult {
    Message(String),
    Rows {
        columns: Vec<String>,
        rows: Vec<Vec<Value>>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Truth {
    True,
    False,
    Unknown,
}

pub struct Engine {
    path: PathBuf,
    db: Database,
    transaction: Option<Database>,
}

impl Engine {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let db = if path.exists() {
            storage::read_database(&path)?
        } else {
            Database::default()
        };
        Ok(Self {
            path,
            db,
            transaction: None,
        })
    }

    pub fn execute_script(&mut self, input: &str) -> Result<Vec<QueryResult>> {
        let statements = parse_script(input)?;
        statements
            .into_iter()
            .map(|statement| self.execute(statement))
            .collect()
    }

    pub fn execute(&mut self, statement: Statement) -> Result<QueryResult> {
        match statement {
            Statement::Begin => self.begin(),
            Statement::Commit => self.commit(),
            Statement::Rollback => self.rollback(),
            Statement::Select {
                table,
                projection,
                where_clause,
            } => self.select(&table, &projection, where_clause.as_ref()),
            statement => {
                let result = self.execute_mutating(statement)?;
                if self.transaction.is_none() {
                    self.persist()?;
                }
                Ok(result)
            }
        }
    }

    #[must_use]
    pub fn table_names(&self) -> Vec<String> {
        self.active_db().tables.keys().cloned().collect()
    }

    pub fn schema(&self, table_name: &str) -> Result<Vec<Column>> {
        let table = self
            .active_db()
            .tables
            .get(table_name)
            .ok_or_else(|| execution_error(format!("table '{table_name}' does not exist")))?;
        Ok(table.columns.clone())
    }

    #[must_use]
    pub fn in_transaction(&self) -> bool {
        self.transaction.is_some()
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn begin(&mut self) -> Result<QueryResult> {
        if self.transaction.is_some() {
            return Err(execution_error("nested BEGIN is not supported"));
        }
        self.transaction = Some(self.db.clone());
        Ok(QueryResult::Message("BEGIN".to_string()))
    }

    fn commit(&mut self) -> Result<QueryResult> {
        let Some(transaction) = self.transaction.take() else {
            return Err(execution_error("no active transaction"));
        };
        self.db = transaction;
        self.persist()?;
        Ok(QueryResult::Message("COMMIT".to_string()))
    }

    fn rollback(&mut self) -> Result<QueryResult> {
        if self.transaction.take().is_none() {
            return Err(execution_error("no active transaction"));
        }
        Ok(QueryResult::Message("ROLLBACK".to_string()))
    }

    fn execute_mutating(&mut self, statement: Statement) -> Result<QueryResult> {
        match statement {
            Statement::CreateTable { name, columns } => self.create_table(name, columns),
            Statement::DropTable { name } => self.drop_table(&name),
            Statement::Insert {
                table,
                columns,
                values,
            } => self.insert(&table, columns.as_deref(), &values),
            Statement::Update {
                table,
                assignments,
                where_clause,
            } => self.update(&table, &assignments, where_clause.as_ref()),
            Statement::Delete {
                table,
                where_clause,
            } => self.delete(&table, where_clause.as_ref()),
            _ => Err(execution_error("statement is not mutating")),
        }
    }

    fn create_table(
        &mut self,
        name: String,
        columns: Vec<baesql_sql::ColumnDef>,
    ) -> Result<QueryResult> {
        let db = self.active_db_mut();
        if db.tables.contains_key(&name) {
            return Err(execution_error(format!("table '{name}' already exists")));
        }
        if columns.is_empty() {
            return Err(execution_error("CREATE TABLE requires at least one column"));
        }
        let mut seen = HashSet::new();
        let mut primary_keys = 0usize;
        let mut table_columns = Vec::new();
        for column in columns {
            if !seen.insert(column.name.clone()) {
                return Err(execution_error(format!(
                    "column '{}' is duplicated",
                    column.name
                )));
            }
            if column.primary_key {
                primary_keys += 1;
            }
            table_columns.push(Column {
                name: column.name,
                data_type: column.data_type,
                primary_key: column.primary_key,
                not_null: column.not_null || column.primary_key,
            });
        }
        if primary_keys > 1 {
            return Err(execution_error(
                "only one PRIMARY KEY column is supported per table",
            ));
        }
        db.tables.insert(
            name.clone(),
            Table {
                name,
                columns: table_columns,
                rows: Vec::new(),
            },
        );
        Ok(QueryResult::Message("CREATE TABLE".to_string()))
    }

    fn drop_table(&mut self, name: &str) -> Result<QueryResult> {
        let db = self.active_db_mut();
        if db.tables.remove(name).is_none() {
            return Err(execution_error(format!("table '{name}' does not exist")));
        }
        Ok(QueryResult::Message("DROP TABLE".to_string()))
    }

    fn insert(
        &mut self,
        table_name: &str,
        columns: Option<&[String]>,
        expressions: &[Expr],
    ) -> Result<QueryResult> {
        let db = self.active_db_mut();
        let table = db
            .tables
            .get(table_name)
            .ok_or_else(|| execution_error(format!("table '{table_name}' does not exist")))?;
        let mut values = vec![Value::Null; table.columns.len()];
        if let Some(column_names) = columns {
            if column_names.len() != expressions.len() {
                return Err(execution_error("column count does not match value count"));
            }
            let mut seen = HashSet::new();
            for (column_name, expression) in column_names.iter().zip(expressions) {
                if !seen.insert(column_name) {
                    return Err(execution_error(format!(
                        "column '{column_name}' is duplicated"
                    )));
                }
                let index = column_index(table, column_name)?;
                values[index] = eval_insert_expr(expression)?;
            }
        } else {
            if expressions.len() != table.columns.len() {
                return Err(execution_error(
                    "value count does not match table column count",
                ));
            }
            for (index, expression) in expressions.iter().enumerate() {
                values[index] = eval_insert_expr(expression)?;
            }
        }
        validate_row(table, &values, None)?;
        let table = db
            .tables
            .get_mut(table_name)
            .ok_or_else(|| execution_error(format!("table '{table_name}' does not exist")))?;
        table.rows.push(Row { values });
        Ok(QueryResult::Message("INSERT 1".to_string()))
    }

    fn select(
        &self,
        table_name: &str,
        projection: &Projection,
        where_clause: Option<&Expr>,
    ) -> Result<QueryResult> {
        let db = self.active_db();
        let table = db
            .tables
            .get(table_name)
            .ok_or_else(|| execution_error(format!("table '{table_name}' does not exist")))?;
        let indexes = projection_indexes(table, projection)?;
        let columns = indexes
            .iter()
            .map(|index| table.columns[*index].name.clone())
            .collect();
        let mut rows = Vec::new();
        for row in &table.rows {
            if row_matches(table, row, where_clause)? {
                rows.push(
                    indexes
                        .iter()
                        .map(|index| row.values[*index].clone())
                        .collect(),
                );
            }
        }
        Ok(QueryResult::Rows { columns, rows })
    }

    fn update(
        &mut self,
        table_name: &str,
        assignments: &[(String, Expr)],
        where_clause: Option<&Expr>,
    ) -> Result<QueryResult> {
        let db = self.active_db_mut();
        let table = db
            .tables
            .get(table_name)
            .ok_or_else(|| execution_error(format!("table '{table_name}' does not exist")))?;
        let mut indexes = Vec::new();
        let mut seen = HashSet::new();
        for (column, _) in assignments {
            if !seen.insert(column) {
                return Err(execution_error(format!("column '{column}' is duplicated")));
            }
            indexes.push(column_index(table, column)?);
        }
        let mut new_table = table.clone();
        let mut updated = 0usize;
        for row_index in 0..new_table.rows.len() {
            let current_row = new_table.rows[row_index].clone();
            if !row_matches(table, &current_row, where_clause)? {
                continue;
            }
            let mut new_values = current_row.values;
            for ((_, expression), column_index) in assignments.iter().zip(&indexes) {
                new_values[*column_index] = eval_expr(
                    table,
                    &Row {
                        values: new_values.clone(),
                    },
                    expression,
                )?;
            }
            validate_row(&new_table, &new_values, Some(row_index))?;
            new_table.rows[row_index] = Row { values: new_values };
            updated += 1;
        }
        let table = db
            .tables
            .get_mut(table_name)
            .ok_or_else(|| execution_error(format!("table '{table_name}' does not exist")))?;
        *table = new_table;
        Ok(QueryResult::Message(format!("UPDATE {updated}")))
    }

    fn delete(&mut self, table_name: &str, where_clause: Option<&Expr>) -> Result<QueryResult> {
        let db = self.active_db_mut();
        let table = db
            .tables
            .get(table_name)
            .ok_or_else(|| execution_error(format!("table '{table_name}' does not exist")))?;
        let mut new_rows = Vec::new();
        let mut deleted = 0usize;
        for row in &table.rows {
            if row_matches(table, row, where_clause)? {
                deleted += 1;
            } else {
                new_rows.push(row.clone());
            }
        }
        let table = db
            .tables
            .get_mut(table_name)
            .ok_or_else(|| execution_error(format!("table '{table_name}' does not exist")))?;
        table.rows = new_rows;
        Ok(QueryResult::Message(format!("DELETE {deleted}")))
    }

    fn active_db(&self) -> &Database {
        self.transaction.as_ref().unwrap_or(&self.db)
    }

    fn active_db_mut(&mut self) -> &mut Database {
        self.transaction.as_mut().unwrap_or(&mut self.db)
    }

    fn persist(&self) -> Result<()> {
        storage::write_database(&self.path, &self.db)
    }
}

fn projection_indexes(table: &Table, projection: &Projection) -> Result<Vec<usize>> {
    match projection {
        Projection::All => Ok((0..table.columns.len()).collect()),
        Projection::Columns(columns) => columns
            .iter()
            .map(|column| column_index(table, column))
            .collect(),
    }
}

fn column_index(table: &Table, name: &str) -> Result<usize> {
    table
        .columns
        .iter()
        .position(|column| column.name == name)
        .ok_or_else(|| execution_error(format!("column '{name}' does not exist")))
}

fn eval_insert_expr(expr: &Expr) -> Result<Value> {
    match expr {
        Expr::Literal(literal) => Ok(value_from_literal(literal)),
        _ => Err(execution_error(
            "INSERT values must be literal INTEGER, TEXT, BOOLEAN, or NULL",
        )),
    }
}

fn eval_expr(table: &Table, row: &Row, expr: &Expr) -> Result<Value> {
    match expr {
        Expr::Literal(literal) => Ok(value_from_literal(literal)),
        Expr::Identifier(name) => Ok(row.values[column_index(table, name)?].clone()),
        Expr::Unary { .. } | Expr::Binary { .. } | Expr::IsNull { .. } => Ok(Value::Boolean(
            matches!(eval_truth(table, row, expr)?, Truth::True),
        )),
    }
}

fn value_from_literal(literal: &Literal) -> Value {
    match literal {
        Literal::Integer(value) => Value::Integer(*value),
        Literal::Text(value) => Value::Text(value.clone()),
        Literal::Boolean(value) => Value::Boolean(*value),
        Literal::Null => Value::Null,
    }
}

fn row_matches(table: &Table, row: &Row, where_clause: Option<&Expr>) -> Result<bool> {
    let Some(expr) = where_clause else {
        return Ok(true);
    };
    Ok(matches!(eval_truth(table, row, expr)?, Truth::True))
}

fn eval_truth(table: &Table, row: &Row, expr: &Expr) -> Result<Truth> {
    match expr {
        Expr::Unary {
            op: UnaryOp::Not,
            expr,
        } => Ok(not_truth(eval_truth(table, row, expr)?)),
        Expr::Binary { left, op, right } => match op {
            BinaryOp::And => Ok(and_truth(
                eval_truth(table, row, left)?,
                eval_truth(table, row, right)?,
            )),
            BinaryOp::Or => Ok(or_truth(
                eval_truth(table, row, left)?,
                eval_truth(table, row, right)?,
            )),
            BinaryOp::Eq
            | BinaryOp::NotEq
            | BinaryOp::Lt
            | BinaryOp::LtEq
            | BinaryOp::Gt
            | BinaryOp::GtEq => {
                let left = eval_expr(table, row, left)?;
                let right = eval_expr(table, row, right)?;
                compare_values(&left, *op, &right)
            }
        },
        Expr::IsNull { expr, negated } => {
            let is_null = matches!(eval_expr(table, row, expr)?, Value::Null);
            Ok(if is_null ^ *negated {
                Truth::True
            } else {
                Truth::False
            })
        }
        Expr::Literal(Literal::Boolean(value)) => {
            Ok(if *value { Truth::True } else { Truth::False })
        }
        Expr::Literal(Literal::Null) => Ok(Truth::Unknown),
        Expr::Literal(_) | Expr::Identifier(_) => match eval_expr(table, row, expr)? {
            Value::Boolean(value) => Ok(if value { Truth::True } else { Truth::False }),
            Value::Null => Ok(Truth::Unknown),
            _ => Err(execution_error("WHERE expression must evaluate to BOOLEAN")),
        },
    }
}

fn compare_values(left: &Value, op: BinaryOp, right: &Value) -> Result<Truth> {
    if matches!(left, Value::Null) || matches!(right, Value::Null) {
        return Ok(Truth::Unknown);
    }
    let result = match (left, right) {
        (Value::Integer(left), Value::Integer(right)) => compare_ordering(left.cmp(right), op),
        (Value::Text(left), Value::Text(right)) => compare_ordering(left.cmp(right), op),
        (Value::Boolean(left), Value::Boolean(right)) => match op {
            BinaryOp::Eq => left == right,
            BinaryOp::NotEq => left != right,
            _ => return Err(execution_error("BOOLEAN only supports = and !=")),
        },
        _ => return Err(execution_error("cannot compare values of different types")),
    };
    Ok(if result { Truth::True } else { Truth::False })
}

fn compare_ordering(ordering: std::cmp::Ordering, op: BinaryOp) -> bool {
    match op {
        BinaryOp::Eq => ordering.is_eq(),
        BinaryOp::NotEq => !ordering.is_eq(),
        BinaryOp::Lt => ordering.is_lt(),
        BinaryOp::LtEq => ordering.is_lt() || ordering.is_eq(),
        BinaryOp::Gt => ordering.is_gt(),
        BinaryOp::GtEq => ordering.is_gt() || ordering.is_eq(),
        BinaryOp::And | BinaryOp::Or => false,
    }
}

fn and_truth(left: Truth, right: Truth) -> Truth {
    match (left, right) {
        (Truth::False, _) | (_, Truth::False) => Truth::False,
        (Truth::True, Truth::True) => Truth::True,
        _ => Truth::Unknown,
    }
}

fn or_truth(left: Truth, right: Truth) -> Truth {
    match (left, right) {
        (Truth::True, _) | (_, Truth::True) => Truth::True,
        (Truth::False, Truth::False) => Truth::False,
        _ => Truth::Unknown,
    }
}

fn not_truth(value: Truth) -> Truth {
    match value {
        Truth::True => Truth::False,
        Truth::False => Truth::True,
        Truth::Unknown => Truth::Unknown,
    }
}

fn validate_row(table: &Table, values: &[Value], current_row_index: Option<usize>) -> Result<()> {
    for (column, value) in table.columns.iter().zip(values) {
        if column.not_null && matches!(value, Value::Null) {
            return Err(execution_error(format!(
                "column '{}' cannot be NULL",
                column.name
            )));
        }
        if !matches!(value, Value::Null) {
            match (&column.data_type, value) {
                (DataType::Integer, Value::Integer(_))
                | (DataType::Text, Value::Text(_))
                | (DataType::Boolean, Value::Boolean(_)) => {}
                _ => {
                    return Err(execution_error(format!(
                        "column '{}' has wrong data type",
                        column.name
                    )));
                }
            }
        }
    }
    if let Some(primary_key_index) = table.columns.iter().position(|column| column.primary_key) {
        if matches!(values[primary_key_index], Value::Null) {
            return Err(execution_error("PRIMARY KEY cannot be NULL"));
        }
        for (index, row) in table.rows.iter().enumerate() {
            if Some(index) != current_row_index
                && row.values[primary_key_index] == values[primary_key_index]
            {
                return Err(execution_error("PRIMARY KEY value already exists"));
            }
        }
    }
    Ok(())
}

fn execution_error(message: impl Into<String>) -> DbError {
    DbError::Execution(message.into())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        std::env::temp_dir().join(format!("baesql-{name}-{unique}.bae"))
    }

    fn rows(result: &QueryResult) -> &[Vec<Value>] {
        match result {
            QueryResult::Rows { rows, .. } => rows,
            QueryResult::Message(message) => panic!("expected rows, got {message}"),
        }
    }

    #[test]
    fn crud_works() {
        let path = temp_path("crud");
        let mut engine = Engine::open(&path).expect("open");
        engine
            .execute_script(
                "
                CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, active BOOLEAN);
                INSERT INTO users VALUES (1, 'Bae', TRUE);
                UPDATE users SET name = 'SQL' WHERE id = 1;
            ",
            )
            .expect("execute");
        let result = engine
            .execute_script("SELECT id, name FROM users WHERE active = TRUE;")
            .expect("select")
            .remove(0);
        assert_eq!(
            rows(&result),
            &[vec![Value::Integer(1), Value::Text("SQL".to_string())]]
        );
        engine
            .execute_script("DELETE FROM users WHERE id = 1;")
            .expect("delete");
        let result = engine
            .execute_script("SELECT * FROM users;")
            .expect("select")
            .remove(0);
        assert!(rows(&result).is_empty());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn data_persists_after_reopen() {
        let path = temp_path("persist");
        {
            let mut engine = Engine::open(&path).expect("open");
            engine
                .execute_script(
                    "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);
                     INSERT INTO users VALUES (1, 'persisted');",
                )
                .expect("execute");
        }
        let mut engine = Engine::open(&path).expect("reopen");
        let result = engine
            .execute_script("SELECT name FROM users WHERE id = 1;")
            .expect("select")
            .remove(0);
        assert_eq!(rows(&result), &[vec![Value::Text("persisted".to_string())]]);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn primary_key_and_not_null_are_enforced() {
        let path = temp_path("constraints");
        let mut engine = Engine::open(&path).expect("open");
        engine
            .execute_script("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);")
            .expect("create");
        engine
            .execute_script("INSERT INTO users VALUES (1, 'one');")
            .expect("insert");
        assert!(
            engine
                .execute_script("INSERT INTO users VALUES (1, 'dupe');")
                .is_err()
        );
        assert!(
            engine
                .execute_script("INSERT INTO users VALUES (2, NULL);")
                .is_err()
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn commit_and_rollback_control_persistence() {
        let path = temp_path("tx");
        {
            let mut engine = Engine::open(&path).expect("open");
            engine
                .execute_script(
                    "CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT);
                     BEGIN;
                     INSERT INTO items VALUES (1, 'rolled');
                     ROLLBACK;",
                )
                .expect("rollback");
            let result = engine
                .execute_script("SELECT * FROM items;")
                .expect("select")
                .remove(0);
            assert!(rows(&result).is_empty());
            engine
                .execute_script(
                    "BEGIN;
                     INSERT INTO items VALUES (2, 'committed');
                     COMMIT;",
                )
                .expect("commit");
        }
        let mut reopened = Engine::open(&path).expect("reopen");
        let result = reopened
            .execute_script("SELECT id FROM items;")
            .expect("select")
            .remove(0);
        assert_eq!(rows(&result), &[vec![Value::Integer(2)]]);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn update_is_statement_atomic() {
        let path = temp_path("atomic");
        let mut engine = Engine::open(&path).expect("open");
        engine
            .execute_script(
                "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);
                 INSERT INTO users VALUES (1, 'one');
                 INSERT INTO users VALUES (2, 'two');",
            )
            .expect("setup");
        assert!(
            engine
                .execute_script("UPDATE users SET id = 1 WHERE id = 2;")
                .is_err()
        );
        let result = engine
            .execute_script("SELECT id FROM users WHERE name = 'two';")
            .expect("select")
            .remove(0);
        assert_eq!(rows(&result), &[vec![Value::Integer(2)]]);
        let _ = fs::remove_file(path);
    }
}
