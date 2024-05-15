// Copyright 2024 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::fs::File;
use std::path::{Path, PathBuf};

use anyhow::Result;
use hyper::{Body, Response};

use super::context::Context;
use crate::utils::http_utils::{HttpClient, HttpClientTrait};
use crate::utils::process_utils::Command;

pub struct RealContext {
    root_dir: PathBuf,
}

impl RealContext {
    pub fn new() -> Self {
        Self {
            root_dir: PathBuf::from("/"),
        }
    }
}

impl Context for RealContext {
    fn command(&mut self, program: &str) -> Command {
        Command::new(program)
    }
    fn root_dir(&self) -> &Path {
        self.root_dir.as_path()
    }
    fn tempdir(&self, prefix: Option<&str>) -> Result<PathBuf> {
        let mut builder = tempfile::Builder::new();
        Ok(match prefix {
            Some(p) => builder.prefix(p),
            None => &mut builder,
        }
        .tempdir()?
        .into_path())
    }
    fn tempfile_in(&mut self, dir: &Path) -> Result<(File, PathBuf)> {
        let file = tempfile::NamedTempFile::new_in(dir)?;
        Ok(file.keep()?)
    }
    fn http_post(&self, url: String, arg: serde_json::Value) -> Result<Response<Body>> {
        HttpClient::new().post(url, arg)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::system::context::Context;
    use crate::system::real_context::RealContext;

    #[test]
    fn test_real_context_tempdir_success() {
        let context = RealContext::new();
        assert!(context.tempdir(None).unwrap().as_path().exists());
    }

    #[test]
    fn test_real_context_tempdir_success_with_prefix() {
        let context = RealContext::new();
        assert!(context.tempdir(Some("prefix")).unwrap().as_path().exists());
    }

    #[test]
    fn test_real_context_tempdir_in_success() {
        let mut context = RealContext::new();
        let (_, path) = context
            .tempfile_in(&context.tempdir(None).unwrap())
            .unwrap();
        assert!(path.exists());
    }

    #[test]
    fn test_http_post_fail() {
        let context = RealContext::new();
        let result = context.http_post("http://localhost".to_string(), json!({"key":"value"}));

        let err = result.unwrap_err();
        let message = err.root_cause();
        assert_eq!(format!("{}", message), "Connection refused (os error 111)");
    }
}
