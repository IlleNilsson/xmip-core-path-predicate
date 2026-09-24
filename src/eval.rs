//! Evaluation of a predicate through the reader it is given.
//!
//! The engine never sees content. Every operand that is a path goes to
//! `reader.read(path)` in whatever language that reader speaks, and the
//! predicate reasons only about the scalars that come back. A path the reader
//! has nothing for compares as null: `= null` and `!= null` say so, `exists`
//! tells a present null from an absent path, and any other comparison with
//! it is simply false rather than an error, because a route that asks
//! `'/amount' > 100` of a message without an amount wants "no", not a fault.
//! Two present values of different kinds are a fault, named by both kinds:
//! that is a mistake in the predicate or the content, not a "no".

use crate::expression::{Expression, Operand, Operator};
use crate::lexer::error;
use contract::{ContractError, StructureReader, StructuredValue};
use std::cmp::Ordering;

/// Evaluate `expression` by reading its paths through `reader`.
///
/// # Errors
/// The reader fails a read; two present values are of kinds that do not
/// compare; `starts-with` or `contains` finds something other than text.
pub fn evaluate(
    expression: &Expression,
    reader: &dyn StructureReader,
) -> Result<bool, ContractError> {
    match expression {
        Expression::Compare {
            left,
            operator,
            right,
        } => compare(left, *operator, right, reader),
        Expression::Exists(path) => Ok(reader.read(path)?.is_some()),
        Expression::StartsWith { path, text } => {
            Ok(text_at(path, "starts-with", reader)?.is_some_and(|found| found.starts_with(text)))
        }
        Expression::Contains { path, text } => {
            Ok(text_at(path, "contains", reader)?.is_some_and(|found| found.contains(text)))
        }
        Expression::And(left, right) => Ok(evaluate(left, reader)? && evaluate(right, reader)?),
        Expression::Or(left, right) => Ok(evaluate(left, reader)? || evaluate(right, reader)?),
        Expression::Not(inner) => Ok(!evaluate(inner, reader)?),
    }
}

fn resolve(
    operand: &Operand,
    reader: &dyn StructureReader,
) -> Result<StructuredValue, ContractError> {
    Ok(match operand {
        Operand::Path(path) => reader.read(path)?.unwrap_or(StructuredValue::Null),
        Operand::Literal(value) => value.clone(),
    })
}

fn compare(
    left: &Operand,
    operator: Operator,
    right: &Operand,
    reader: &dyn StructureReader,
) -> Result<bool, ContractError> {
    let left_value = resolve(left, reader)?;
    let right_value = resolve(right, reader)?;
    let either_null =
        matches!(left_value, StructuredValue::Null) || matches!(right_value, StructuredValue::Null);
    if either_null {
        let both_null = matches!(
            (&left_value, &right_value),
            (StructuredValue::Null, StructuredValue::Null)
        );
        return Ok(match operator {
            Operator::Equal => both_null,
            Operator::NotEqual => !both_null,
            _ => false,
        });
    }
    let ordering = order(&left_value, &right_value).ok_or_else(|| {
        error(format!(
            "{} is {} and {} is {}: not comparable",
            describe(left),
            kind(&left_value),
            describe(right),
            kind(&right_value)
        ))
    })?;
    Ok(match operator {
        Operator::Equal => ordering == Ordering::Equal,
        Operator::NotEqual => ordering != Ordering::Equal,
        Operator::Less => ordering == Ordering::Less,
        Operator::LessOrEqual => ordering != Ordering::Greater,
        Operator::Greater => ordering == Ordering::Greater,
        Operator::GreaterOrEqual => ordering != Ordering::Less,
    })
}

/// How two present values order; `None` when their kinds do not compare.
/// An integer against a decimal is promoted to decimal; text is lexical.
fn order(left: &StructuredValue, right: &StructuredValue) -> Option<Ordering> {
    use StructuredValue::{Binary, Bool, Decimal, Integer, Text};
    Some(match (left, right) {
        (Integer(a), Integer(b)) => a.cmp(b),
        (Integer(a), Decimal(b)) => widen(*a).total_cmp(b),
        (Decimal(a), Integer(b)) => a.total_cmp(&widen(*b)),
        (Decimal(a), Decimal(b)) => a.total_cmp(b),
        (Text(a), Text(b)) => a.cmp(b),
        (Bool(a), Bool(b)) => a.cmp(b),
        (Binary(a), Binary(b)) => a.cmp(b),
        _ => return None,
    })
}

/// The promotion of an integer beside a decimal. Above 2^53 the widening
/// rounds, and a predicate over such a number beside a decimal is comparing
/// approximations already.
#[allow(clippy::cast_precision_loss)]
fn widen(integer: i64) -> f64 {
    integer as f64
}

fn text_at(
    path: &str,
    function: &str,
    reader: &dyn StructureReader,
) -> Result<Option<String>, ContractError> {
    match reader.read(path)? {
        None | Some(StructuredValue::Null) => Ok(None),
        Some(StructuredValue::Text(text)) => Ok(Some(text)),
        Some(other) => Err(error(format!(
            "'{path}' is {}; {function}() needs Text",
            kind(&other)
        ))),
    }
}

fn kind(value: &StructuredValue) -> &'static str {
    match value {
        StructuredValue::Null => "Null",
        StructuredValue::Bool(_) => "Bool",
        StructuredValue::Integer(_) => "Integer",
        StructuredValue::Decimal(_) => "Decimal",
        StructuredValue::Text(_) => "Text",
        StructuredValue::Binary(_) => "Binary",
    }
}

fn describe(operand: &Operand) -> String {
    match operand {
        Operand::Path(path) => format!("'{path}'"),
        Operand::Literal(StructuredValue::Text(text)) => format!("{text:?}"),
        Operand::Literal(StructuredValue::Null) => "null".to_string(),
        Operand::Literal(StructuredValue::Bool(flag)) => flag.to_string(),
        Operand::Literal(StructuredValue::Integer(integer)) => integer.to_string(),
        Operand::Literal(StructuredValue::Decimal(decimal)) => decimal.to_string(),
        Operand::Literal(StructuredValue::Binary(bytes)) => format!("{} bytes", bytes.len()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::MapReader;

    fn holds(predicate: &str, reader: &MapReader) -> bool {
        let parsed = Expression::parse(predicate).expect("parses");
        evaluate(&parsed, reader).expect("evaluates")
    }

    fn fails(predicate: &str, reader: &MapReader) -> String {
        let parsed = Expression::parse(predicate).expect("parses");
        evaluate(&parsed, reader).expect_err("refused").message
    }

    #[test]
    fn compares_numbers_text_and_booleans_with_promotion() {
        let reader = MapReader::order();
        assert!(holds("'/order/amount' > 100", &reader));
        assert!(holds("'/order/amount' >= 120.5", &reader));
        assert!(holds("'/order/amount' = 120.5", &reader));
        assert!(holds("'/order/qty' = 3.0", &reader));
        assert!(holds("'/order/qty' < 3.5 and '/order/qty' >= 3", &reader));
        assert!(holds("'/order/id' = \"A1\"", &reader));
        assert!(holds("'/order/id' < \"B\"", &reader));
        assert!(holds("'/order/id' != \"a1\"", &reader));
        assert!(holds("'/order/paid' = false", &reader));
        assert!(holds("'/order/qty' = '/order/lines'", &reader));
        assert!(!holds("'/order/qty' != '/order/lines'", &reader));
        assert!(holds("\"b\" > \"a\"", &reader));
    }

    #[test]
    fn a_missing_path_is_null_and_only_null_tests_and_exists_speak_of_it() {
        let reader = MapReader::order();
        assert!(holds("'/nowhere' = null", &reader));
        assert!(!holds("'/nowhere' != null", &reader));
        assert!(!holds("'/nowhere' = 1", &reader));
        assert!(holds("'/nowhere' != 1", &reader));
        assert!(!holds("'/nowhere' > 1", &reader));
        assert!(!holds("'/nowhere' <= \"z\"", &reader));
        assert!(!holds("exists('/nowhere')", &reader));
        assert!(holds("exists('/order/note')", &reader));
        assert!(holds("'/order/note' = null", &reader));
        assert!(holds("'/order/note' = '/nowhere'", &reader));
        assert!(!holds("starts-with('/nowhere', \"A\")", &reader));
        assert!(!holds("contains('/order/note', \"A\")", &reader));
    }

    #[test]
    fn text_functions_and_the_connectives_hold_together() {
        let reader = MapReader::order();
        assert!(holds("starts-with('/order/id', \"A\")", &reader));
        assert!(!holds("starts-with('/order/id', \"B\")", &reader));
        assert!(holds("contains('/order/customer', \"Nils\")", &reader));
        assert!(holds("not contains('/order/customer', \"Karl\")", &reader));
        assert!(holds(
            "'/order/paid' = true or '/order/amount' > 100",
            &reader
        ));
        assert!(!holds(
            "'/order/paid' = true and '/order/amount' > 100",
            &reader
        ));
        assert!(holds(
            "not ('/order/paid' = true and '/order/amount' > 100)",
            &reader
        ));
        assert!(holds("not not '/order/paid' = false", &reader));
    }

    #[test]
    fn precedence_is_or_over_and_over_not() {
        let reader = MapReader::order();
        // a or (b and c): a holds, so the whole holds even though b does not.
        assert!(holds(
            "'/order/qty' = 3 or '/order/paid' = true and '/order/qty' = 3",
            &reader
        ));
        // (a or b) and c would be false here; the parentheses say so.
        assert!(!holds(
            "('/order/qty' = 3 or '/order/paid' = true) and '/order/paid' = true",
            &reader
        ));
        // (b and c) or a, the same predicate the other way round.
        assert!(holds(
            "'/order/paid' = true and '/order/qty' = 3 or '/order/qty' = 3",
            &reader
        ));
        // not binds to the comparison, not to the conjunction.
        assert!(!holds(
            "not '/order/paid' = false and '/order/qty' = 3",
            &reader
        ));
    }

    #[test]
    fn a_kind_mismatch_is_an_error_naming_both_kinds() {
        let reader = MapReader::order();
        assert_eq!(
            fails("'/order/id' < 5", &reader),
            "predicate: '/order/id' is Text and 5 is Integer: not comparable"
        );
        assert_eq!(
            fails("'/order/paid' = \"false\"", &reader),
            "predicate: '/order/paid' is Bool and \"false\" is Text: not comparable"
        );
        assert_eq!(
            fails("starts-with('/order/qty', \"3\")", &reader),
            "predicate: '/order/qty' is Integer; starts-with() needs Text"
        );
        assert!(fails("contains('/order/paid', \"f\")", &reader).contains("contains()"));
    }

    #[test]
    fn a_reader_that_fails_fails_the_predicate() {
        let reader = MapReader::order();
        assert_eq!(
            fails("'/broken' = 1", &reader),
            "the fixture refuses /broken"
        );
        assert!(evaluate(&Expression::Exists("/broken".into()), &reader).is_err());
    }
}
