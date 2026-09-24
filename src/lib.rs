#![forbid(unsafe_code)]

//! The predicate path technology — a technology of `xmip-core-path`.
//!
//! One thing, because this language addresses no content of its own:
//! [`PredicateEngine`], the [`PathEngine`] for the language `predicate`. A
//! predicate is a boolean expression whose operands are paths in the language
//! of whatever [`StructureReader`] the engine is handed — `'/order/amount'`
//! for a JSON Pointer reader, `'order.amount'` for a dot reader; the engine
//! does not know and does not need to, it calls `reader.read(path)` and
//! reasons about the scalars that come back. A route or a Subscription reads
//! one truth from content this way: `'/order/amount' > 100 and
//! starts-with('/order/id', "EU")`.
//!
//! The language: comparisons `=`, `!=`, `<`, `<=`, `>`, `>=` between paths and
//! literals (`"text"`, integers, decimals, `true`, `false`, `null`);
//! `exists('path')`, `starts-with('path', "text")`, `contains('path', "text")`;
//! `and`, `or`, `not` in that order of loosening precedence; parentheses. An
//! integer beside a decimal is promoted to decimal, text compares lexically,
//! two present values of different kinds are an error naming both, and a
//! path the reader has nothing for compares as null. Read yields a
//! [`StructuredValue::Bool`]; write is refused, because a predicate is read,
//! not written.

mod eval;
mod expression;
mod lexer;

pub use expression::{Expression, Operand, Operator};

use contract::{ContractError, StructureReader, StructureWriter, StructuredValue};
use path::{Path, PathCost, PathEngine};

/// The `predicate` engine. It parses the expression and evaluates it through
/// the reader it is given, one `read` per path operand.
pub struct PredicateEngine;

impl PathEngine for PredicateEngine {
    fn language(&self) -> &'static str {
        "predicate"
    }

    fn read(
        &self,
        reader: &dyn StructureReader,
        path: &Path,
    ) -> Result<Option<StructuredValue>, ContractError> {
        let expression = Expression::parse(&path.expression)?;
        eval::evaluate(&expression, reader).map(|truth| Some(StructuredValue::Bool(truth)))
    }

    fn write(
        &self,
        _writer: &mut dyn StructureWriter,
        _path: &Path,
        _value: StructuredValue,
    ) -> Result<(), ContractError> {
        Err(lexer::error("a predicate is read, not written"))
    }

    /// The cost is the reader's, and the engine cannot know which reader it
    /// will be handed; the most expensive answer is the only honest one.
    fn cost(&self, _path: &Path) -> PathCost {
        PathCost::Materialized
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use contract::{ContractDescriptor, ContractId};
    use std::collections::HashMap;

    /// A reader backed by a map of path to value, standing in for any real
    /// structure reader. One path, `/broken`, fails on purpose.
    pub(crate) struct MapReader {
        descriptor: ContractDescriptor,
        values: HashMap<String, StructuredValue>,
    }

    impl MapReader {
        pub(crate) fn order() -> Self {
            let values = [
                ("/order/id", StructuredValue::Text("A1".into())),
                (
                    "/order/customer",
                    StructuredValue::Text("Ilian Nilsson".into()),
                ),
                ("/order/amount", StructuredValue::Decimal(120.5)),
                ("/order/qty", StructuredValue::Integer(3)),
                ("/order/lines", StructuredValue::Integer(3)),
                ("/order/paid", StructuredValue::Bool(false)),
                ("/order/note", StructuredValue::Null),
            ]
            .into_iter()
            .map(|(path, value)| (path.to_string(), value))
            .collect();
            Self {
                descriptor: ContractDescriptor {
                    id: ContractId("fixture".to_string()),
                    version: "1".to_string(),
                    representation: "application/json".to_string(),
                },
                values,
            }
        }
    }

    impl StructureReader for MapReader {
        fn contract(&self) -> &ContractDescriptor {
            &self.descriptor
        }

        fn read(&self, path: &str) -> Result<Option<StructuredValue>, ContractError> {
            if path == "/broken" {
                return Err(ContractError {
                    message: "the fixture refuses /broken".to_string(),
                });
            }
            Ok(self.values.get(path).cloned())
        }
    }

    struct NoWriter(ContractDescriptor);

    impl StructureWriter for NoWriter {
        fn contract(&self) -> &ContractDescriptor {
            &self.0
        }

        fn write(&mut self, _path: &str, _value: StructuredValue) -> Result<(), ContractError> {
            panic!("the engine must refuse before reaching the writer");
        }

        fn finish(self: Box<Self>) -> Result<stream::Stream, ContractError> {
            panic!("never finished");
        }
    }

    #[test]
    fn reads_one_truth_through_the_reader_it_is_given() {
        let reader = MapReader::order();
        let engine = PredicateEngine;
        let read = |text: &str| engine.read(&reader, &Path::new("predicate", text));
        assert_eq!(engine.language(), "predicate");
        assert_eq!(
            read("'/order/amount' > 100 and starts-with('/order/id', \"A\")").expect("reads"),
            Some(StructuredValue::Bool(true))
        );
        assert_eq!(
            read("'/order/paid' = true or '/nowhere' != null").expect("reads"),
            Some(StructuredValue::Bool(false))
        );
        assert_eq!(
            read("'/order/qty' = 3 or '/order/paid' = true and '/order/qty' = 4").expect("reads"),
            Some(StructuredValue::Bool(true))
        );
        let mismatch = read("'/order/id' > 1").expect_err("refused");
        assert_eq!(
            mismatch.message,
            "predicate: '/order/id' is Text and 1 is Integer: not comparable"
        );
        assert!(read("'/order/id' >").is_err());
        assert_eq!(
            engine.cost(&Path::new("predicate", "'/a' = 1")),
            PathCost::Materialized
        );
    }

    #[test]
    fn a_write_is_refused_before_any_writer_is_touched() {
        let mut writer = NoWriter(MapReader::order().descriptor.clone());
        let refused = PredicateEngine
            .write(
                &mut writer,
                &Path::new("predicate", "'/a' = 1"),
                StructuredValue::Bool(true),
            )
            .expect_err("refused");
        assert_eq!(
            refused.message,
            "predicate: a predicate is read, not written"
        );
    }
}
