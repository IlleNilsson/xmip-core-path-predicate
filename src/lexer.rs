//! The tokens of a predicate.
//!
//! Two kinds of quote, because a predicate holds two kinds of string: a path
//! in single quotes, which the reader it is handed to will interpret, and a
//! text literal in double quotes, which the predicate compares with. Keeping
//! them apart at the lexer means `'/name' = "name"` can never be misread.
//! Bare words are keywords and function names; the parser sorts them.

use codec::char_reader::CharReader;
use contract::ContractError;

/// One token of a predicate.
#[derive(Clone, Debug, PartialEq)]
pub enum Token {
    /// A single-quoted path, quotes removed, in the reader's own language.
    Path(String),
    /// A double-quoted text literal, quotes removed.
    Text(String),
    /// A whole number, optionally negative.
    Integer(i64),
    /// A number with a point.
    Decimal(f64),
    /// A bare word: `and`, `or`, `not`, `true`, `false`, `null`, or a function
    /// name such as `starts-with`.
    Word(String),
    /// `=`
    Equal,
    /// `!=`
    NotEqual,
    /// `<`
    Less,
    /// `<=`
    LessOrEqual,
    /// `>`
    Greater,
    /// `>=`
    GreaterOrEqual,
    /// `(`
    OpenParen,
    /// `)`
    CloseParen,
    /// `,`
    Comma,
}

/// Split `predicate` into tokens. Whitespace is any Unicode whitespace, and
/// a word may hold any letter.
///
/// # Errors
/// A character that begins no token, an unterminated string, or a number that
/// does not parse.
pub fn tokenize(predicate: &str) -> Result<Vec<Token>, ContractError> {
    let mut reader = CharReader::new(predicate);
    let mut tokens = Vec::new();
    while let Some(first) = reader.peek() {
        if reader.skip_whitespace() {
            continue;
        }
        let token = match first {
            '!' if reader.eat_str("!=") => Token::NotEqual,
            '<' if reader.eat_str("<=") => Token::LessOrEqual,
            '>' if reader.eat_str(">=") => Token::GreaterOrEqual,
            '(' => single(&mut reader, Token::OpenParen),
            ')' => single(&mut reader, Token::CloseParen),
            ',' => single(&mut reader, Token::Comma),
            '=' => single(&mut reader, Token::Equal),
            '<' => single(&mut reader, Token::Less),
            '>' => single(&mut reader, Token::Greater),
            '\'' | '"' => quoted(&mut reader, first)?,
            character if character.is_ascii_digit() => number(&mut reader)?,
            '-' if reader.peek_nth(1).is_some_and(|next| next.is_ascii_digit()) => {
                number(&mut reader)?
            }
            character if character.is_alphabetic() => {
                let word =
                    reader.take_while(|next| next.is_alphanumeric() || next == '-' || next == '_');
                Token::Word(word.to_string())
            }
            other => {
                return Err(error(format!(
                    "unexpected {other:?} at {} in {predicate:?}",
                    reader.offset()
                )));
            }
        };
        tokens.push(token);
    }
    Ok(tokens)
}

fn single(reader: &mut CharReader<'_>, token: Token) -> Token {
    reader.bump();
    token
}

/// A string in `quote`s; a backslash escapes the character after it.
fn quoted(reader: &mut CharReader<'_>, quote: char) -> Result<Token, ContractError> {
    let start = reader.offset();
    reader.bump();
    let mut text = String::new();
    while let Some(character) = reader.bump() {
        match character {
            '\\' => match reader.bump() {
                Some(escaped) => text.push(escaped),
                None => break,
            },
            character if character == quote => {
                return Ok(if quote == '\'' {
                    Token::Path(text)
                } else {
                    Token::Text(text)
                });
            }
            other => text.push(other),
        }
    }
    Err(error(format!(
        "unterminated string in {:?}",
        reader.since(start)
    )))
}

/// An optional minus, then digits and points; a trailing point is not the
/// number's.
fn number(reader: &mut CharReader<'_>) -> Result<Token, ContractError> {
    let start = reader.offset();
    reader.eat('-');
    let run = reader.peek_while(|character| character.is_ascii_digit() || character == '.');
    reader.eat_str(run.trim_end_matches('.'));
    let digits = reader.since(start);
    Ok(if digits.contains('.') {
        Token::Decimal(digits.parse().map_err(|_| not_a_number(digits))?)
    } else {
        Token::Integer(digits.parse().map_err(|_| not_a_number(digits))?)
    })
}

fn not_a_number(digits: &str) -> ContractError {
    error(format!("{digits:?} is not a number"))
}

pub(crate) fn error(message: impl std::fmt::Display) -> ContractError {
    ContractError {
        message: format!("predicate: {message}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizes_every_kind() {
        let tokens = tokenize(
            r#"'/order/amount' >= -12.5 and not starts-with('/id', "A\"1") or ('/n' != null, 3)"#,
        )
        .expect("lexes");
        assert_eq!(
            tokens,
            vec![
                Token::Path("/order/amount".into()),
                Token::GreaterOrEqual,
                Token::Decimal(-12.5),
                Token::Word("and".into()),
                Token::Word("not".into()),
                Token::Word("starts-with".into()),
                Token::OpenParen,
                Token::Path("/id".into()),
                Token::Comma,
                Token::Text("A\"1".into()),
                Token::CloseParen,
                Token::Word("or".into()),
                Token::OpenParen,
                Token::Path("/n".into()),
                Token::NotEqual,
                Token::Word("null".into()),
                Token::Comma,
                Token::Integer(3),
                Token::CloseParen,
            ]
        );
    }

    #[test]
    fn every_comparison_operator_lexes_on_its_own() {
        let tokens = tokenize("= != < <= > >=").expect("lexes");
        assert_eq!(
            tokens,
            vec![
                Token::Equal,
                Token::NotEqual,
                Token::Less,
                Token::LessOrEqual,
                Token::Greater,
                Token::GreaterOrEqual,
            ]
        );
    }

    #[test]
    fn refuses_stray_characters_and_open_strings() {
        let stray = tokenize("'/a' # 1").expect_err("refused");
        assert!(stray.message.contains("'#'"), "{}", stray.message);
        assert!(tokenize("'/a' = \"open").is_err());
        assert!(tokenize("'open = 1").is_err());
        assert!(tokenize("'/a' ! 1").is_err());
        assert!(tokenize("'/a' = 1.2.3").is_err());
    }

    #[test]
    fn multibyte_whitespace_and_letters_lex_without_panic() {
        let tokens =
            tokenize("'/naïve'\u{a0}=\u{3000}\"Zoë 名前\"\u{2003}and\u{a0}größer").expect("lexes");
        assert_eq!(
            tokens,
            vec![
                Token::Path("/naïve".into()),
                Token::Equal,
                Token::Text("Zoë 名前".into()),
                Token::Word("and".into()),
                Token::Word("größer".into()),
            ]
        );
        let alone = tokenize("-\u{a0}1").expect_err("a minus alone");
        assert!(alone.message.contains("'-' at 0"), "{}", alone.message);
        assert!(tokenize("'/a' = \"öpen\u{a0}").is_err());
        assert!(tokenize("'/a' = 1\u{a0}€").is_err());
    }
}
