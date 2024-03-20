// Copyright 2024 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.
use anyhow::Result;
use hyper::{Body, Client, Method, Request, Response};

pub fn post(url: String, arg: serde_json::Value) -> Result<Response<Body>> {
    let req = Request::builder()
        .method(Method::POST)
        .uri(url)
        .header("content-type", "application/json")
        .body(Body::from(arg.to_string()))?;

    let client = Client::new();
    Ok(futures::executor::block_on(client.request(req))?)
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

    #[tokio::test]
    async fn test_post() {
        let result = http_utils::post("http://www.some_url.com".to_string(), json!({}));

        let err = result.unwrap_err();
        let message = err.root_cause();
        assert_eq!(
            format!("{}", message),
            "failed to lookup address information: Name or service not known"
        );
    }
}
