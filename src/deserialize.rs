use serde::{Deserialize, Deserializer};
use std::fmt::Display;
use std::str::FromStr;

/// Deserializing function from `String` to numeric which can be any integer,
/// or floating-point number.
///
/// # Also see
/// Look at example at https://serde.rs/stream-array.html
pub fn de_string_to_number<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: FromStr + Deserialize<'de>,
    <T as FromStr>::Err: Display // std::str::FromStr has `Err` type, see https://doc.rust-lang.org/std/str/trait.FromStr.html
{
    let buf = String::deserialize(deserializer)?;
    // convert into serde's custom Error type
    buf.parse::<T>().map_err(serde::de::Error::custom)
}
