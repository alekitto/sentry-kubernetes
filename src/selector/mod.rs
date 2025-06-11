use crate::selector::Operator::{In, NotIn};
use serde::de::{Unexpected, Visitor};
use serde::{de, Deserialize, Deserializer};
use std::fmt::Formatter;

mod parser;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Operator {
    In,
    NotIn,
}

#[derive(Debug)]
pub struct Selectors(Vec<Selector>);

impl TryFrom<&str> for Selectors {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        if value.is_empty() {
            Ok(Self(vec![]))
        } else {
            match parser::root(value) {
                Ok((_, selectors)) => Ok(selectors),
                Err(e) => Err(e.to_owned().into()),
            }
        }
    }
}

impl Selectors {
    #[allow(unused)]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn matches<'a, I: Iterator<Item = (&'a str, &'a str)> + Clone>(&'a self, itr: I) -> bool {
        self.is_empty() || self.0.iter().any(move |s| s.matches(itr.clone()))
    }
}

impl<'de> Deserialize<'de> for Selectors {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct SelectorVisitor;
        impl Visitor<'_> for SelectorVisitor {
            type Value = Selectors;

            fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
                formatter.write_str("valid selector string")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Selectors::try_from(v)
                    .map_err(|_| de::Error::invalid_value(Unexpected::Str(v), &self))
            }
        }

        deserializer.deserialize_newtype_struct("Selectors", SelectorVisitor)
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Selector {
    negate: bool,
    key: String,
    requirement: Option<Restriction>,
}

impl Selector {
    pub fn matches<'a, I: Iterator<Item = (&'a str, &'a str)>>(&self, itr: I) -> bool {
        let mut matches = false;
        for (k, v) in itr {
            if k != self.key {
                continue;
            }

            if self.requirement.as_ref().is_none_or(|r| r.matches(v)) {
                matches = true;
                break;
            }
        }

        if self.negate {
            !matches
        } else {
            matches
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Restriction {
    operator: Operator,
    values: Vec<String>,
}

impl Restriction {
    fn matches(&self, value: &str) -> bool {
        let in_values = self.values.iter().any(|v| v == value);
        match self.operator {
            In => in_values,
            NotIn => !in_values,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::selector::{Operator, Restriction, Selector, Selectors};

    #[test]
    fn match_only_key() {
        let selector = Selector {
            negate: false,
            key: "environment".to_string(),
            requirement: None,
        };

        assert!(selector.matches([("environment", "")].into_iter()));
        assert!(selector.matches([("environment", "test")].into_iter()));
        assert!(selector.matches([("app", "sentry"), ("environment", "test")].into_iter()));
    }

    #[test]
    fn match_negate() {
        let selector = Selector {
            negate: true,
            key: "environment".to_string(),
            requirement: Some(Restriction {
                values: vec!["test".to_string()],
                operator: Operator::In,
            }),
        };

        assert!(selector.matches([("environment", "")].into_iter()));
        assert!(!selector.matches([("environment", "test")].into_iter()));
        assert!(selector.matches([("app", "sentry")].into_iter()));
    }

    #[test]
    fn match_key_value() {
        let selector = Selector {
            negate: false,
            key: "environment".to_string(),
            requirement: Some(Restriction {
                values: vec!["test".to_string()],
                operator: Operator::In,
            }),
        };

        assert!(!selector.matches([("environment", "")].into_iter()));
        assert!(selector.matches([("environment", "test")].into_iter()));
        assert!(selector.matches([("app", "sentry"), ("environment", "test")].into_iter()));
        assert!(!selector.matches([("app", "test"), ("environment", "sentry")].into_iter()));

        let selector = Selector {
            negate: false,
            key: "environment".to_string(),
            requirement: Some(Restriction {
                values: vec!["test".to_string()],
                operator: Operator::NotIn,
            }),
        };

        assert!(selector.matches([("environment", "")].into_iter()));
        assert!(!selector.matches([("environment", "test")].into_iter()));
        assert!(!selector.matches([("app", "sentry"), ("environment", "test")].into_iter()));
        assert!(selector.matches([("app", "test"), ("environment", "sentry")].into_iter()));
    }

    #[test]
    fn from_str_empty() {
        let selector = Selectors::try_from("").expect("parse failed");

        assert!(selector.is_empty());
        assert!(selector.matches([].into_iter()));
    }

    #[test]
    fn from_str_equality_based() {
        let selector = Selectors::try_from("environment=production,tier==frontend,app!=nginx")
            .expect("parse failed");

        assert_eq!(selector.len(), 3);
        assert_eq!(
            selector.0.get(0).unwrap(),
            &Selector {
                negate: false,
                key: "environment".to_string(),
                requirement: Some(Restriction {
                    operator: Operator::In,
                    values: vec!["production".to_string()],
                })
            }
        );
        assert_eq!(
            selector.0.get(1).unwrap(),
            &Selector {
                negate: false,
                key: "tier".to_string(),
                requirement: Some(Restriction {
                    operator: Operator::In,
                    values: vec!["frontend".to_string()],
                })
            }
        );
        assert_eq!(
            selector.0.get(2).unwrap(),
            &Selector {
                negate: false,
                key: "app".to_string(),
                requirement: Some(Restriction {
                    operator: Operator::NotIn,
                    values: vec!["nginx".to_string()],
                })
            }
        );
    }

    #[test]
    fn from_set_based() {
        let selector = Selectors::try_from(
            "environment in (production, qa),tier in (frontend), app notin (nginx,sentry)",
        )
        .expect("parse failed");

        assert_eq!(selector.len(), 3);
        assert_eq!(
            selector.0.get(0).unwrap(),
            &Selector {
                negate: false,
                key: "environment".to_string(),
                requirement: Some(Restriction {
                    operator: Operator::In,
                    values: vec!["production".to_string(), "qa".to_string()],
                })
            }
        );
        assert_eq!(
            selector.0.get(1).unwrap(),
            &Selector {
                negate: false,
                key: "tier".to_string(),
                requirement: Some(Restriction {
                    operator: Operator::In,
                    values: vec!["frontend".to_string()],
                })
            }
        );
        assert_eq!(
            selector.0.get(2).unwrap(),
            &Selector {
                negate: false,
                key: "app".to_string(),
                requirement: Some(Restriction {
                    operator: Operator::NotIn,
                    values: vec!["nginx".to_string(), "sentry".to_string()],
                })
            }
        );
    }
}
