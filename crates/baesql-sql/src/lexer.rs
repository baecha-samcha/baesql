use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    Keyword(Keyword),
    Identifier(String),
    Integer(i64),
    String(String),
    Star,
    Comma,
    Semicolon,
    LParen,
    RParen,
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keyword {
    Create,
    Table,
    Drop,
    Insert,
    Into,
    Values,
    Select,
    From,
    Where,
    Update,
    Set,
    Delete,
    Begin,
    Commit,
    Rollback,
    Integer,
    Text,
    Boolean,
    Primary,
    Key,
    Not,
    Null,
    And,
    Or,
    Is,
    True,
    False,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexError {
    pub position: usize,
    pub message: String,
}

impl fmt::Display for LexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "lex error at byte {}: {}", self.position, self.message)
    }
}

impl std::error::Error for LexError {}

pub fn lex(input: &str) -> Result<Vec<Token>, LexError> {
    let mut lexer = Lexer { input, pos: 0 };
    lexer.lex_all()
}

struct Lexer<'a> {
    input: &'a str,
    pos: usize,
}

impl Lexer<'_> {
    fn lex_all(&mut self) -> Result<Vec<Token>, LexError> {
        let mut tokens = Vec::new();
        while let Some(ch) = self.peek_char() {
            match ch {
                c if c.is_whitespace() => {
                    self.bump_char();
                }
                ',' => {
                    self.bump_char();
                    tokens.push(Token::Comma);
                }
                ';' => {
                    self.bump_char();
                    tokens.push(Token::Semicolon);
                }
                '(' => {
                    self.bump_char();
                    tokens.push(Token::LParen);
                }
                ')' => {
                    self.bump_char();
                    tokens.push(Token::RParen);
                }
                '*' => {
                    self.bump_char();
                    tokens.push(Token::Star);
                }
                '=' => {
                    self.bump_char();
                    tokens.push(Token::Eq);
                }
                '!' => {
                    let start = self.pos;
                    self.bump_char();
                    if self.peek_char() == Some('=') {
                        self.bump_char();
                        tokens.push(Token::NotEq);
                    } else {
                        return Err(self.error_at(start, "expected '=' after '!'"));
                    }
                }
                '<' => {
                    self.bump_char();
                    match self.peek_char() {
                        Some('=') => {
                            self.bump_char();
                            tokens.push(Token::LtEq);
                        }
                        Some('>') => {
                            self.bump_char();
                            tokens.push(Token::NotEq);
                        }
                        _ => tokens.push(Token::Lt),
                    }
                }
                '>' => {
                    self.bump_char();
                    if self.peek_char() == Some('=') {
                        self.bump_char();
                        tokens.push(Token::GtEq);
                    } else {
                        tokens.push(Token::Gt);
                    }
                }
                '\'' => tokens.push(Token::String(self.lex_string()?)),
                c if c.is_ascii_digit() => tokens.push(Token::Integer(self.lex_integer()?)),
                c if is_identifier_start(c) => tokens.push(self.lex_word()),
                _ => return Err(self.error(format!("unexpected character '{ch}'"))),
            }
        }
        Ok(tokens)
    }

    fn lex_string(&mut self) -> Result<String, LexError> {
        let start = self.pos;
        self.bump_char();
        let mut value = String::new();
        while let Some(ch) = self.peek_char() {
            self.bump_char();
            if ch == '\'' {
                if self.peek_char() == Some('\'') {
                    self.bump_char();
                    value.push('\'');
                } else {
                    return Ok(value);
                }
            } else {
                value.push(ch);
            }
        }
        Err(self.error_at(start, "unterminated string literal"))
    }

    fn lex_integer(&mut self) -> Result<i64, LexError> {
        let start = self.pos;
        while matches!(self.peek_char(), Some(c) if c.is_ascii_digit()) {
            self.bump_char();
        }
        self.input[start..self.pos]
            .parse::<i64>()
            .map_err(|_| self.error_at(start, "integer literal is out of range"))
    }

    fn lex_word(&mut self) -> Token {
        let start = self.pos;
        while matches!(self.peek_char(), Some(c) if is_identifier_continue(c)) {
            self.bump_char();
        }
        let raw = &self.input[start..self.pos];
        match keyword(raw) {
            Some(keyword) => Token::Keyword(keyword),
            None => Token::Identifier(raw.to_ascii_lowercase()),
        }
    }

    fn peek_char(&self) -> Option<char> {
        self.input[self.pos..].chars().next()
    }

    fn bump_char(&mut self) {
        if let Some(ch) = self.peek_char() {
            self.pos += ch.len_utf8();
        }
    }

    fn error(&self, message: impl Into<String>) -> LexError {
        self.error_at(self.pos, message)
    }

    fn error_at(&self, position: usize, message: impl Into<String>) -> LexError {
        LexError {
            position,
            message: message.into(),
        }
    }
}

fn is_identifier_start(ch: char) -> bool {
    ch.is_ascii_alphabetic() || ch == '_'
}

fn is_identifier_continue(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

fn keyword(raw: &str) -> Option<Keyword> {
    match raw.to_ascii_uppercase().as_str() {
        "CREATE" => Some(Keyword::Create),
        "TABLE" => Some(Keyword::Table),
        "DROP" => Some(Keyword::Drop),
        "INSERT" => Some(Keyword::Insert),
        "INTO" => Some(Keyword::Into),
        "VALUES" => Some(Keyword::Values),
        "SELECT" => Some(Keyword::Select),
        "FROM" => Some(Keyword::From),
        "WHERE" => Some(Keyword::Where),
        "UPDATE" => Some(Keyword::Update),
        "SET" => Some(Keyword::Set),
        "DELETE" => Some(Keyword::Delete),
        "BEGIN" => Some(Keyword::Begin),
        "COMMIT" => Some(Keyword::Commit),
        "ROLLBACK" => Some(Keyword::Rollback),
        "INTEGER" => Some(Keyword::Integer),
        "TEXT" => Some(Keyword::Text),
        "BOOLEAN" => Some(Keyword::Boolean),
        "PRIMARY" => Some(Keyword::Primary),
        "KEY" => Some(Keyword::Key),
        "NOT" => Some(Keyword::Not),
        "NULL" => Some(Keyword::Null),
        "AND" => Some(Keyword::And),
        "OR" => Some(Keyword::Or),
        "IS" => Some(Keyword::Is),
        "TRUE" => Some(Keyword::True),
        "FALSE" => Some(Keyword::False),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexes_keywords_case_insensitively_and_escaped_strings() {
        let tokens = lex("SeLeCt 'it''s', TRUE FROM users;").expect("lex");
        assert_eq!(
            tokens,
            vec![
                Token::Keyword(Keyword::Select),
                Token::String("it's".to_string()),
                Token::Comma,
                Token::Keyword(Keyword::True),
                Token::Keyword(Keyword::From),
                Token::Identifier("users".to_string()),
                Token::Semicolon,
            ]
        );
    }

    #[test]
    fn rejects_unterminated_string() {
        assert!(lex("SELECT 'oops").is_err());
    }
}
