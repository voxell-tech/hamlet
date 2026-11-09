//! SQL functions the schema's columns need that diesel types
//! otherwise.

use diesel::expression::functions::declare_sql_function;
use diesel::sql_types::{BigInt, Nullable};

#[declare_sql_function]
extern "SQL" {
    /// The sum of whole numbers as a whole number, which SQLite's
    /// `SUM` gives and diesel's `sum` would read as a decimal.
    #[aggregate]
    #[sql_name = "SUM"]
    fn total(expr: BigInt) -> Nullable<BigInt>;
}
