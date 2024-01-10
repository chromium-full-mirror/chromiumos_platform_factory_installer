// Copyright 2022 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::error::Error;
use std::fmt::{Debug, Display, Formatter, Result};

#[derive(Debug)]
pub struct NotImplementedError;

impl Error for NotImplementedError {}

impl Display for NotImplementedError {
    fn fmt(&self, f: &mut Formatter) -> Result {
        write!(
            f,
            "The function is not implemented and should not be called."
        )
    }
}

#[derive(Debug)]
pub enum RegexMatchError {
    NamedGroupNotFoundError,
}

impl Error for RegexMatchError {}

impl Display for RegexMatchError {
    fn fmt(&self, f: &mut Formatter) -> Result {
        match self {
            RegexMatchError::NamedGroupNotFoundError => write!(f, "Named group is not found!"),
        }
    }
}
