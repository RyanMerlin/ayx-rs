//! Reqwest adapter for the platform-neutral authentication transport seam.

use anyhow::{Context, Result};
use ayx_core::auth::{HttpRequest, HttpResponse, HttpTransport};
use reqwest::blocking::Client;

#[derive(Debug, Clone)]
pub struct ReqwestBlockingTransport {
    client: Client,
}

impl ReqwestBlockingTransport {
    pub fn new(client: Client) -> Self {
        Self { client }
    }
}

impl Default for ReqwestBlockingTransport {
    fn default() -> Self {
        Self {
            client: Client::new(),
        }
    }
}

impl HttpTransport for ReqwestBlockingTransport {
    type Error = anyhow::Error;

    fn send(&mut self, request: HttpRequest) -> Result<HttpResponse> {
        let method = reqwest::Method::from_bytes(request.method.as_bytes())
            .context("invalid authentication HTTP method")?;
        let mut builder = self.client.request(method, &request.url);
        for (name, value) in request.headers {
            builder = builder.header(name, value);
        }
        if let Some(body) = request.body {
            builder = builder.body(body);
        }
        let response = builder
            .send()
            .context("authentication HTTP request failed")?;
        let status = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .filter_map(|(name, value)| Some((name.to_string(), value.to_str().ok()?.to_string())))
            .collect();
        let body = response
            .bytes()
            .context("failed to read authentication HTTP response")?
            .to_vec();
        Ok(HttpResponse {
            status,
            headers,
            body,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adapter_constructs_without_exposing_request_secrets() {
        let request = HttpRequest {
            method: "GET".to_string(),
            url: "https://example.invalid".to_string(),
            headers: Default::default(),
            body: None,
        };
        let encoded = serde_json::to_string(&request).unwrap();
        assert!(!encoded.contains("access_token"));
        let _transport = ReqwestBlockingTransport::default();
    }
}
