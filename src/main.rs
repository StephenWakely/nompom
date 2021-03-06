use nom::{
    branch::alt,
    bytes::complete::{escaped, tag, take_while, take_while1},
    character::complete::{char, digit1, satisfy},
    combinator::{consumed, cut, map, opt, recognize, value},
    error::{context, ContextError, FromExternalError, ParseError},
    multi::{many1, separated_list0},
    number::complete::double,
    sequence::{preceded, separated_pair, terminated, tuple},
    AsChar, IResult, InputTakeAtPosition,
};
use std::{collections::BTreeMap, num::ParseIntError};

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Bytes(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
    Object(BTreeMap<String, Value>),
    Array(Vec<Value>),
    Null,
}

trait HashParseError<T>: ParseError<T> + ContextError<T> + FromExternalError<T, ParseIntError> {}
impl<T, E: ParseError<T> + ContextError<T> + FromExternalError<T, ParseIntError>> HashParseError<T>
    for E
{
}

fn sp<'a, E: ParseError<&'a str>>(input: &'a str) -> IResult<&'a str, &'a str, E> {
    let chars = " \t\r\n";

    take_while(move |c| chars.contains(c))(input)
}

fn parse_inner_str<'a, E: ParseError<&'a str> + ContextError<&'a str>>(
    delimiter: char,
) -> impl FnMut(&'a str) -> IResult<&'a str, &'a str, E> {
    move |input| {
        map(
            opt(escaped(
                recognize(many1(tuple((
                    take_while1(|c: char| c != '\\' && c != delimiter),
                    // Consume \something
                    opt(tuple((
                        satisfy(|c| c == '\\'),
                        satisfy(|c| c != '\\' && c != delimiter),
                    ))),
                )))),
                '\\',
                satisfy(|c| c == '\\' || c == delimiter),
            )),
            |inner| inner.unwrap_or(""),
        )(input)
    }
}

/// Parses text with a given delimiter.
fn parse_str<'a, E: ParseError<&'a str> + ContextError<&'a str>>(
    delimiter: char,
) -> impl FnMut(&'a str) -> IResult<&'a str, &'a str, E> {
    context(
        "string",
        preceded(
            char(delimiter),
            cut(terminated(parse_inner_str(delimiter), char(delimiter))),
        ),
    )
}

fn parse_boolean<'a, E: ParseError<&'a str>>(input: &'a str) -> IResult<&'a str, bool, E> {
    let parse_true = value(true, tag("true"));
    let parse_false = value(false, tag("false"));

    alt((parse_true, parse_false))(input)
}

fn parse_nil<'a, E: ParseError<&'a str>>(input: &'a str) -> IResult<&'a str, Value, E> {
    value(Value::Null, tag("nil"))(input)
}

fn parse_bytes<'a, E: HashParseError<&'a str>>(input: &'a str) -> IResult<&'a str, String, E> {
    context(
        "bytes",
        map(alt((parse_str('"'), parse_str('\''))), |value| {
            value.to_string()
        }),
    )(input)
}

fn parse_symbol_key<T, E: ParseError<T>>(input: T) -> IResult<T, T, E>
where
    T: std::fmt::Display,
    T: InputTakeAtPosition,
    <T as InputTakeAtPosition>::Item: AsChar,
{
    take_while1(move |item: <T as InputTakeAtPosition>::Item| {
        let c = item.as_char();
        c.is_alphanum() || c == '_'
    })(input)
}

fn parse_colon_key<'a, E: ParseError<&'a str>>(input: &'a str) -> IResult<&'a str, &'a str, E> {
    map(consumed(preceded(char(':'), parse_symbol_key)), |res| res.0)(input)
}

fn parse_key_arrow_hash<'a, E: HashParseError<&'a str>>(
    input: &'a str,
) -> IResult<&'a str, String, E> {
    map(
        alt((parse_str('"'), parse_str('\''), parse_colon_key, digit1)),
        String::from,
    )(input)
}

fn parse_key_colon_hash<'a, E: HashParseError<&'a str>>(
    input: &'a str,
) -> IResult<&'a str, String, E> {
    map(
        alt((parse_str('"'), parse_str('\''), parse_symbol_key, digit1)),
        String::from,
    )(input)
}

fn parse_array<'a, E: HashParseError<&'a str>>(input: &'a str) -> IResult<&'a str, Value, E> {
    context(
        "array",
        map(
            preceded(
                char('['),
                cut(terminated(
                    separated_list0(preceded(sp, char(',')), parse_value),
                    preceded(sp, char(']')),
                )),
            ),
            Value::Array,
        ),
    )(input)
}

fn parse_key_value_arrow<'a, E: HashParseError<&'a str>>(
    input: &'a str,
) -> IResult<&'a str, (String, Value), E> {
    separated_pair(
        preceded(sp, parse_key_arrow_hash),
        cut(preceded(sp, tag("=>"))),
        parse_value,
    )(input)
}

fn parse_hash<'a, E: HashParseError<&'a str>>(input: &'a str) -> IResult<&'a str, Value, E> {
    context(
        "map",
        map(
            preceded(
                char('{'),
                cut(terminated(
                    map(
                        separated_list0(preceded(sp, char(',')), parse_key_value),
                        |tuple_vec| tuple_vec.into_iter().collect(),
                    ),
                    preceded(sp, char('}')),
                )),
            ),
            Value::Object,
        ),
    )(input)
}

fn parse_value<'a, E: HashParseError<&'a str>>(input: &'a str) -> IResult<&'a str, Value, E> {
    preceded(
        sp,
        alt((
            parse_nil,
            parse_hash,
            parse_array,
            map(parse_bytes, Value::Bytes),
            map(double, |value| Value::Float(value)),
            map(parse_boolean, Value::Boolean),
        )),
    )(input)
}

fn parse_key_value_colon<'a, E: HashParseError<&'a str>>(
    input: &'a str,
) -> IResult<&'a str, (String, Value), E> {
    separated_pair(
        preceded(sp, parse_key_colon_hash),
        cut(preceded(sp, tag(":"))),
        parse_value,
    )(input)
}

fn parse_key_value<'a, E: HashParseError<&'a str>>(
    input: &'a str,
) -> IResult<&'a str, (String, Value), E> {
    alt((parse_key_value_colon, parse_key_value_arrow))(input)
}

fn main() {
    let input = r#":foo => "bar""#;
    println!(
        "{:?}",
        parse_key_value(input).map_err(|err| match err {
            nom::Err::Error(err) | nom::Err::Failure(err) => {
                // Create a descriptive error message if possible.
                nom::error::convert_error(input, err)
            }
            _ => err.to_string(),
        })
    );

    let input = r#"foo: "bar""#;
    println!(
        "{:?}",
        parse_key_value(input).map_err(|err| match err {
            nom::Err::Error(err) | nom::Err::Failure(err) => {
                // Create a descriptive error message if possible.
                nom::error::convert_error(input, err)
            }
            _ => err.to_string(),
        })
    );
}
