// Copyright 2022 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::fmt::Debug;

use thiserror::Error;

/// For generic error messages.
#[derive(Error, Debug)]
pub enum Error {
    #[error("The function is not implemented and should not be called.")]
    NotImplementedError,
}

#[derive(Error, Debug)]
pub enum RegexMatchError {
    #[error("Named group is not found!")]
    NamedGroupNotFoundError,
}
