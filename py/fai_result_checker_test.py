#!/usr/bin/env python3
# Copyright 2022 The ChromiumOS Authors
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.
"""Tests for fai_result_checker."""

import io
import textwrap
import unittest

import fai_result_checker


class FAIResultTest(unittest.TestCase):
    """Unit test for FAIResult."""

    def setUp(self):
        self.fai_result = fai_result_checker.FAIResult({"a": [{"b": "value"}]})

    def test_get_value_success(self):
        self.assertEqual(self.fai_result["a.0.b"], "value")

    def test_get_value_key_not_exist(self):
        with self.assertRaises(fai_result_checker.ExtractKeyError):
            self.fai_result.__getitem__("a.0.c")

    def test_get_value_list_out_of_index(self):
        with self.assertRaises(fai_result_checker.ExtractKeyError):
            self.fai_result.__getitem__("a.1.b")


class RuleTest(unittest.TestCase):
    """Unit test for Rule."""

    def test_load_rules_success(self):
        raw_rule = textwrap.dedent(
            '''\
            - key: "a.b.c"
              pattern: "123"
              type: "match"'''
        )

        with io.StringIO(raw_rule) as file:
            rules = fai_result_checker.Rule.load_rules(file)

        self.assertEqual(
            rules, [fai_result_checker.Rule("a.b.c", "123", "match")]
        )

    def test_load_rules_invlaid_key(self):
        raw_rule = textwrap.dedent(
            '''\
            - not_key: "a.b.c"
              pattern: "123"
              type: "match"'''
        )

        with self.assertRaises(fai_result_checker.ResultCheckerError):
            with io.StringIO(raw_rule) as file:
                fai_result_checker.Rule.load_rules(file)

    def test_load_rules_invlaid_type(self):
        raw_rule = textwrap.dedent(
            '''\
            - key: "a.b.c"
              pattern: "123"
              type: "invalid_type"'''
        )

        with self.assertRaises(fai_result_checker.ResultCheckerError):
            with io.StringIO(raw_rule) as file:
                fai_result_checker.Rule.load_rules(file)

    def test_load_rules_not_a_list(self):
        raw_rule = textwrap.dedent(
            '''\
            key: "a.b.c"
            pattern: "123"
            type: "invalid_type"'''
        )

        with self.assertRaises(fai_result_checker.ResultCheckerError):
            with io.StringIO(raw_rule) as file:
                fai_result_checker.Rule.load_rules(file)

    def test_validate_match(self):
        fai_result = fai_result_checker.FAIResult({"key": "123"})
        self.assertIsNone(
            fai_result_checker.Rule("key", "123", "match").validate(fai_result)
        )

    def test_validate_match_replace_keys(self):
        fai_result = fai_result_checker.FAIResult(
            {"key1": "123", "key2": "123"}
        )
        self.assertIsNone(
            fai_result_checker.Rule("key1", "${key2}", "match").validate(
                fai_result
            )
        )

    def test_validate_not_match(self):
        fai_result = fai_result_checker.FAIResult({"key": "456"})
        self.assertIsInstance(
            fai_result_checker.Rule("key", "123", "match").validate(fai_result),
            fai_result_checker.MismatchError,
        )

    def test_validate_include(self):
        fai_result = fai_result_checker.FAIResult({"key": ["123"]})
        self.assertIsNone(
            fai_result_checker.Rule("key", "123", "include").validate(
                fai_result
            )
        )

    def test_validate_not_include(self):
        fai_result = fai_result_checker.FAIResult({"key": ["456"]})
        self.assertIsInstance(
            fai_result_checker.Rule("key", "123", "include").validate(
                fai_result
            ),
            fai_result_checker.MismatchError,
        )

    def test_validate_exclude(self):
        fai_result = fai_result_checker.FAIResult({"key": []})
        self.assertIsNone(
            fai_result_checker.Rule("key", "123", "exclude").validate(
                fai_result
            )
        )

    def test_validate_not_exclude(self):
        fai_result = fai_result_checker.FAIResult({"key": ["123"]})
        self.assertIsInstance(
            fai_result_checker.Rule("key", "123", "exclude").validate(
                fai_result
            ),
            fai_result_checker.MismatchError,
        )

    def test_validate_key_error(self):
        fai_result = fai_result_checker.FAIResult({})
        self.assertIsInstance(
            fai_result_checker.Rule("key", "123", "match").validate(fai_result),
            fai_result_checker.KeyNotFoundError,
        )


if __name__ == "__main__":
    unittest.main()
