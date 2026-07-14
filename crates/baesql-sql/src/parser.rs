use std::fmt;

use crate::ast::{BinaryOp, ColumnDef, DataType, Expr, Literal, Projection, Statement, UnaryOp};
use crate::lexer::{Keyword, LexError, Token, lex};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "parse error: {}", self.message)
    }
}

impl std::error::Error for ParseError {}

impl From<LexError> for ParseError {
    fn from(value: LexError) -> Self {
        Self {
            message: value.to_string(),
        }
    }
}

pub fn parse_statement(input: &str) -> Result<Statement, ParseError> {
    let mut statements = parse_script(input)?;
    if statements.len() != 1 {
        return Err(ParseError {
            message: format!("expected one statement, got {}", statements.len()),
        });
    }
    Ok(statements.remove(0))
}

pub fn parse_script(input: &str) -> Result<Vec<Statement>, ParseError> {
    let tokens = lex(input)?;
    Parser::new(tokens).parse_script()
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn parse_script(&mut self) -> Result<Vec<Statement>, ParseError> {
        let mut statements = Vec::new();
        while !self.is_eof() {
            if self.consume_semicolon() {
                continue;
            }
            statements.push(self.parse_statement_inner()?);
            let _ = self.consume_semicolon();
        }
        Ok(statements)
    }

    fn parse_statement_inner(&mut self) -> Result<Statement, ParseError> {
        match self.peek() {
            Some(Token::Keyword(Keyword::Create)) => self.parse_create_table(),
            Some(Token::Keyword(Keyword::Drop)) => self.parse_drop_table(),
            Some(Token::Keyword(Keyword::Insert)) => self.parse_insert(),
            Some(Token::Keyword(Keyword::Select)) => self.parse_select(),
            Some(Token::Keyword(Keyword::Update)) => self.parse_update(),
            Some(Token::Keyword(Keyword::Delete)) => self.parse_delete(),
            Some(Token::Keyword(Keyword::Begin)) => {
                self.bump();
                Ok(Statement::Begin)
            }
            Some(Token::Keyword(Keyword::Commit)) => {
                self.bump();
                Ok(Statement::Commit)
            }
            Some(Token::Keyword(Keyword::Rollback)) => {
                self.bump();
                Ok(Statement::Rollback)
            }
            Some(token) => Err(self.error(format!("unexpected token {token:?}"))),
            None => Err(self.error("expected statement")),
        }
    }

    fn parse_create_table(&mut self) -> Result<Statement, ParseError> {
        self.expect_keyword(Keyword::Create)?;
        self.expect_keyword(Keyword::Table)?;
        let name = self.expect_identifier()?;
        self.expect_token(&Token::LParen)?;
        let mut columns = Vec::new();
        loop {
            let column_name = self.expect_identifier()?;
            let data_type = self.parse_data_type()?;
            let mut primary_key = false;
            let mut not_null = false;
            loop {
                if self.consume_keyword(Keyword::Primary) {
                    self.expect_keyword(Keyword::Key)?;
                    primary_key = true;
                } else if self.consume_keyword(Keyword::Not) {
                    self.expect_keyword(Keyword::Null)?;
                    not_null = true;
                } else {
                    break;
                }
            }
            columns.push(ColumnDef {
                name: column_name,
                data_type,
                primary_key,
                not_null,
            });
            if self.consume_token(&Token::Comma) {
                continue;
            }
            break;
        }
        self.expect_token(&Token::RParen)?;
        Ok(Statement::CreateTable { name, columns })
    }

    fn parse_drop_table(&mut self) -> Result<Statement, ParseError> {
        self.expect_keyword(Keyword::Drop)?;
        self.expect_keyword(Keyword::Table)?;
        Ok(Statement::DropTable {
            name: self.expect_identifier()?,
        })
    }

    fn parse_insert(&mut self) -> Result<Statement, ParseError> {
        self.expect_keyword(Keyword::Insert)?;
        self.expect_keyword(Keyword::Into)?;
        let table = self.expect_identifier()?;
        let columns = if self.consume_token(&Token::LParen) {
            let mut columns = Vec::new();
            loop {
                columns.push(self.expect_identifier()?);
                if self.consume_token(&Token::Comma) {
                    continue;
                }
                break;
            }
            self.expect_token(&Token::RParen)?;
            Some(columns)
        } else {
            None
        };
        self.expect_keyword(Keyword::Values)?;
        self.expect_token(&Token::LParen)?;
        let mut values = Vec::new();
        loop {
            values.push(self.parse_expr()?);
            if self.consume_token(&Token::Comma) {
                continue;
            }
            break;
        }
        self.expect_token(&Token::RParen)?;
        Ok(Statement::Insert {
            table,
            columns,
            values,
        })
    }

    fn parse_select(&mut self) -> Result<Statement, ParseError> {
        self.expect_keyword(Keyword::Select)?;
        let projection = if self.consume_token(&Token::Star) {
            Projection::All
        } else {
            let mut columns = Vec::new();
            loop {
                columns.push(self.expect_identifier()?);
                if self.consume_token(&Token::Comma) {
                    continue;
                }
                break;
            }
            Projection::Columns(columns)
        };
        self.expect_keyword(Keyword::From)?;
        let table = self.expect_identifier()?;
        let where_clause = self.parse_optional_where()?;
        Ok(Statement::Select {
            table,
            projection,
            where_clause,
        })
    }

    fn parse_update(&mut self) -> Result<Statement, ParseError> {
        self.expect_keyword(Keyword::Update)?;
        let table = self.expect_identifier()?;
        self.expect_keyword(Keyword::Set)?;
        let mut assignments = Vec::new();
        loop {
            let column = self.expect_identifier()?;
            self.expect_token(&Token::Eq)?;
            let expr = self.parse_expr()?;
            assignments.push((column, expr));
            if self.consume_token(&Token::Comma) {
                continue;
            }
            break;
        }
        let where_clause = self.parse_optional_where()?;
        Ok(Statement::Update {
            table,
            assignments,
            where_clause,
        })
    }

    fn parse_delete(&mut self) -> Result<Statement, ParseError> {
        self.expect_keyword(Keyword::Delete)?;
        self.expect_keyword(Keyword::From)?;
        let table = self.expect_identifier()?;
        let where_clause = self.parse_optional_where()?;
        Ok(Statement::Delete {
            table,
            where_clause,
        })
    }

    fn parse_optional_where(&mut self) -> Result<Option<Expr>, ParseError> {
        if self.consume_keyword(Keyword::Where) {
            Ok(Some(self.parse_expr()?))
        } else {
            Ok(None)
        }
    }

    fn parse_data_type(&mut self) -> Result<DataType, ParseError> {
        if self.consume_keyword(Keyword::Integer) {
            Ok(DataType::Integer)
        } else if self.consume_keyword(Keyword::Text) {
            Ok(DataType::Text)
        } else if self.consume_keyword(Keyword::Boolean) {
            Ok(DataType::Boolean)
        } else {
            Err(self.error("expected data type"))
        }
    }

    fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_and()?;
        while self.consume_keyword(Keyword::Or) {
            let right = self.parse_and()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op: BinaryOp::Or,
                right: Box::new(right),
            };
        }
        Ok(expr)
    }

    fn parse_and(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_not()?;
        while self.consume_keyword(Keyword::And) {
            let right = self.parse_not()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op: BinaryOp::And,
                right: Box::new(right),
            };
        }
        Ok(expr)
    }

    fn parse_not(&mut self) -> Result<Expr, ParseError> {
        if self.consume_keyword(Keyword::Not) {
            Ok(Expr::Unary {
                op: UnaryOp::Not,
                expr: Box::new(self.parse_not()?),
            })
        } else {
            self.parse_comparison()
        }
    }

    fn parse_comparison(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_primary()?;
        if self.consume_keyword(Keyword::Is) {
            let negated = self.consume_keyword(Keyword::Not);
            self.expect_keyword(Keyword::Null)?;
            expr = Expr::IsNull {
                expr: Box::new(expr),
                negated,
            };
        } else if let Some(op) = self.consume_comparison_op() {
            let right = self.parse_primary()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(right),
            };
        }
        Ok(expr)
    }

    fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        match self.peek().cloned() {
            Some(Token::Integer(value)) => {
                self.bump();
                Ok(Expr::Literal(Literal::Integer(value)))
            }
            Some(Token::String(value)) => {
                self.bump();
                Ok(Expr::Literal(Literal::Text(value)))
            }
            Some(Token::Keyword(Keyword::True)) => {
                self.bump();
                Ok(Expr::Literal(Literal::Boolean(true)))
            }
            Some(Token::Keyword(Keyword::False)) => {
                self.bump();
                Ok(Expr::Literal(Literal::Boolean(false)))
            }
            Some(Token::Keyword(Keyword::Null)) => {
                self.bump();
                Ok(Expr::Literal(Literal::Null))
            }
            Some(Token::Identifier(name)) => {
                self.bump();
                Ok(Expr::Identifier(name))
            }
            Some(Token::LParen) => {
                self.bump();
                let expr = self.parse_expr()?;
                self.expect_token(&Token::RParen)?;
                Ok(expr)
            }
            Some(token) => Err(self.error(format!("expected expression, got {token:?}"))),
            None => Err(self.error("expected expression")),
        }
    }

    fn consume_comparison_op(&mut self) -> Option<BinaryOp> {
        match self.peek() {
            Some(Token::Eq) => {
                self.bump();
                Some(BinaryOp::Eq)
            }
            Some(Token::NotEq) => {
                self.bump();
                Some(BinaryOp::NotEq)
            }
            Some(Token::Lt) => {
                self.bump();
                Some(BinaryOp::Lt)
            }
            Some(Token::LtEq) => {
                self.bump();
                Some(BinaryOp::LtEq)
            }
            Some(Token::Gt) => {
                self.bump();
                Some(BinaryOp::Gt)
            }
            Some(Token::GtEq) => {
                self.bump();
                Some(BinaryOp::GtEq)
            }
            _ => None,
        }
    }

    fn expect_identifier(&mut self) -> Result<String, ParseError> {
        match self.peek().cloned() {
            Some(Token::Identifier(name)) => {
                self.bump();
                Ok(name)
            }
            Some(token) => Err(self.error(format!("expected identifier, got {token:?}"))),
            None => Err(self.error("expected identifier")),
        }
    }

    fn expect_keyword(&mut self, keyword: Keyword) -> Result<(), ParseError> {
        if self.consume_keyword(keyword) {
            Ok(())
        } else {
            Err(self.error(format!("expected keyword {keyword:?}")))
        }
    }

    fn consume_keyword(&mut self, keyword: Keyword) -> bool {
        if self.peek() == Some(&Token::Keyword(keyword)) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn expect_token(&mut self, token: &Token) -> Result<(), ParseError> {
        if self.consume_token(token) {
            Ok(())
        } else {
            Err(self.error(format!("expected token {token:?}")))
        }
    }

    fn consume_token(&mut self, token: &Token) -> bool {
        if self.peek() == Some(token) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn consume_semicolon(&mut self) -> bool {
        self.consume_token(&Token::Semicolon)
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn bump(&mut self) {
        self.pos += 1;
    }

    fn is_eof(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    fn error(&self, message: impl Into<String>) -> ParseError {
        ParseError {
            message: format!("{} near token {}", message.into(), self.pos),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_create_table() {
        let stmt = parse_statement(
            "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, active BOOLEAN)",
        )
        .expect("parse");
        assert!(matches!(stmt, Statement::CreateTable { .. }));
    }

    #[test]
    fn parses_select_where_precedence() {
        let stmt = parse_statement("SELECT id, name FROM users WHERE active = TRUE AND id >= 2;")
            .expect("parse");
        assert!(matches!(
            stmt,
            Statement::Select {
                projection: Projection::Columns(_),
                where_clause: Some(_),
                ..
            }
        ));
    }

    #[test]
    fn rejects_bad_sql() {
        assert!(parse_statement("SELECT FROM users").is_err());
    }
}
