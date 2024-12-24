use crate::selector::Operator::{In, NotIn};
use crate::selector::{Restriction, Selector, Selectors};
use nom::branch::alt;
use nom::bytes::complete::tag;
use nom::character::complete::{char, multispace0};
use nom::combinator::{map, opt, value};
use nom::error::{Error, ParseError};
use nom::multi::separated_list1;
use nom::sequence::{delimited, pair, preceded, tuple};
use nom::{IResult, Parser};
use nom_regex::lib::regex::Regex;
use nom_regex::str::re_find;

fn sp<'a, O, E: ParseError<&'a str>, F: Parser<&'a str, O, E>>(
    parser: F,
) -> impl Parser<&'a str, O, E> {
    delimited(multispace0, parser, multispace0)
}

pub fn root(input: &str) -> IResult<&str, Selectors> {
    map(
        preceded(
            multispace0,
            separated_list1(sp(char(',')), ExpressionParser),
        ),
        Selectors,
    )(input)
}

fn expr_value<'a>() -> impl Parser<&'a str, String, Error<&'a str>> {
    map(
        sp(re_find(Regex::new("[A-Za-z0-9_.-]+").unwrap())),
        String::from,
    )
}

fn set_based_restriction<'a>() -> impl Parser<&'a str, Restriction, Error<&'a str>> {
    map(
        pair(
            sp(alt((value(In, tag("in")), value(NotIn, tag("notin"))))),
            sp(delimited(
                sp(char('(')),
                separated_list1(sp(char(',')), expr_value()),
                sp(char(')')),
            )),
        ),
        |(operator, values)| Restriction { operator, values },
    )
}

fn exact_match_restriction<'a>() -> impl Parser<&'a str, Restriction, Error<&'a str>> {
    map(
        pair(
            sp(alt((
                value(In, alt((tag("=="), tag("=")))),
                value(NotIn, tag("!=")),
            ))),
            expr_value(),
        ),
        |(operator, value)| Restriction {
            operator,
            values: vec![value],
        },
    )
}

struct ExpressionParser;

impl<'a> Parser<&'a str, Selector, Error<&'a str>> for ExpressionParser {
    fn parse(&mut self, input: &'a str) -> IResult<&'a str, Selector> {
        let dns_regex =
            Regex::new("[a-z0-9]([-a-z0-9]*[a-z0-9])?(/[a-z0-9]([-a-z0-9]*[a-z0-9])?)?").unwrap();
        let mut parser = map(
            tuple((
                sp(opt(tag("!"))),
                sp(re_find(dns_regex)),
                sp(opt(alt((
                    set_based_restriction(),
                    exact_match_restriction(),
                )))),
            )),
            |(not, key, requirement)| Selector {
                negate: not.is_some(),
                key: key.to_string(),
                requirement,
            },
        );

        parser(input)
    }
}
