// Copyright 2024 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use anyhow::Result;
use hyper::{Body, Response};
use lazy_static;

#[cfg(test)]
pub use crate::system::mock_context::MockContext as ContextImpl;
#[cfg(not(test))]
pub use crate::system::real_context::RealContext as ContextImpl;
use crate::utils::process_utils::Command;

/// Mockable system context.
///
/// The object will act differently in the production and testing environment. See MockContext for
/// details for unit test.
///
/// # Examples:
///
/// In production:
/// ```
/// use factory_installer::system::context::{Context, ContextImpl};
/// let mut context = ContextImpl::new();
/// // execute a command
/// context.command("echo").arg("123").output();
/// ```
pub trait Context {
    fn command(&mut self, program: &str) -> Command;
    fn root_dir(&self) -> &Path;
    fn tempdir(&self, prefix: Option<&str>) -> Result<PathBuf>;
    fn tempfile_in(&mut self, dir: &Path) -> Result<(File, PathBuf)>;
    fn http_post(&self, url: String, arg: serde_json::Value) -> Result<Response<Body>>;
}

lazy_static::lazy_static! {
    static ref CONTEXT: Mutex<ContextImpl> = Mutex::new(ContextImpl::new());
}

/// Acquires the global context.
///
/// Note that this function returns a mutex of the context. This means you can only acquire
/// one context in a single thread otherwise there will be a dead lock.
///
/// # Examples:
/// ```
/// use factory_installer::system::context;
/// let mut context1 = context::global_context();
/// // Do something with the context
/// // Must release the lock before acquiring the second context, otherwise will be a dead lock.
/// drop(context1);
/// let mut context2 = context::global_context();
/// ```
pub fn global_context<'a>() -> MutexGuard<'a, ContextImpl> {
    CONTEXT.lock().unwrap()
}
