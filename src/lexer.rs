//! The tokens of a predicate.
//!
//! Two kinds of quote, because a predicate holds two kinds of string: a path
//! in single quotes, which the reader it is handed to will interpret, and a
//! text literal in double quotes, which the predicate compares with. Keeping
//! them apart at the lexer means `'/name' = "name"` can never be misread.
//! Bare words are keywords and function names; the parser sorts them.

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

/// Split `predicate` into tokens.
///
/// # Errors
/// A character that begins no token, an unterminated string, or a number that
/// does not parse.
pub fn tokenize(predicate: &str) -> Result<Vec<Token>, ContractError> {
    let mut tokens = Vec::new();
    let mut rest = predicate;
    while let Some(first) = rest.chars().next() {
        let consumed = match first {
            character if character.is_whitespace() => 1,
            '(' => push(&mut tokens, Token::OpenParen, 1),
            ')' => push(&mut tokens, Token::CloseParen, 1),
            ',' => push(&mut tokens, Token::Comma, 1),
            '=' => push(&mut tokens, Token::Equal, 1),
            '!' if rest.starts_with("!=") => push(&mut tokens, Token::NotEqual, 2),
            '<' if rest.starts_with("<=") => push(&mut tokens, Token::LessOrEqual, 2),
            '<' => push(&mut tokens, Token::Less, 1),
            '>' if rest.starts_with(">=") => push(&mut tokens, Token::GreaterOrEqual, 2),
            '>' => push(&mut tokens, Token::Greater, 1),
            '\'' | '"' => quoted(rest, first, &mut tokens)?,
            character if character.is_ascii_digit() => number(rest, &mut tokens)?,
            '-' if rest[1..].starts_with(|character: char| character.is_ascii_digit()) => {
                number(rest, &mut tokens)?
            }
            character if character.is_alphabetic() => word(rest, &mut tokens),
            other => {
                return Err(error(format!(
                    "unexpected {other:?} at {} in {predicate:?}",
                    predicate.len() - rest.len()
                )));
            }
        };
        rest = &rest[consumed..];
    }
    Ok(tokens)
}

fn push(tokens: &mut Vec<Token>, token: Token, width: usize) -> usize {
    tokens.push(token);
    width
}

/// A string in `quote`s; a backslash escapes the character after it.
fn quoted(rest: &str, quote: char, tokens: &mut Vec<Token>) -> Result<usize, ContractError> {
    let mut text = String::new();
    let mut characters = rest.char_indices().skip(1);
    while let Some((at, character)) = characters.next() {
        match character {
            '\\' => match characters.next() {
                Some((_, escaped)) => text.push(escaped),
                None => break,
            },
            character if character == quote => {
                tokens.push(if quote == '\'' {
                    Token::Path(text)
                } else {
                    Token::Text(text)
                });
                return Ok(at + 1);
            }
            other => text.push(other),
        }
    }
    Err(error(format!("unterminated string in {rest:?}")))
}

fn number(rest: &str, tokens: &mut Vec<Token>) -> Result<usize, ContractError> {
    let end = rest[1..]
        .find(|character: char| !character.is_ascii_digit() && character != '.')
        .map_or(rest.len(), |offset| offset + 1);
    let digits = rest[..end].trim_end_matches('.');
    let token = if digits.contains('.') {
        Token::Decimal(digits.parse().map_err(|_| not_a_number(digits))?)
    } else {
        Token::Integer(digits.parse().map_err(|_| not_a_number(digits))?)
    };
    tokens.push(token);
    Ok(digits.len())
}

fn not_a_number(digits: &str) -> ContractError {
    error(format!("{digits:?} is not a number"))
}

fn word(rest: &str, tokens: &mut Vec<Token>) -> usize {
    let end = rest
        .find(|character: char| {
            !character.is_alphanumeric() && character != '-' && character != '_'
        })
        .unwrap_or(rest.len());
    tokens.push(Token::Word(rest[..end].to_string()));
    end
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
}
