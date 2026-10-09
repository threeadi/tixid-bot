use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Serialize, de::DeserializeOwned};

use crate::models::*;

/// All API endpoints use the same gateway.
const BASE: &str = "https://api-b2b.tix.id";

static VERBOSE_TIMING: AtomicBool = AtomicBool::new(false);

/// Enable or disable printing detailed HTTP request timing metrics to terminal.
pub fn set_verbose_timing(enabled: bool) {
    VERBOSE_TIMING.store(enabled, Ordering::Relaxed);
}

/// Check if verbose HTTP timing mode is currently enabled.
pub fn is_verbose_timing() -> bool {
    VERBOSE_TIMING.load(Ordering::Relaxed)
}

/// Detailed network & server timing metrics for an HTTP request.
#[derive(Debug, Clone)]
pub struct HttpTiming {
    pub endpoint: String,
    pub method: String,
    pub status: u16,
    pub total_ms: u128,
    pub ttfb_ms: u128,
    pub server_upstream_ms: Option<u64>,
    pub server_proxy_ms: Option<u64>,
    pub network_rtt_ms: u128,
    pub download_ms: u128,
    pub parse_ms: u128,
    pub payload_bytes: usize,
}

/// Measures direct TCP handshake RTT to api-b2b.tix.id:443.
pub async fn probe_network_rtt() -> Result<std::time::Duration> {
    let t0 = Instant::now();
    let stream = tokio::net::TcpStream::connect("api-b2b.tix.id:443")
        .await
        .context("Gagal terhubung ke api-b2b.tix.id:443")?;
    let rtt = t0.elapsed();
    drop(stream);
    Ok(rtt)
}

/// Prints a formatted visual breakdown of where request latency was spent.
pub fn print_timing_breakdown(t: &HttpTiming) {
    let size_kb = t.payload_bytes as f64 / 1024.0;
    let upstream = t.server_upstream_ms.unwrap_or(0);
    let proxy = t.server_proxy_ms.unwrap_or(0);
    let server_total = (upstream + proxy) as u128;
    let total = t.total_ms.max(1);

    let net_pct = (t.network_rtt_ms as f64 / total as f64 * 100.0).round() as u64;
    let srv_pct = (server_total as f64 / total as f64 * 100.0).round() as u64;
    let dl_pct = (t.download_ms as f64 / total as f64 * 100.0).round() as u64;

    let sys = crate::metrics::sample_metrics();

    println!("  ┌─ ⏱️  [{} {}] Total: {}ms (Payload: {:.1} KB, Status: {})", t.method, t.endpoint, t.total_ms, size_kb, t.status);
    println!("  ├─ 🌐 Network RTT (Transit) : {:>3}ms ({:>2}%)", t.network_rtt_ms, net_pct);
    println!("  ├─ ⚙️  Server Backend (TIX)  : {:>3}ms ({:>2}%) [Upstream: {}ms, Proxy: {}ms]", server_total, srv_pct, upstream, proxy);
    println!("  ├─ 📥 Download Stream       : {:>3}ms ({:>2}%)", t.download_ms, dl_pct);
    println!("  ├─ 🧩 JSON Parse (CPU)      : {:>3}ms", t.parse_ms);
    println!("  ├─ 💻 Bot System Footprint  : RAM: {:.1} MB • CPU: {:.1}%", sys.ram_mb, sys.cpu_percent);

    if srv_pct > 60 {
        println!("  └─ 💡 Kesimpulan: {}% waktu dihabiskan oleh SERVER TIX ID (Koneksi internet Anda cepat).", srv_pct);
    } else if net_pct > 60 {
        println!("  └─ 💡 Kesimpulan: {}% waktu dihabiskan di JARINGAN INTERNET (Server TIX ID sebenarnya cepat, koneksi Anda lemot!).", net_pct);
    } else {
        println!("  └─ 💡 Kesimpulan: Waktu terbagi seimbang antara Jaringan ({}%) dan Server TIX ID ({}%).", net_pct, srv_pct);
    }
}

struct RawApiResponse {
    pub status: reqwest::StatusCode,
    pub bytes: Vec<u8>,
    pub timing: HttpTiming,
}

async fn execute_with_timing(
    client: &Client,
    req: reqwest::Request,
    ctx: &str,
) -> Result<RawApiResponse> {
    let method = req.method().to_string();
    let t0 = Instant::now();

    let resp = client
        .execute(req)
        .await
        .with_context(|| format!("{ctx} request failed"))?;

    let ttfb = t0.elapsed();
    let status = resp.status();

    // Extract Kong latency headers from TIX ID server
    let upstream_ms: Option<u64> = resp
        .headers()
        .get("x-kong-upstream-latency")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.parse().ok());

    let proxy_ms: Option<u64> = resp
        .headers()
        .get("x-kong-proxy-latency")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.parse().ok());

    let t_dl = Instant::now();
    let body_bytes = resp
        .bytes()
        .await
        .with_context(|| format!("{ctx}: failed to read response body"))?;
    let download_time = t_dl.elapsed();
    let payload_bytes = body_bytes.len();
    let bytes = body_bytes.to_vec();
    let total_time = t0.elapsed();

    let server_processing_ms = (upstream_ms.unwrap_or(0) + proxy_ms.unwrap_or(0)) as u128;
    let network_rtt_ms = ttfb.as_millis().saturating_sub(server_processing_ms);

    let timing = HttpTiming {
        endpoint: ctx.to_string(),
        method,
        status: status.as_u16(),
        total_ms: total_time.as_millis(),
        ttfb_ms: ttfb.as_millis(),
        server_upstream_ms: upstream_ms,
        server_proxy_ms: proxy_ms,
        network_rtt_ms,
        download_ms: download_time.as_millis(),
        parse_ms: 0,
        payload_bytes,
    };

    Ok(RawApiResponse {
        status,
        bytes,
        timing,
    })
}

async fn parse_raw<T: DeserializeOwned>(mut raw: RawApiResponse, ctx: &str) -> Result<T> {
    let t_parse = Instant::now();
    let json: serde_json::Value = serde_json::from_slice(&raw.bytes)
        .with_context(|| format!("{ctx}: invalid JSON: {}", String::from_utf8_lossy(&raw.bytes)))?;
    raw.timing.parse_ms = t_parse.elapsed().as_millis();
    raw.timing.total_ms += raw.timing.parse_ms;

    tracing::debug!(
        endpoint = %raw.timing.endpoint,
        status = raw.timing.status,
        total_ms = raw.timing.total_ms,
        ttfb_ms = raw.timing.ttfb_ms,
        upstream_ms = ?raw.timing.server_upstream_ms,
        proxy_ms = ?raw.timing.server_proxy_ms,
        network_rtt_ms = raw.timing.network_rtt_ms,
        download_ms = raw.timing.download_ms,
        parse_ms = raw.timing.parse_ms,
        bytes = raw.timing.payload_bytes,
        "← HTTP Response Timing"
    );

    if is_verbose_timing() {
        print_timing_breakdown(&raw.timing);
    }

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

// ── internal helpers ─────────────────────────────────────────────────────────

/// GET helper — logs URL then delegates to `execute_with_timing` and `parse_raw`.
async fn get_api<T: DeserializeOwned>(
    client: &Client,
    builder: reqwest::RequestBuilder,
    ctx: &str,
) -> Result<T> {
    let req = builder
        .build()
        .with_context(|| format!("{ctx}: failed to build request"))?;
    tracing::debug!(endpoint = ctx, method = "GET", url = %req.url(), "→ request");

    let raw = execute_with_timing(client, req, ctx).await?;
    parse_raw(raw, ctx).await
}

/// POST helper — serialises body for logging, then sends with timing.
async fn post_api<B, T>(client: &Client, url: &str, body: &B, ctx: &str) -> Result<T>
where
    B: Serialize,
    T: DeserializeOwned,
{
    let body_json = serde_json::to_string(body).unwrap_or_else(|_| "{}".to_owned());
    tracing::debug!(endpoint = ctx, method = "POST", url, request = %body_json, "→ request");

    let req = client
        .post(url)
        .json(body)
        .build()
        .with_context(|| format!("{ctx}: failed to build request"))?;

    let raw = execute_with_timing(client, req, ctx).await?;
    parse_raw(raw, ctx).await
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
    let req = client
        .post(&url)
        .build()
        .context("refresh_token: failed to build request")?;
    let raw = execute_with_timing(client, req, "refresh_token").await?;
    parse_raw(raw, "refresh_token").await
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

    let raw = execute_with_timing(client, req, "get_schedule_dates").await?;
    let status = raw.status;
    if status == reqwest::StatusCode::BAD_REQUEST || status == reqwest::StatusCode::NOT_FOUND {
        let text = String::from_utf8_lossy(&raw.bytes);
        tracing::debug!(
            endpoint = "get_schedule_dates",
            status = status.as_u16(),
            response = %text,
            "← schedule not found"
        );
        if is_verbose_timing() {
            print_timing_breakdown(&raw.timing);
        }
        return Ok(None);
    }

    let data: Vec<ScheduleDate> = parse_raw(raw, "get_schedule_dates").await?;
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
    seats: &[crate::models::SelectedSeat],
) -> Result<OrderData> {
    let seat_data = seats
        .iter()
        .map(|s| SeatData {
            seat_id: s.seat_id.clone(),
            seat_name: s.display.clone(),
            seat_grd_cd: s.grd_cd.clone(),
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
