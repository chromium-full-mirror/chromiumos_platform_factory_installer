// Copyright 2022 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::collections::HashMap;

use anyhow::{self, Result};

fn parse_dict_impl<D, L, I>(
    lines: I,
    delimiter: D,
    ignore_error: bool,
) -> Result<HashMap<String, String>>
where
    D: AsRef<str>,
    L: AsRef<str>,
    I: IntoIterator<Item = L>,
{
    let delimiter = delimiter.as_ref();
    let mut ret = HashMap::new();
    for line in lines.into_iter() {
        let line = line.as_ref();
        let split: Vec<&str> = line.split(delimiter).collect();
        if split.len() != 2 {
            if ignore_error {
                continue;
            } else {
                return Err(anyhow::anyhow!("Failed to parse line: {}", line));
            }
        }
        ret.insert(split[0].trim().to_string(), split[1].trim().to_string());
    }
    Ok(ret)
}

/// A function to parse a list of key/value pair string.
///
/// # Arguments
/// * `lines` - A list of string to be parsed.
/// * `delimiter` - The delimiter between key and value.
///
/// # Return
/// `Result<HashMap<String, String>>` which maps the keys to values.
///
/// # Examples
/// ```
/// use factory_installer::utils::string_utils;
///
/// let parse_result = string_utils::parse_dict(["key:value"], ":").unwrap();
/// let value = parse_result.get("key").unwrap(); // will return "value"
/// ```
pub fn parse_dict<D, L, I>(lines: I, delimiter: D) -> Result<HashMap<String, String>>
where
    D: AsRef<str>,
    L: AsRef<str>,
    I: IntoIterator<Item = L>,
{
    parse_dict_impl(lines, delimiter, false)
}

/// A function to parse a list of key/value pair string, but ignore error when failed to parse any
/// line.
///
/// # Arguments
/// * `lines` - A list of string to be parsed.
/// * `delimiter` - The delimiter between key and value.
///
/// # Return
/// `HashMap<String, String>` which maps the keys to values.
///
/// # Examples
/// ```
/// use factory_installer::utils::string_utils;
///
/// let parse_result = string_utils::parse_dict_ignore_error(["key:value"], ":");
/// let value = parse_result.get("key").unwrap(); // will return "value"
/// ```
pub fn parse_dict_ignore_error<D, L, I>(lines: I, delimiter: D) -> HashMap<String, String>
where
    D: AsRef<str>,
    L: AsRef<str>,
    I: IntoIterator<Item = L>,
{
    parse_dict_impl(lines, delimiter, true).unwrap()
}

/// Transform a hex string to an u32 integer.
///
/// # Arguments
/// * `hex_str` - The hex string to be transformed.
///
/// # Return
/// An u32 integer.
///
/// # Examples
/// ```
/// use factory_installer::utils::string_utils;
///
/// let hex = string_utils::hex_to_u32("0x1234").unwrap();
/// ```
pub fn hex_to_u32<S>(hex_str: S) -> Result<u32>
where
    S: AsRef<str>,
{
    Ok(u32::from_str_radix(
        hex_str.as_ref().trim().trim_start_matches("0x"),
        16,
    )?)
}

/// Transform an u32 integer to a lowercase hex string.
///
/// # Arguments
/// * `num` - The u32 integer to be transformed.
///
/// # Return
/// A lowercase hex string which begins with "0x".
///
/// # Examples
/// ```
/// use factory_installer::utils::string_utils;
///
/// let hex = string_utils::u32_to_hex(4660);
/// ```
pub fn u32_to_hex(num: u32) -> String {
    format!("0x{:x}", num)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::utils::string_utils;

    #[test]
    fn test_parse_dict_success() {
        let mut expected = HashMap::new();
        expected.insert("key1".to_string(), "value1".to_string());
        expected.insert("key2".to_string(), "value2".to_string());

        let parse_result = string_utils::parse_dict(["key1:value1", "key2:value2"], ":").unwrap();

        assert_eq!(expected, parse_result);
    }

    #[test]
    fn test_parse_dict_success_duplicated_keys() {
        let mut expected = HashMap::new();
        expected.insert("key1".to_string(), "value2".to_string());

        let parse_result = string_utils::parse_dict(["key1:value1", "key1:value2"], ":").unwrap();

        assert_eq!(expected, parse_result);
    }

    #[test]
    fn test_parse_dict_success_trim_whitespaces() {
        let mut expected = HashMap::new();
        expected.insert("key1".to_string(), "value1".to_string());
        expected.insert("key2".to_string(), "value2".to_string());

        let parse_result =
            string_utils::parse_dict([" key1 : value1", "\tkey2\t:\tvalue2\t"], ":").unwrap();

        assert_eq!(expected, parse_result);
    }

    #[test]
    fn test_parse_dict_failed_invalid_input() {
        let parse_result = string_utils::parse_dict(["key1=value1"], ":");

        assert!(parse_result.is_err());
    }

    #[test]
    fn test_parse_dict_ignore_error_success() {
        let mut expected = HashMap::new();
        expected.insert("key1".to_string(), "value1".to_string());
        expected.insert("key2".to_string(), "value2".to_string());

        let parse_result = string_utils::parse_dict_ignore_error(
            ["key1:value1", "cannot_be_parsed", "key2:value2"],
            ":",
        );

        assert_eq!(expected, parse_result);
    }

    #[test]
    fn test_hex_to_u32() {
        let ret = string_utils::hex_to_u32("0x1234").unwrap();
        assert_eq!(4660, ret);
    }

    #[test]
    fn test_u32_to_hex() {
        let ret = string_utils::u32_to_hex(4660);
        assert_eq!("0x1234", ret);
    }
}
