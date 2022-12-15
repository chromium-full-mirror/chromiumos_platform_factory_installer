#!/usr/bin/env python3
# Copyright 2022 The ChromiumOS Authors
# Use of this source code is governed by a BSD-style license that can be
# found in the LICENSE file.
"""ChromeOS Factory FAI result checker."""

import argparse
import enum
import json
import logging
import os
import re
import sys
from typing import IO, List, Mapping, Optional, Sequence

import yaml  # pylint: disable=import-error


DEFAULT_RULE_PATH = os.path.join(
    os.path.abspath(os.path.dirname(__file__)), "../py/default_rules.yaml"
)


class ResultCheckerError(Exception):
    """Raised when the result checker goes wrong."""


class ExtractKeyError(ResultCheckerError):
    """Raised when failed to extract the key from the FAI result."""


class KeyNotFoundError(ResultCheckerError):
    """Raised when the key isn't found in the FAI result."""


class MismatchError(ResultCheckerError):
    """Raised when the value isn't match in the FAI result."""


class FAIResult:
    """Wrapper class for FAI result."""

    def __init__(self, raw_result: Mapping):
        self._raw_result = raw_result

    def __getitem__(self, key: str):
        """Recrsively extract the value from FAI Result.

        Examples:
          result = FAIResult(...)
          # result["a.b.c"] is equivalent to result._raw_result["a"]["b"]["c"]

        Raises:
          ExtractKeyError when failed to extract the key from the result.
        """
        sub_keys = key.split(".")
        result = self._raw_result
        for sub_key in sub_keys:
            try:
                if isinstance(result, Mapping):
                    result = result[sub_key]
                elif isinstance(result, Sequence):
                    result = result[int(sub_key)]
            except (KeyError, ValueError, IndexError) as ex:
                raise ExtractKeyError from ex
        return result


class RuleType(str, enum.Enum):
    """List of valid rule types."""

    match = "match"
    include = "include"
    exclude = "exclude"

    @classmethod
    def has_value(cls, value: str) -> bool:
        return value in cls.__members__.values()

    def __str__(self) -> str:
        return str(self.value)


class Rule:
    """Rule to validate FAI result."""

    def __init__(self, key: str, pattern: str, rule_type: RuleType):
        if not RuleType.has_value(rule_type):
            raise KeyError(f"{rule_type} is not a valid type.")
        self._key = key
        self._pattern = pattern
        self._rule_type = rule_type

    @property
    def key(self):
        return self._key

    @property
    def pattern(self):
        return self._pattern

    @property
    def rule_type(self):
        return self._rule_type

    def validate(self, fai_result: FAIResult) -> Optional[ResultCheckerError]:
        """Validates the given FAI result."""
        try:
            value = fai_result[self.key]
        except ExtractKeyError:
            return KeyNotFoundError(
                f"{self.key!r} does not exist in FAI result."
            )

        pattern = self.pattern
        replace_keys = re.findall(r"\${([^}]*)}", pattern)
        for key in replace_keys:
            try:
                pattern = pattern.replace(f"${{{key}}}", fai_result[key])
            except ExtractKeyError:
                return KeyNotFoundError(
                    f"Failed to evaluate {self._pattern!r}: "
                    f"{key!r} does not exist in FAI result."
                )

        if self.rule_type == RuleType.match:
            if not re.match(pattern, value):
                return MismatchError(
                    f"{self.key!r} should match {self.pattern!r}."
                )
        elif self.rule_type == RuleType.include:
            if pattern not in value:
                return MismatchError(
                    f"{self.key!r} should include {self.pattern!r}."
                )
        elif self.rule_type == RuleType.exclude:
            if pattern in value:
                return MismatchError(
                    f"{self.key!r} should not include {self.pattern!r}."
                )
        else:
            # Should not reach this line.
            raise ResultCheckerError(
                f"Unexpected rule type: {self.rule_type!r}"
            )
        return None

    @classmethod
    def from_dict(cls, rule: Mapping[str, str]):
        try:
            return cls(rule["key"], rule["pattern"], rule["type"])
        except KeyError as ex:
            raise ResultCheckerError(f"Invalid rule: {rule}") from ex

    def __eq__(self, rhs) -> bool:
        return (
            self.key == rhs.key
            and self.pattern == rhs.pattern
            and self.rule_type == rhs.rule_type
        )

    @classmethod
    def load_rules(cls, rule_file: IO) -> Sequence:
        raw_rule = yaml.safe_load(rule_file)
        rules = []
        if not isinstance(raw_rule, Sequence):
            raise ResultCheckerError("Rule set should be a list.")
        for rule in raw_rule:
            rules.append(cls.from_dict(rule))
        return rules


def main(argv: Optional[List[str]] = None) -> Optional[int]:
    parser = argparse.ArgumentParser()
    parser.add_argument("input_file", help="Factory FAI result file.")
    parser.add_argument(
        "-r",
        "--rule-files",
        nargs="+",
        default=[DEFAULT_RULE_PATH],
        help="Rules to validate the result.",
    )
    args = parser.parse_args(argv)

    with open(args.input_file, "r", encoding="utf-8") as file:
        fai_result = FAIResult(json.load(file))

    rules = []
    for rule_file in args.rule_files:
        with open(rule_file, "r", encoding="utf-8") as file:
            rules += Rule.load_rules(file)

    validate_errors = []
    for rule in rules:
        error = rule.validate(fai_result)
        if error:
            validate_errors.append(repr(error))

    if validate_errors:
        logging.error(
            "Invalid FAI result found:\n\t%s", "\n\t".join(validate_errors)
        )
        return 1
    else:
        logging.info("FAI result %s is valid.", args.input_file)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
