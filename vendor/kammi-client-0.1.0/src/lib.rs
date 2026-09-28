//! Harness-independent client. The server owns all custody and policy decisions.
use hashbrown::HashMap;
use reqwest::Method;
use reqwest::blocking::{Body, Client, Response};
use serde_json::Value;
use std::{error::Error, fs::File, path::Path, time::Duration};

pub type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

pub struct KammiClient {
    http: Client,
    base: String,
    token: String,
}

impl KammiClient {
    fn safe_vault_component(value: &str) -> bool {
        !value.is_empty()
            && value.len() <= 160
            && value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b':' | b'-')
            })
    }

    pub fn new(base: &str, token: &str) -> Result<Self> {
        if !(base.starts_with("http://127.0.0.1:") || base.starts_with("https://")) {
            return Err("use loopback HTTP or authenticated HTTPS".into());
        }
        if memchr::memchr2(b'\r', b'\n', token.as_bytes()).is_some() || token.is_empty() {
            return Err("invalid credential".into());
        }
        Ok(Self {
            http: Client::builder().timeout(Duration::from_secs(30)).build()?,
            base: base.trim_end_matches('/').to_owned(),
            token: token.to_owned(),
        })
    }

    pub fn call(&self, method: Method, path: &str, body: Option<&Value>) -> Result<Value> {
        if !path.starts_with("/v1/") || path.contains("..") {
            return Err("invalid API path".into());
        }
        let mut request = self
            .http
            .request(method, format!("{}{path}", self.base))
            .bearer_auth(&self.token);
        if let Some(value) = body {
            request = request.json(value);
        }
        let response = request.send()?;
        let status = response.status();
        let raw: bytes::Bytes = response.bytes()?;
        if !status.is_success() {
            return Err(format!("HTTP {status}: {}", String::from_utf8_lossy(&raw)).into());
        }
        Ok(serde_json::from_slice(&raw)?)
    }

    pub fn status(&self) -> Result<Value> {
        self.call(Method::GET, "/v1/status", None)
    }
    pub fn memory_record(&self, body: &Value) -> Result<Value> {
        self.call(Method::POST, "/v1/memory", Some(body))
    }
    pub fn memory_search(&self, body: &Value) -> Result<Value> {
        self.call(Method::POST, "/v1/memory/search", Some(body))
    }
    pub fn lease_acquire(&self, body: &Value) -> Result<Value> {
        self.call(Method::POST, "/v1/leases/acquire", Some(body))
    }
    pub fn authorization_request(&self, body: &Value) -> Result<Value> {
        self.call(Method::POST, "/v1/authorize", Some(body))
    }
    pub fn vault_create(&self, body: &Value) -> Result<Value> {
        self.call(Method::POST, "/v1/vaults", Some(body))
    }
    pub fn vault_commit_source(&self, body: &Value) -> Result<Value> {
        self.call(Method::POST, "/v1/vaults/source", Some(body))
    }
    pub fn vault_stage_asset(&self, body: &Value) -> Result<Value> {
        self.call(Method::POST, "/v1/vaults/asset", Some(body))
    }
    pub fn vault_select_generation(&self, body: &Value) -> Result<Value> {
        self.call(Method::POST, "/v1/vaults/generation", Some(body))
    }
    pub fn vault_set_reader(&self, body: &Value) -> Result<Value> {
        self.call(Method::POST, "/v1/vaults/reader", Some(body))
    }
    pub fn vault_select_product_primary(&self, body: &Value) -> Result<Value> {
        self.call(Method::POST, "/v1/vaults/product-primary", Some(body))
    }
    pub fn vault_view(&self, vault_id: &str, actor_id: &str) -> Result<Value> {
        if !Self::safe_vault_component(vault_id) || !Self::safe_vault_component(actor_id) {
            return Err("invalid vault or actor ID".into());
        }
        self.call(
            Method::GET,
            &format!("/v1/vaults/{vault_id}?actor_id={actor_id}"),
            None,
        )
    }
    pub fn vault_source(&self, vault_id: &str, source_id: &str, actor_id: &str) -> Result<Value> {
        if [vault_id, source_id, actor_id]
            .iter()
            .any(|value| !Self::safe_vault_component(value))
        {
            return Err("invalid vault source identity".into());
        }
        self.call(
            Method::GET,
            &format!("/v1/vaults/{vault_id}/sources/{source_id}?actor_id={actor_id}"),
            None,
        )
    }
    pub fn vault_source_bytes(
        &self,
        vault_id: &str,
        source_id: &str,
        actor_id: &str,
    ) -> Result<Response> {
        if [vault_id, source_id, actor_id]
            .iter()
            .any(|value| !Self::safe_vault_component(value))
        {
            return Err("invalid vault source identity".into());
        }
        Ok(self
            .http
            .get(format!(
                "{}/v1/vaults/{vault_id}/sources/{source_id}/bytes",
                self.base
            ))
            .bearer_auth(&self.token)
            .query(&[("actor_id", actor_id)])
            .send()?
            .error_for_status()?)
    }
    pub fn vault_commit_source_file(
        &self,
        path: &Path,
        vault_id: &str,
        source_id: &str,
        base_revision: u64,
        actor_id: &str,
        request_id: &str,
    ) -> Result<Value> {
        if [vault_id, source_id, actor_id, request_id]
            .iter()
            .any(|value| !Self::safe_vault_component(value))
        {
            return Err("invalid vault source identity".into());
        }
        let file = File::open(path)?;
        let size = file.metadata()?.len();
        if size > 16 * 1024 * 1024 {
            return Err("vault source exceeds 16 MiB".into());
        }
        Ok(self
            .http
            .post(format!("{}/v1/vaults/source-stream", self.base))
            .bearer_auth(&self.token)
            .header("x-vault-id", vault_id)
            .header("x-source-id", source_id)
            .header("x-base-revision", base_revision)
            .header("x-actor-id", actor_id)
            .header("x-request-id", request_id)
            .body(Body::sized(file, size))
            .send()?
            .error_for_status()?
            .json()?)
    }
    pub fn vault_upload(
        &self,
        path: &Path,
        vault_id: &str,
        actor_id: &str,
        kind: &str,
        request_id: &str,
    ) -> Result<Value> {
        if [vault_id, actor_id, kind, request_id]
            .iter()
            .any(|value| !Self::safe_vault_component(value))
        {
            return Err("invalid vault upload identity".into());
        }
        let file = File::open(path)?;
        let size = file.metadata()?.len();
        let response = self
            .http
            .post(format!("{}/v1/vaults/asset-stream", self.base))
            .bearer_auth(&self.token)
            .header("x-vault-id", vault_id)
            .header("x-actor-id", actor_id)
            .header("x-kind", kind)
            .header("x-request-id", request_id)
            .body(Body::sized(file, size))
            .send()?
            .error_for_status()?;
        Ok(response.json()?)
    }
    pub fn vault_asset(
        &self,
        vault_id: &str,
        artifact_id: &str,
        actor_id: &str,
    ) -> Result<Response> {
        if [vault_id, artifact_id, actor_id]
            .iter()
            .any(|value| !Self::safe_vault_component(value))
        {
            return Err("invalid vault asset identity".into());
        }
        Ok(self
            .http
            .get(format!(
                "{}/v1/vaults/{vault_id}/assets/{artifact_id}",
                self.base
            ))
            .bearer_auth(&self.token)
            .query(&[("actor_id", actor_id)])
            .send()?
            .error_for_status()?)
    }
    pub fn vault_package(&self, vault_id: &str, actor_id: &str) -> Result<Response> {
        if !Self::safe_vault_component(vault_id) || !Self::safe_vault_component(actor_id) {
            return Err("invalid vault or actor ID".into());
        }
        Ok(self
            .http
            .get(format!("{}/v1/vaults/{vault_id}/package", self.base))
            .bearer_auth(&self.token)
            .query(&[("actor_id", actor_id)])
            .send()?
            .error_for_status()?)
    }
    pub fn vault_import(
        &self,
        package: &Path,
        actor_id: &str,
        package_root: &str,
    ) -> Result<Value> {
        if !Self::safe_vault_component(actor_id) || !Self::safe_vault_component(package_root) {
            return Err("invalid vault import identity".into());
        }
        let file = File::open(package)?;
        let size = file.metadata()?.len();
        Ok(self
            .http
            .post(format!("{}/v1/vaults/package", self.base))
            .bearer_auth(&self.token)
            .header("x-actor-id", actor_id)
            .header("x-package-root", package_root)
            .body(Body::sized(file, size))
            .send()?
            .error_for_status()?
            .json()?)
    }
    pub fn upload(&self, path: &Path, metadata: &HashMap<&str, &str>) -> Result<Value> {
        // Stream the file; no whole-artifact allocation or local policy implementation.
        let file = File::open(path)?;
        let size = file.metadata()?.len();
        let mut request = self
            .http
            .post(format!("{}/v1/artifacts", self.base))
            .bearer_auth(&self.token)
            .body(Body::sized(file, size));
        for (key, value) in metadata {
            request = request.header(*key, *value);
        }
        Ok(request.send()?.error_for_status()?.json()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reject_credential_injection_and_nonlocal_cleartext() {
        assert!(KammiClient::new("http://127.0.0.1:8765", "x\r\ny").is_err());
        assert!(KammiClient::new("http://example.com:8765", "secret").is_err());
        assert!(!KammiClient::safe_vault_component("../../admin"));
        assert!(!KammiClient::safe_vault_component("bad\r\nheader"));
    }
}
