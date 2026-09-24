//! The shape of a predicate and the parser that builds it.
//!
//! Precedence is the usual one: `not` binds tightest, then `and`, then `or`,
//! and parentheses override. So `a or b and c` is `a or (b and c)`, as in
//! every language a route author will have written before this one.
//!
//! The cursor the parser walks its tokens with is the capability's, shared
//! with `FHIRPath` (ADR-0044); the grammar below is the predicate's.

use crate::lexer::{Token, error, tokenize};
use path::cursor::Cursor;
use sdk::contract::{ContractError, StructuredValue};

/// One side of a comparison.
#[derive(Clone, Debug, PartialEq)]
pub enum Operand {
    /// A path the reader answers, `'...'`.
    Path(String),
    /// A literal: `"text"`, an integer, a decimal, `true`, `false` or `null`.
    Literal(StructuredValue),
}

/// The six comparisons.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Operator {
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
}

/// A parsed predicate.
#[derive(Clone, Debug, PartialEq)]
pub enum Expression {
    /// `left <operator> right`
    Compare {
        /// The left side.
        left: Operand,
        /// The comparison.
        operator: Operator,
        /// The right side.
        right: Operand,
    },
    /// `exists('path')`: the reader has a value there, null included.
    Exists(String),
    /// `starts-with('path', "text")`
    StartsWith {
        /// The path whose text is examined.
        path: String,
        /// The prefix looked for.
        text: String,
    },
    /// `contains('path', "text")`
    Contains {
        /// The path whose text is examined.
        path: String,
        /// The substring looked for.
        text: String,
    },
    /// `a and b`
    And(Box<Expression>, Box<Expression>),
    /// `a or b`
    Or(Box<Expression>, Box<Expression>),
    /// `not a`
    Not(Box<Expression>),
}

impl Expression {
    /// Parse `text`.
    ///
    /// # Errors
    /// The text does not lex, or is not a predicate of comparisons, functions,
    /// `and`, `or`, `not` and parentheses.
    pub fn parse(text: &str) -> Result<Self, ContractError> {
        let tokens = tokenize(text)?;
        let mut cursor = Cursor::new("predicate", &tokens);
        let parsed = disjunction(&mut cursor)?;
        match cursor.peek() {
            None => Ok(parsed),
            Some(token) => Err(error(format!("unexpected {token:?} after the predicate"))),
        }
    }
}

type Tokens<'a> = Cursor<'a, Token>;

fn disjunction(cursor: &mut Tokens<'_>) -> Result<Expression, ContractError> {
    let mut left = conjunction(cursor)?;
    while word_is(cursor, "or") {
        cursor.advance(1);
        let right = conjunction(cursor)?;
        left = Expression::Or(Box::new(left), Box::new(right));
    }
    Ok(left)
}

fn conjunction(cursor: &mut Tokens<'_>) -> Result<Expression, ContractError> {
    let mut left = negation(cursor)?;
    while word_is(cursor, "and") {
        cursor.advance(1);
        let right = negation(cursor)?;
        left = Expression::And(Box::new(left), Box::new(right));
    }
    Ok(left)
}

fn negation(cursor: &mut Tokens<'_>) -> Result<Expression, ContractError> {
    if word_is(cursor, "not") {
        cursor.advance(1);
        return Ok(Expression::Not(Box::new(negation(cursor)?)));
    }
    primary(cursor)
}

fn primary(cursor: &mut Tokens<'_>) -> Result<Expression, ContractError> {
    match cursor.peek() {
        Some(Token::OpenParen) => {
            cursor.advance(1);
            let inner = disjunction(cursor)?;
            cursor.expect(&Token::CloseParen)?;
            Ok(inner)
        }
        Some(Token::Word(name)) if cursor.peek_at(1) == Some(&Token::OpenParen) => {
            let name = name.clone();
            cursor.advance(2);
            let called = call(cursor, &name)?;
            cursor.expect(&Token::CloseParen)?;
            Ok(called)
        }
        _ => comparison(cursor),
    }
}

fn call(cursor: &mut Tokens<'_>, name: &str) -> Result<Expression, ContractError> {
    match name {
        "exists" => Ok(Expression::Exists(path(cursor)?)),
        "starts-with" | "contains" => {
            let path = path(cursor)?;
            cursor.expect(&Token::Comma)?;
            let text = match cursor.take() {
                Some(Token::Text(text)) => text.clone(),
                other => {
                    return Err(error(format!("{name}() needs a \"text\", found {other:?}")));
                }
            };
            Ok(if name == "contains" {
                Expression::Contains { path, text }
            } else {
                Expression::StartsWith { path, text }
            })
        }
        other => Err(error(format!(
            "{other}() is not a function a predicate has"
        ))),
    }
}

fn comparison(cursor: &mut Tokens<'_>) -> Result<Expression, ContractError> {
    let left = operand(cursor)?;
    let operator = match cursor.take() {
        Some(Token::Equal) => Operator::Equal,
        Some(Token::NotEqual) => Operator::NotEqual,
        Some(Token::Less) => Operator::Less,
        Some(Token::LessOrEqual) => Operator::LessOrEqual,
        Some(Token::Greater) => Operator::Greater,
        Some(Token::GreaterOrEqual) => Operator::GreaterOrEqual,
        other => return Err(error(format!("expected a comparison, found {other:?}"))),
    };
    let right = operand(cursor)?;
    Ok(Expression::Compare {
        left,
        operator,
        right,
    })
}

fn operand(cursor: &mut Tokens<'_>) -> Result<Operand, ContractError> {
    let literal = match cursor.take() {
        Some(Token::Path(path)) => return Ok(Operand::Path(path.clone())),
        Some(Token::Text(text)) => StructuredValue::Text(text.clone()),
        Some(Token::Integer(integer)) => StructuredValue::Integer(*integer),
        Some(Token::Decimal(decimal)) => StructuredValue::Decimal(*decimal),
        Some(Token::Word(word)) if word == "true" => StructuredValue::Bool(true),
        Some(Token::Word(word)) if word == "false" => StructuredValue::Bool(false),
        Some(Token::Word(word)) if word == "null" => StructuredValue::Null,
        other => {
            return Err(error(format!(
                "expected a 'path' or a literal, found {other:?}"
            )));
        }
    };
    Ok(Operand::Literal(literal))
}

fn path(cursor: &mut Tokens<'_>) -> Result<String, ContractError> {
    match cursor.take() {
        Some(Token::Path(path)) => Ok(path.clone()),
        other => Err(error(format!("expected a 'path', found {other:?}"))),
    }
}

fn word_is(cursor: &Tokens<'_>, word: &str) -> bool {
    matches!(cursor.peek(), Some(Token::Word(found)) if found == word)
}

#[cfg(test)]
mod tests {
    use super::*;

    // A boxed operand is what the tree holds, so the helper hands one back.
    #[allow(clippy::unnecessary_box_returns)]
    fn equals(path_text: &str, integer: i64) -> Box<Expression> {
        Box::new(Expression::Compare {
            left: path(path_text),
            operator: Operator::Equal,
            right: Operand::Literal(StructuredValue::Integer(integer)),
        })
    }

    fn path(text: &str) -> Operand {
        Operand::Path(text.into())
    }

    #[test]
    fn or_binds_looser_than_and_which_binds_looser_than_not() {
        let parsed = Expression::parse("'/a' = 1 or not '/b' = 2 and '/c' = 3").expect("parses");
        let a = equals("/a", 1);
        let b = equals("/b", 2);
        let c = equals("/c", 3);
        assert_eq!(
            parsed,
            Expression::Or(
                a,
                Box::new(Expression::And(Box::new(Expression::Not(b)), c))
            )
        );
    }

    #[test]
    fn parentheses_functions_and_every_literal_parse() {
        let parsed = Expression::parse(concat!(
            r#"('/a' = "x" or '/b' != null) and exists('/c') "#,
            r#"and starts-with('/d', "pre") and contains('/e', "in") "#,
            "and '/f' <= true"
        ))
        .expect("parses");
        let Expression::And(rest, last) = parsed else {
            panic!("and at the top");
        };
        assert_eq!(
            *last,
            Expression::Compare {
                left: path("/f"),
                operator: Operator::LessOrEqual,
                right: Operand::Literal(StructuredValue::Bool(true)),
            }
        );
        let Expression::And(rest, contains) = *rest else {
            panic!("and below");
        };
        assert_eq!(
            *contains,
            Expression::Contains {
                path: "/e".into(),
                text: "in".into()
            }
        );
        let Expression::And(rest, starts) = *rest else {
            panic!("and below");
        };
        assert_eq!(
            *starts,
            Expression::StartsWith {
                path: "/d".into(),
                text: "pre".into()
            }
        );
        let Expression::And(group, exists) = *rest else {
            panic!("and below");
        };
        assert_eq!(*exists, Expression::Exists("/c".into()));
        assert!(matches!(*group, Expression::Or(..)));
    }

    #[test]
    fn refuses_what_a_predicate_does_not_have() {
        let unknown = Expression::parse("length('/a')").expect_err("refused");
        assert_eq!(
            unknown.message,
            "predicate: length() is not a function a predicate has"
        );
        assert!(Expression::parse("'/a'").is_err());
        assert!(Expression::parse("'/a' = ").is_err());
        assert!(Expression::parse("'/a' = 1 '/b' = 2").is_err());
        let unclosed = Expression::parse("('/a' = 1").expect_err("refused");
        assert_eq!(
            unclosed.message,
            "predicate: expected CloseParen, found None"
        );
        assert!(Expression::parse("exists(\"/a\")").is_err());
        assert!(Expression::parse("starts-with('/a', '/b')").is_err());
        assert!(Expression::parse("").is_err());
    }
}
