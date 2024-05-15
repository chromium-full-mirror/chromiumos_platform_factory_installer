// Copyright 2024 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.
use anyhow::Result;
use hyper::{Body, Client, Method, Request, Response};
#[cfg(test)]
use mockall::automock;
use tokio::runtime::Runtime;

#[cfg_attr(test, automock)]
pub trait HttpClientTrait {
    fn post(&self, url: String, arg: serde_json::Value) -> Result<Response<Body>>;
}

pub struct HttpClient {}
impl HttpClient {
    pub fn new() -> Self {
        Self {}
    }
}
impl HttpClientTrait for HttpClient {
    fn post(&self, url: String, arg: serde_json::Value) -> Result<Response<Body>> {
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
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::utils::http_utils::{self, HttpClientTrait};

    #[test]
    fn test_post_fail() {
        let client = http_utils::HttpClient::new();
        let result = client.post("http://localhost".to_string(), json!({"key":"value"}));

        let err = result.unwrap_err();
        let message = err.root_cause();
        assert_eq!(format!("{}", message), "Connection refused (os error 111)");
    }
}
