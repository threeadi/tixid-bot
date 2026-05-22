use anyhow::Result;
use reqwest::{
    header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE, USER_AGENT},
    Client,
};

const APP_VERSION: &str = "1.0.0";
const ORIGIN: &str = "https://app.tix.id";
const UA: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/148.0.0.0 Safari/537.36";

/// Build an HTTP client with the mandatory TIX.ID headers.
/// Pass `token = None` before login; pass `Some(jwt)` afterwards.
pub fn build(token: Option<&str>, device_id: &str) -> Result<Client> {
    let mut headers = HeaderMap::new();
    headers.insert("app_version", HeaderValue::from_static(APP_VERSION));
    headers.insert(
        "device_id",
        HeaderValue::from_str(device_id)
            .map_err(|e| anyhow::anyhow!("Invalid device_id header: {}", e))?,
    );
    headers.insert("lang", HeaderValue::from_static("en"));
    headers.insert("platform", HeaderValue::from_static("web"));
    headers.insert("session_id", HeaderValue::from_static("null"));
    headers.insert(
        "origin",
        HeaderValue::from_static(ORIGIN),
    );
    headers.insert(USER_AGENT, HeaderValue::from_static(UA));
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

    if let Some(t) = token {
        let bearer = format!("Bearer {}", t);
        let mut auth_val = HeaderValue::from_str(&bearer)
            .map_err(|e| anyhow::anyhow!("Invalid auth token: {}", e))?;
        auth_val.set_sensitive(true);
        headers.insert(AUTHORIZATION, auth_val);
    }

    let client = Client::builder()
        .default_headers(headers)
        .build()
        .map_err(|e| anyhow::anyhow!("Failed to build HTTP client: {}", e))?;

    Ok(client)
}
