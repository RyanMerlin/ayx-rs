//! Trusted Alteryx One endpoint parsing.
//!
//! Authentication endpoints are a trust boundary: accepting an arbitrary URL
//! here could send cookies, authorization codes, or client credentials to a
//! hostile host.  Keep the production constructor deliberately narrow.  Local
//! HTTP servers must opt in through [`OneEndpoint::for_test_localhost`].

use std::fmt;

use thiserror::Error;
use url::Url;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum OneEndpointError {
    #[error("Alteryx One endpoint is not a valid URL")]
    InvalidUrl,
    #[error("Alteryx One endpoint must use HTTPS")]
    InsecureScheme,
    #[error("Alteryx One endpoint must not include credentials")]
    Credentials,
    #[error("Alteryx One endpoint must not include a query string or fragment")]
    QueryOrFragment,
    #[error("Alteryx One endpoint host is not an approved Alteryx regional or Ping host")]
    UntrustedHost,
}

/// A parsed endpoint permitted to receive Alteryx One authentication traffic.
#[derive(Clone, PartialEq, Eq)]
pub struct OneEndpoint(Url);

impl OneEndpoint {
    /// Parses a production endpoint.  Only HTTPS Alteryx Cloud regional and
    /// Ping hosts are accepted, and URL credentials/query/fragment components
    /// are forbidden.
    pub fn parse(value: &str) -> Result<Self, OneEndpointError> {
        let url = Url::parse(value.trim()).map_err(|_| OneEndpointError::InvalidUrl)?;
        Self::validate(&url, false)?;
        Ok(Self(url))
    }

    /// Constructs an endpoint for an explicit local test fixture.  This is
    /// intentionally named so it cannot be mistaken for a production parser.
    /// It accepts only loopback HTTP(S) URLs and has the same credential and
    /// query/fragment restrictions as [`Self::parse`].
    pub fn for_test_localhost(value: &str) -> Result<Self, OneEndpointError> {
        let url = Url::parse(value.trim()).map_err(|_| OneEndpointError::InvalidUrl)?;
        Self::validate(&url, true)?;
        Ok(Self(url))
    }

    fn validate(url: &Url, allow_localhost: bool) -> Result<(), OneEndpointError> {
        if !url.username().is_empty() || url.password().is_some() {
            return Err(OneEndpointError::Credentials);
        }
        if url.query().is_some() || url.fragment().is_some() {
            return Err(OneEndpointError::QueryOrFragment);
        }
        let host = url.host_str().ok_or(OneEndpointError::UntrustedHost)?;
        let local = matches!(host, "localhost" | "127.0.0.1" | "::1");
        if allow_localhost && local {
            if matches!(url.scheme(), "http" | "https") {
                return Ok(());
            }
            return Err(OneEndpointError::InsecureScheme);
        }
        if url.scheme() != "https" {
            return Err(OneEndpointError::InsecureScheme);
        }
        // Regional cells are named like `us1`/`eu1`; Ping issuers are
        // `pingauth` or `pingauth-<region>-<cell>`.  A label boundary prevents
        // lookalikes such as alteryxcloud.com.evil.example, while rejecting
        // the bare registrable domain and unrelated Cloud subdomains.
        let Some(label) = host.strip_suffix(".alteryxcloud.com") else {
            return Err(OneEndpointError::UntrustedHost);
        };
        let regional = !label.is_empty()
            && label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            && label
                .bytes()
                .last()
                .is_some_and(|byte| byte.is_ascii_digit());
        if label == "pingauth" || label.starts_with("pingauth-") || regional {
            Ok(())
        } else {
            Err(OneEndpointError::UntrustedHost)
        }
    }

    pub fn as_url(&self) -> &Url {
        &self.0
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    pub fn into_string(self) -> String {
        self.0.into()
    }
}

impl fmt::Debug for OneEndpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("OneEndpoint")
            .field(&self.0.as_str())
            .finish()
    }
}

impl fmt::Display for OneEndpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_endpoint_accepts_regional_and_ping_hosts() {
        assert!(OneEndpoint::parse("https://us1.alteryxcloud.com").is_ok());
        assert!(OneEndpoint::parse("https://pingauth-us1-4.alteryxcloud.com/as/token").is_ok());
    }

    #[test]
    fn production_endpoint_rejects_unsafe_urls() {
        for value in [
            "http://us1.alteryxcloud.com",
            "https://evil.example",
            "https://alteryxcloud.com",
            "https://unrelated.alteryxcloud.com",
            "https://alteryxcloud.com.evil.example",
            "https://user:pass@us1.alteryxcloud.com",
            "https://us1.alteryxcloud.com/?next=x",
            "https://us1.alteryxcloud.com/#part",
        ] {
            assert!(OneEndpoint::parse(value).is_err(), "{value}");
        }
    }

    #[test]
    fn localhost_requires_explicit_test_constructor() {
        assert!(OneEndpoint::parse("http://127.0.0.1:8080").is_err());
        assert!(OneEndpoint::for_test_localhost("http://127.0.0.1:8080").is_ok());
    }

    #[test]
    fn production_rejections_report_the_boundary_that_failed() {
        assert_eq!(
            OneEndpoint::parse("http://us1.alteryxcloud.com").unwrap_err(),
            OneEndpointError::InsecureScheme
        );
        assert_eq!(
            OneEndpoint::parse("https://user:secret@us1.alteryxcloud.com").unwrap_err(),
            OneEndpointError::Credentials
        );
        assert_eq!(
            OneEndpoint::parse("https://us1.alteryxcloud.com/?next=/login").unwrap_err(),
            OneEndpointError::QueryOrFragment
        );
        assert_eq!(
            OneEndpoint::parse("https://us1.alteryxcloud.com/#callback").unwrap_err(),
            OneEndpointError::QueryOrFragment
        );
    }

    #[test]
    fn test_constructor_keeps_non_loopback_hosts_untrusted() {
        for value in [
            "http://192.0.2.10:8080",
            "http://localhost.evil.example:8080",
            "ftp://127.0.0.1:8080",
            "http://127.0.0.1:8080/?fixture=true",
        ] {
            assert!(
                OneEndpoint::for_test_localhost(value).is_err(),
                "test endpoint unexpectedly accepted: {value}"
            );
        }
    }
}
