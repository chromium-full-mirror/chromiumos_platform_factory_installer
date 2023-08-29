// Copyright 2022 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::default::Default;

use anyhow::Result;
use regex::Regex;
use serde::{self, Deserialize, Serialize};
use serde_json::{self, Map, Value};

use crate::utils::string_utils;

pub trait Parser {
    fn parse(&self, input: String) -> Result<Value>;
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum DataParser {
    DefaultParser(DefaultParser),
    DictParser(DictParser),
    RegexParser(RegexParser),
}

impl Parser for DataParser {
    fn parse(&self, input: String) -> Result<Value> {
        match self {
            DataParser::DefaultParser(parser) => parser.parse(input),
            DataParser::DictParser(parser) => parser.parse(input),
            DataParser::RegexParser(parser) => parser.parse(input),
        }
    }
}

impl Default for DataParser {
    fn default() -> Self {
        DataParser::DefaultParser(DefaultParser {})
    }
}

/// A Parser to convert raw `String` to `serde_json::Value`.
#[derive(Serialize, Deserialize)]
pub struct DefaultParser {}

impl Parser for DefaultParser {
    fn parse(&self, input: String) -> Result<Value> {
        Ok(Value::String(input.trim().to_string()))
    }
}

/// A Parser to parse key/value pairs from the input string.
///
/// # Example
/// ```
/// use factory_installer::factory_fai::parsers::{Parser, DictParser};
///
/// let parser = DictParser::new(":".to_string()).comment_mark("//".to_string());
/// let input = "key1: value1 // comment1".to_string();
///
/// let output = parser.parse(input).unwrap();
///
/// assert_eq!(output.get("key1").unwrap(), "value1");
/// ```
#[derive(Serialize, Deserialize)]
pub struct DictParser {
    delimiter: String,
    comment_mark: Option<String>,
}

impl DictParser {
    pub fn new(delimiter: String) -> Self {
        Self {
            delimiter: delimiter,
            comment_mark: None,
        }
    }
    pub fn comment_mark(mut self, comment_mark: String) -> Self {
        self.comment_mark = Some(comment_mark);
        self
    }
}

impl Parser for DictParser {
    fn parse(&self, input: String) -> Result<Value> {
        let lines = input.split("\n");
        let lines: Vec<&str> = match &self.comment_mark {
            Some(mark) => lines.map(|line| line.split(mark).nth(0).unwrap()).collect(),
            None => lines.collect(),
        };
        let output = string_utils::parse_dict_ignore_error(lines, &self.delimiter);
        Ok(serde_json::json!(output))
    }
}

/// A Parser to search certain pattern from the input string.
///
/// # Example
/// Non-capture usage:
/// ```
/// use factory_installer::factory_fai::parsers::{Parser, RegexParser};
///
/// let parser = RegexParser::new(r"key1".to_string());
/// let input = "key1 some text".to_string();
///
/// let output = parser.parse(input).unwrap();
///
/// assert_eq!(output, "key1");
/// ```
///
/// Capture usage:
/// ```
/// use factory_installer::factory_fai::parsers::{Parser, RegexParser};
///
/// let parser = RegexParser::new(r"(?P<key1>value1)".to_string());
/// let input = "value1 some text".to_string();
///
/// let output = parser.parse(input).unwrap();
///
/// assert_eq!(output.get("key1").unwrap(), "value1");
/// ```
#[derive(Serialize, Deserialize)]
pub struct RegexParser {
    pattern: String,
}

impl RegexParser {
    pub fn new(pattern: String) -> Self {
        Self { pattern: pattern }
    }

    fn parse_captures(&self, re: &Regex, input: &String) -> Result<Value> {
        let cap_names: Vec<&str> = re.capture_names().flatten().collect();
        let mut matches = Map::new();
        for caps in re.captures_iter(&input) {
            for key in cap_names.iter() {
                if let Some(value) = caps.name(key) {
                    matches.insert(key.to_string(), Value::String(value.as_str().to_string()));
                }
            }
        }
        Ok(Value::Object(matches))
    }

    fn parse_non_captures(&self, re: &Regex, input: &String) -> Result<Value> {
        if let Some(m) = re.find(&input) {
            Ok(Value::String(m.as_str().to_string()))
        } else {
            Err(anyhow::anyhow!("Pattern not found: {}", self.pattern))
        }
    }
}

impl Parser for RegexParser {
    fn parse(&self, input: String) -> Result<Value> {
        let re = Regex::new(&self.pattern)?;
        if re.capture_names().flatten().count() > 0 {
            self.parse_captures(&re, &input)
        } else {
            self.parse_non_captures(&re, &input)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::factory_fai::parsers::{DictParser, Parser, RegexParser};

    #[test]
    fn test_dict_parser() {
        let parser = DictParser::new(":".to_string());
        let input = r#"
        key1: value1
        key2: value2"#
            .to_string();

        let output = parser.parse(input).unwrap();

        assert_eq!(output.get("key1").unwrap(), "value1");
        assert_eq!(output.get("key2").unwrap(), "value2");
    }

    #[test]
    fn test_dict_parser_with_comment() {
        let parser = DictParser::new(":".to_string()).comment_mark("//".to_string());
        let input = r#"
        key1: value1 // comment1
        key2: value2"#
            .to_string();

        let output = parser.parse(input).unwrap();

        assert_eq!(output.get("key1").unwrap(), "value1");
        assert_eq!(output.get("key2").unwrap(), "value2");
    }

    #[test]
    fn test_regex_parser() {
        let parser = RegexParser::new(r"key1".to_string());
        let input = "key1 some text".to_string();

        let output = parser.parse(input).unwrap();

        assert_eq!(output, "key1");
    }

    #[test]
    fn test_regex_parser_pattern_not_found() {
        let parser = RegexParser::new(r"key3".to_string());
        let input = "key1 some text key2".to_string();

        let output = parser.parse(input);

        assert!(output.is_err());
    }

    #[test]
    fn test_regex_parser_captures() {
        let parser = RegexParser::new(r"(?P<key1>value1)|(?P<key2>value2)".to_string());
        let input = "value1 some text value2\n not a value".to_string();

        let output = parser.parse(input).unwrap();

        assert_eq!(output.get("key1").unwrap(), "value1");
        assert_eq!(output.get("key2").unwrap(), "value2");
    }
}
