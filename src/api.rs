use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Serialize, de::DeserializeOwned};

use crate::models::*;

/// All API endpoints use the same gateway.
const BASE: &str = "https://api-b2b.tix.id";

// ── internal helpers ─────────────────────────────────────────────────────────

/// Reads response body, logs it, checks `success`, deserialises `data`.
async fn parse_api<T: DeserializeOwned>(resp: reqwest::Response, ctx: &str) -> Result<T> {
    let status = resp.status().as_u16();
    let text = resp
        .text()
        .await
        .with_context(|| format!("{ctx}: failed to read body"))?;

    tracing::debug!(endpoint = ctx, status, response = %text, "← response");

    let json: serde_json::Value =
        serde_json::from_str(&text).with_context(|| format!("{ctx}: invalid JSON"))?;

    let success = json
        .get("success")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    if !success {
        let code = json["error"]["code"].as_str().unwrap_or("UNKNOWN");
        let msg = json["error"]["message"].as_str().unwrap_or("no message");
        tracing::error!(endpoint = ctx, code, message = msg, "api error");
        return Err(anyhow::anyhow!("{ctx}: API error [{code}] {msg}"));
    }

    serde_json::from_value(json["data"].clone())
        .with_context(|| format!("{ctx}: failed to parse `data` field"))
}

/// GET helper — logs URL then delegates to `parse_api`.
async fn get_api<T: DeserializeOwned>(
    client: &Client,
    builder: reqwest::RequestBuilder,
    ctx: &str,
) -> Result<T> {
    let req = builder
        .build()
        .with_context(|| format!("{ctx}: failed to build request"))?;
    tracing::debug!(endpoint = ctx, method = "GET", url = %req.url(), "→ request");

    let resp = client
        .execute(req)
        .await
        .with_context(|| format!("{ctx} request failed"))?;
    parse_api(resp, ctx).await
}

/// POST helper — serialises body for logging, then sends.
async fn post_api<B, T>(client: &Client, url: &str, body: &B, ctx: &str) -> Result<T>
where
    B: Serialize,
    T: DeserializeOwned,
{
    let body_json = serde_json::to_string(body).unwrap_or_else(|_| "{}".to_owned());
    tracing::debug!(endpoint = ctx, method = "POST", url, request = %body_json, "→ request");

    let resp = client
        .post(url)
        .json(body)
        .send()
        .await
        .with_context(|| format!("{ctx} request failed"))?;
    parse_api(resp, ctx).await
}

// ── public API functions ─────────────────────────────────────────────────────

/// POST /v1/auth → guest token (must be called before login)
pub async fn get_guest_token(client: &Client) -> Result<GuestAuthData> {
    let body = GuestAuthRequest {
        client_id: "tixid_guest".to_owned(),
        auth_code: None,
    };
    post_api(client, &format!("{BASE}/v1/auth"), &body, "get_guest_token").await
}

/// POST /v1/users/refresh → new access token + refresh token.
/// The `client` must be built with the refresh_token as Authorization.
pub async fn refresh_user_token(client: &Client) -> Result<RefreshData> {
    let url = format!("{BASE}/v1/users/refresh");
    tracing::debug!(endpoint = "refresh_token", method = "POST", url = %url, "→ request");
    let resp = client
        .post(&url)
        .send()
        .await
        .context("refresh_token request failed")?;
    parse_api(resp, "refresh_token").await
}

/// POST /v1/users/login → JWT token + user info
pub async fn login(client: &Client, msisdn: &str, password: &str) -> Result<LoginData> {
    let body = LoginRequest {
        msisdn: msisdn.to_owned(),
        password: password.to_owned(),
    };
    post_api(client, &format!("{BASE}/v1/users/login"), &body, "login").await
}

/// GET /v1/movies/{movie_id} → MovieData (data.id = schedule_id)
pub async fn get_movie(client: &Client, movie_id: &str) -> Result<MovieData> {
    let builder = client.get(format!("{BASE}/v1/movies/{movie_id}"));
    get_api(client, builder, "get_movie").await
}

/// GET /v1/schedules/date?schedule_id=&city_id= → list of dates.
/// Returns `Ok(None)` when the movie has no schedule yet (400 DATA_NOT_FOUND).
pub async fn get_schedule_dates(
    client: &Client,
    schedule_id: &str,
    city_id: &str,
) -> Result<Option<Vec<ScheduleDate>>> {
    let builder = client
        .get(format!("{BASE}/v1/schedules/date"))
        .query(&[("schedule_id", schedule_id), ("city_id", city_id)]);

    let req = builder
        .build()
        .context("get_schedule_dates: failed to build request")?;
    tracing::debug!(
        endpoint = "get_schedule_dates",
        method = "GET",
        url = %req.url(),
        "→ request"
    );

    let response = client
        .execute(req)
        .await
        .context("get_schedule_dates request failed")?;

    let status = response.status();
    if status == reqwest::StatusCode::BAD_REQUEST || status == reqwest::StatusCode::NOT_FOUND {
        let text = response.text().await.unwrap_or_default();
        tracing::debug!(
            endpoint = "get_schedule_dates",
            status = status.as_u16(),
            response = %text,
            "← schedule not found"
        );
        return Ok(None);
    }

    let data: Vec<ScheduleDate> = parse_api(response, "get_schedule_dates").await?;
    Ok(Some(data))
}

/// GET /v1/schedules/movies/{schedule_id}?city_id=&date=&page=1 → theaters + showtimes
pub async fn get_showtimes(
    client: &Client,
    schedule_id: &str,
    city_id: &str,
    date: &str,
) -> Result<SchedulesData> {
    let builder = client
        .get(format!("{BASE}/v1/schedules/movies/{schedule_id}"))
        .query(&[("city_id", city_id), ("date", date), ("page", "1")]);
    get_api(client, builder, "get_showtimes").await
}

/// GET /v1/movies/{merchant_slug}/layout?show_time_id=&tz=7 → seat layout
pub async fn get_seat_layout(
    client: &Client,
    merchant_slug: &str,
    show_time_id: &str,
) -> Result<SeatLayoutData> {
    let builder = client
        .get(format!("{BASE}/v1/movies/{merchant_slug}/layout"))
        .query(&[("show_time_id", show_time_id), ("tz", "7")]);
    get_api(client, builder, "get_seat_layout").await
}

/// GET /v1/orders/{order_id}/payment?browser_type=desktop → list of payment channels
pub async fn get_payment_channels(
    client: &Client,
    order_id: &str,
) -> Result<Vec<PaymentGroup>> {
    let builder = client
        .get(format!("{BASE}/v1/orders/{order_id}/payment"))
        .query(&[("browser_type", "desktop")]);
    get_api(client, builder, "get_payment_channels").await
}

/// POST /v1/orders/{order_id}/checkout → CheckoutData (QRIS code, payment url, etc.)
pub async fn checkout(
    client: &Client,
    order_id: &str,
    latitude: &str,
    longitude: &str,
    payment_method: &str,
    payment_option: &str,
) -> Result<CheckoutData> {
    let body = CheckoutRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        latitude: latitude.to_owned(),
        longitude: longitude.to_owned(),
        payment_method: payment_method.to_owned(),
        payment_option: payment_option.to_owned(),
    };
    post_api(
        client,
        &format!("{BASE}/v1/orders/{order_id}/checkout"),
        &body,
        "checkout",
    )
    .await
}

/// POST /v1/orders → OrderData
pub async fn create_order(
    client: &Client,
    merchant_id: &str,
    time_show_id: &str,
    seats: &[String],
) -> Result<OrderData> {
    let seat_data = seats
        .iter()
        .map(|s| SeatData {
            seat_id: s.clone(),
            seat_name: s.clone(),
            seat_grd_cd: s.clone(),
        })
        .collect();

    let body = OrderRequest {
        merchant_id: merchant_id.to_owned(),
        time_show_id: time_show_id.to_owned(),
        request_id: uuid::Uuid::new_v4().to_string(),
        seat_data,
    };

    post_api(client, &format!("{BASE}/v1/orders"), &body, "create_order").await
}
