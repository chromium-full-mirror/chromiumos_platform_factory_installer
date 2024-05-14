// Copyright 2024 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.
use anyhow::Result;
use hyper::{Body, Client, Method, Request, Response};
use tokio::runtime::Runtime;

pub fn post(url: String, arg: serde_json::Value) -> Result<Response<Body>> {
    let rt = Runtime::new()?;
    let req = Request::builder()
        .method(Method::POST)
        .uri(url)
        .header("content-type", "application/json")
        .body(Body::from(arg.to_string()))?;

    let client = Client::new();
    let res = futures::executor::block_on(async move {
        rt.spawn(async move { client.request(req).await }).await
    })?;
    Ok(res?)
}

#[cfg(test)]
pub mod mock {
    use anyhow::Result;

    pub fn post(_: String, _: serde_json::Value) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::utils::http_utils;

    #[test]
    fn test_post_fail() {
        let result = http_utils::post("http://localhost".to_string(), json!({"key":"value"}));

        let err = result.unwrap_err();
        let message = err.root_cause();
        assert_eq!(format!("{}", message), "Connection refused (os error 111)");
    }
}
