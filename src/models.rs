use serde::{Deserialize, Serialize};

// ── Guest Auth ────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct GuestAuthRequest {
    pub client_id: String,
    pub auth_code: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GuestAuthData {
    pub token: String,
    pub expires_in: i64,
}

// ── Login ────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct LoginRequest {
    pub msisdn: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct LoginData {
    pub id: String,
    pub name: String,
    pub phone: String,
    pub token: String,
    pub refresh_token: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RefreshData {
    pub token: String,
    pub refresh_token: String,
}

// ── Movie ────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct MovieData {
    /// This is the schedule_id used for subsequent schedule endpoints.
    pub id: String,
    pub movie_id: String,
    pub name: String,
    pub status: String,
    pub duration: i32,
    /// 1 = presale/coming soon, 0 = normal
    pub presale_flag: Option<i32>,
    /// Unix seconds — release date (present for UPCOMING movies)
    pub release_date: Option<i64>,
}

// ── Schedule dates ───────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ScheduleDate {
    pub date: String,
    pub is_any_schedule: bool,
}

// ── Theaters & showtimes ─────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct SchedulesData {
    pub has_next: bool,
    pub page: i32,
    pub theaters: Vec<Theater>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Theater {
    pub id: String,
    pub name: String,
    pub merchant: Merchant,
    pub address: Option<String>,
    pub price_groups: Vec<PriceGroup>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Merchant {
    pub merchant_id: String,
    pub merchant_name: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct PriceGroup {
    pub category: String,
    pub low_price: i64,
    pub high_price: i64,
    pub price_string: String,
    pub show_time: Vec<ShowTime>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ShowTime {
    pub id: String,
    /// Unix milliseconds
    pub time: i64,
    /// "HH:MM" display string, e.g. "14:30"
    pub display_time: String,
    pub studio: String,
    /// Unix milliseconds — booking cutoff
    pub expired: i64,
    /// 0 = expired/closed, 1 = open for purchase
    pub status: i32,
    pub price: i64,
}

// ── Seat layout ──────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct SeatLayoutData {
    pub user_seat_transaction_limit: usize,
    pub seat_map: Vec<SeatMap>,
}

#[derive(Debug, Deserialize)]
pub struct SeatMap {
    pub seat_code: String,
    pub max_row: usize,
    pub seat_rows: Vec<SeatRow>,
}

#[derive(Debug, Deserialize)]
pub struct SeatRow {
    pub seat_row: String,
    /// 1 = available, 6 = no physical seat / blocked
    pub status: i32,
}

// ── Payment channels ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct PaymentGroup {
    pub payment_method: String,
    pub payment_options: Vec<PaymentOption>,
}

#[derive(Debug, Deserialize)]
pub struct PaymentOption {
    pub name: String,
    pub payment_method: String,
    pub payment_option: String,
    pub payment_fee: i64,
    pub is_disable: bool,
    pub slug: String,
}

// ── Checkout ─────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct CheckoutRequest {
    pub request_id: String,
    pub latitude: String,
    pub longitude: String,
    pub payment_method: String,
    pub payment_option: String,
}

#[derive(Debug, Deserialize)]
pub struct CheckoutData {
    pub payment_method: String,
    pub payment_option: String,
    pub checkout_url: String,
    /// QRIS string (for QRIS payment_option)
    pub payment_code: String,
    pub total_payment: i64,
}


#[derive(Debug, Serialize)]
pub struct OrderRequest {
    pub merchant_id: String,
    pub time_show_id: String,
    pub request_id: String,
    pub seat_data: Vec<SeatData>,
}

#[derive(Debug, Serialize)]
pub struct SeatData {
    pub seat_id: String,
    pub seat_name: String,
    pub seat_grd_cd: String,
}

#[derive(Debug, Deserialize)]
pub struct OrderData {
    pub id: String,
    pub movie_name: String,
    pub theater_name: String,
    pub studio_name: String,
    /// Unix seconds
    pub event_start: i64,
    pub quantity: usize,
    pub selected_seats: Vec<String>,
    pub total_ticket_price: i64,
    pub convenience_fee: i64,
    pub total: i64,
    /// Unix seconds
    pub expired_at: i64,
    pub merchant: Merchant,
}
