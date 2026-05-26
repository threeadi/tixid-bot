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

/// A seat chosen for booking, carrying all fields needed for `create_order`.
pub struct SelectedSeat {
    /// The booking ID sent as `seat_id` in `create_order`.
    pub seat_id: String,
    /// Human-readable label (e.g. "A4") for display and `seat_name`.
    pub display: String,
    /// Price-tier code sent as `seat_grd_cd` in `create_order`.
    pub grd_cd: String,
}

// ── Seat layout ──────────────────────────────────────────────────────────────

/// Normalised seat layout returned by `api::get_seat_layout`.
/// Supports both XXI (nested) and Cinepolis/CGV (flat) API responses.
#[derive(Debug)]
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
    /// Actual ID passed to `create_order`. For Cinepolis/CGV this differs from
    /// `seat_row` (e.g. "2-0-0-0" vs display label "A1"). None for XXI, where
    /// `seat_row` doubles as the booking ID.
    #[serde(default)]
    pub booking_id: Option<String>,
    /// 1 = available, 6 = no physical seat / blocked
    pub status: i32,
    /// Price-tier code sent as `seat_grd_cd` in `create_order`
    /// (e.g. "0000000013" for Cinepolis PREFERRED). None / "" for XXI.
    #[serde(default)]
    pub grid_cd: Option<String>,
    /// Physical aisle side: "L" or "R". Computed post-parse, never from API.
    /// Prevents cross-aisle seats from being treated as consecutive.
    #[serde(skip)]
    pub aisle_side: Option<String>,
}

// ── Multi-format seat layout deserialization ──────────────────────────────────

/// Vertical aisle rule from XXI `seat_rules.vertical_lane`.
/// `before_seat_column` is the first column on the RIGHT side of the aisle
/// (i.e. columns < before_seat_column are LEFT, columns >= are RIGHT).
#[derive(Deserialize)]
struct VerticalLane {
    start: String,
    end: String,
    before_seat_column: usize,
}

#[derive(Deserialize, Default)]
struct SeatRulesRaw {
    #[serde(default)]
    vertical_lane: Option<Vec<VerticalLane>>,
}

/// XXI: seat_map is already grouped by row with seat_code / max_row / seat_rows.
#[derive(Deserialize)]
struct SeatLayoutXxi {
    user_seat_transaction_limit: usize,
    seat_map: Vec<SeatMap>,
    #[serde(default)]
    seat_rules: Option<SeatRulesRaw>,
}

/// Cinepolis / CGV: seat_map is a flat list of individual seat objects.
#[derive(Deserialize)]
struct SeatLayoutFlatRaw {
    user_seat_transaction_limit: usize,
    seat_map: Vec<FlatSeat>,
}

#[derive(Deserialize)]
struct FlatSeat {
    seat_id: String,
    row_name: String,
    seat_no: Option<String>,
    #[serde(default)]
    seat_yn: Option<String>,
    seat_status: i32,
    #[serde(default)]
    seat_grd_cd: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum SeatLayoutRaw {
    Xxi(SeatLayoutXxi),
    Flat(SeatLayoutFlatRaw),
}

impl<'de> Deserialize<'de> for SeatLayoutData {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        SeatLayoutRaw::deserialize(deserializer).map(|raw| match raw {
            SeatLayoutRaw::Xxi(mut x) => {
                // Apply aisle-side tags from seat_rules.vertical_lane so that
                // seats on opposite sides of the centre aisle (e.g. F7 / F8)
                // are never treated as consecutive.
                if let Some(rules) = x.seat_rules.take() {
                    if let Some(lanes) = rules.vertical_lane {
                        for sm in &mut x.seat_map {
                            let row_idx = row_char_idx(&sm.seat_code);
                            for lane in &lanes {
                                let lo = row_char_idx(&lane.start);
                                let hi = row_char_idx(&lane.end);
                                if row_idx >= lo && row_idx <= hi {
                                    let boundary = lane.before_seat_column;
                                    for sr in &mut sm.seat_rows {
                                        if let Some(col) = label_col(&sr.seat_row) {
                                            sr.aisle_side = Some(
                                                if col < boundary { "L" } else { "R" }
                                                    .to_string(),
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                SeatLayoutData {
                    user_seat_transaction_limit: x.user_seat_transaction_limit,
                    seat_map: x.seat_map,
                }
            }
            SeatLayoutRaw::Flat(f) => SeatLayoutData {
                user_seat_transaction_limit: f.user_seat_transaction_limit,
                seat_map: normalize_flat_seats(f.seat_map),
            },
        })
    }
}

/// Row letter → 0-based index: "A" → 0, "B" → 1, …
fn row_char_idx(code: &str) -> usize {
    code.chars()
        .next()
        .map(|c| (c.to_ascii_uppercase() as usize).saturating_sub('A' as usize))
        .unwrap_or(0)
}

/// Extract column number from a seat label like "F7" → Some(7).
fn label_col(label: &str) -> Option<usize> {
    let digits: String = label.chars().filter(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

/// Convert a Cinepolis/CGV flat seat list into the normalised `Vec<SeatMap>`
/// (one entry per row), preserving `seat_id` as the booking ID and tagging
/// each seat with its physical aisle side ("L" / "R") based on spacer position.
fn normalize_flat_seats(seats: Vec<FlatSeat>) -> Vec<SeatMap> {
    use std::collections::BTreeMap;

    let mut rows: BTreeMap<String, Vec<(usize, SeatRow)>> = BTreeMap::new();
    // Last real column seen per row — used to record where the spacer falls.
    let mut last_col: BTreeMap<String, usize> = BTreeMap::new();
    // Column after which an aisle spacer was observed, per row.
    let mut aisle_after: BTreeMap<String, usize> = BTreeMap::new();

    for seat in seats {
        if seat.seat_yn.as_deref() != Some("1") {
            // Spacer / aisle marker — record its position within this row.
            if let Some(&col) = last_col.get(&seat.row_name) {
                aisle_after.entry(seat.row_name).or_insert(col);
            }
            continue;
        }
        let col: usize = match seat.seat_no.as_deref().and_then(|s| s.parse().ok()) {
            Some(n) => n,
            None => continue,
        };
        last_col.insert(seat.row_name.clone(), col);
        rows.entry(seat.row_name.clone())
            .or_default()
            .push((col, SeatRow {
                seat_row: format!("{}{}", seat.row_name, col),
                booking_id: Some(seat.seat_id),
                status: seat.seat_status,
                grid_cd: seat.seat_grd_cd,
                aisle_side: None, // filled below
            }));
    }

    rows.into_iter()
        .map(|(row_name, mut entries)| {
            entries.sort_by_key(|(col, _)| *col);
            // Tag each seat with its physical side relative to the centre aisle.
            if let Some(&boundary) = aisle_after.get(&row_name) {
                for (col, sr) in &mut entries {
                    sr.aisle_side =
                        Some(if *col <= boundary { "L" } else { "R" }.to_string());
                }
            }
            let max_row = entries.iter().map(|(col, _)| *col).max().unwrap_or(0);
            SeatMap {
                seat_code: row_name,
                max_row,
                seat_rows: entries.into_iter().map(|(_, sr)| sr).collect(),
            }
        })
        .collect()
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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Auth models ───────────────────────────────────────────────────────

    #[test]
    fn deserialize_guest_auth_data() {
        let json = r#"{"token":"abc","expires_in":3600}"#;
        let d: GuestAuthData = serde_json::from_str(json).unwrap();
        assert_eq!(d.token, "abc");
        assert_eq!(d.expires_in, 3600);
    }

    #[test]
    fn serialize_guest_auth_request_includes_client_id() {
        let r = GuestAuthRequest { client_id: "tixid_guest".into(), auth_code: None };
        let s = serde_json::to_string(&r).unwrap();
        assert!(s.contains("tixid_guest"));
    }

    #[test]
    fn serialize_guest_auth_request_with_auth_code() {
        let r = GuestAuthRequest { client_id: "id".into(), auth_code: Some("code123".into()) };
        let s = serde_json::to_string(&r).unwrap();
        assert!(s.contains("code123"));
    }

    #[test]
    fn deserialize_login_data_with_refresh_token() {
        let json = r#"{"id":"1","name":"Alice","phone":"08123","token":"tok","refresh_token":"ref"}"#;
        let d: LoginData = serde_json::from_str(json).unwrap();
        assert_eq!(d.name, "Alice");
        assert_eq!(d.refresh_token, Some("ref".into()));
    }

    #[test]
    fn deserialize_login_data_without_refresh_token() {
        let json = r#"{"id":"1","name":"Bob","phone":"08123","token":"tok"}"#;
        let d: LoginData = serde_json::from_str(json).unwrap();
        assert_eq!(d.refresh_token, None);
    }

    #[test]
    fn deserialize_refresh_data() {
        let json = r#"{"token":"new_tok","refresh_token":"new_ref"}"#;
        let d: RefreshData = serde_json::from_str(json).unwrap();
        assert_eq!(d.token, "new_tok");
        assert_eq!(d.refresh_token, "new_ref");
    }

    // ── Movie / Schedule ──────────────────────────────────────────────────

    #[test]
    fn deserialize_movie_data_minimal() {
        let json = r#"{"id":"s1","movie_id":"m1","name":"Test Movie","status":"NOW_PLAYING","duration":120}"#;
        let d: MovieData = serde_json::from_str(json).unwrap();
        assert_eq!(d.name, "Test Movie");
        assert_eq!(d.duration, 120);
        assert!(d.presale_flag.is_none());
        assert!(d.release_date.is_none());
    }

    #[test]
    fn deserialize_movie_data_with_presale_and_release() {
        let json = r#"{"id":"s1","movie_id":"m1","name":"Upcoming","status":"UPCOMING","duration":100,"presale_flag":1,"release_date":1748736000}"#;
        let d: MovieData = serde_json::from_str(json).unwrap();
        assert_eq!(d.presale_flag, Some(1));
        assert_eq!(d.release_date, Some(1_748_736_000));
    }

    #[test]
    fn deserialize_schedule_date() {
        let json = r#"{"date":"2025-06-01","is_any_schedule":true}"#;
        let d: ScheduleDate = serde_json::from_str(json).unwrap();
        assert_eq!(d.date, "2025-06-01");
        assert!(d.is_any_schedule);
    }

    #[test]
    fn deserialize_theater_with_merchant_and_price_groups() {
        let json = r#"{
            "id":"t1","name":"XXI Mall",
            "merchant":{"merchant_id":"m1","merchant_name":"Cinema XXI"},
            "price_groups":[]
        }"#;
        let d: Theater = serde_json::from_str(json).unwrap();
        assert_eq!(d.name, "XXI Mall");
        assert_eq!(d.merchant.merchant_name, "Cinema XXI");
        assert!(d.address.is_none());
    }

    #[test]
    fn deserialize_showtime_fields() {
        let json = r#"{"id":"st1","time":1748768400000,"display_time":"14:00","studio":"Studio 1","expired":1748772000000,"status":1,"price":75000}"#;
        let d: ShowTime = serde_json::from_str(json).unwrap();
        assert_eq!(d.display_time, "14:00");
        assert_eq!(d.status, 1);
        assert_eq!(d.price, 75_000);
    }

    // ── SeatLayoutData deserialization: XXI format ─────────────────────────

    #[test]
    fn deserialize_seat_layout_xxi_format() {
        let json = r#"{
            "user_seat_transaction_limit": 6,
            "seat_map": [
                {
                    "seat_code": "A",
                    "max_row": 8,
                    "seat_rows": [
                        {"seat_row": "A1", "status": 1},
                        {"seat_row": "A2", "status": 0}
                    ]
                }
            ]
        }"#;
        let layout: SeatLayoutData = serde_json::from_str(json).unwrap();
        assert_eq!(layout.user_seat_transaction_limit, 6);
        assert_eq!(layout.seat_map.len(), 1);
        assert_eq!(layout.seat_map[0].seat_code, "A");
        assert_eq!(layout.seat_map[0].seat_rows[0].status, 1);
        assert_eq!(layout.seat_map[0].seat_rows[1].status, 0);
    }

    #[test]
    fn deserialize_seat_layout_xxi_applies_aisle_side_from_seat_rules() {
        // F1 (col 1) < boundary 7 → "L". F7 (col 7) >= 7 → "R".
        let json = r#"{
            "user_seat_transaction_limit": 6,
            "seat_map": [
                {
                    "seat_code": "F",
                    "max_row": 12,
                    "seat_rows": [
                        {"seat_row": "F1", "status": 1},
                        {"seat_row": "F7", "status": 1},
                        {"seat_row": "F12", "status": 1}
                    ]
                }
            ],
            "seat_rules": {
                "vertical_lane": [{"start": "A", "end": "J", "before_seat_column": 7}]
            }
        }"#;
        let layout: SeatLayoutData = serde_json::from_str(json).unwrap();
        let row = &layout.seat_map[0];
        let f1 = row.seat_rows.iter().find(|r| r.seat_row == "F1").unwrap();
        let f7 = row.seat_rows.iter().find(|r| r.seat_row == "F7").unwrap();
        assert_eq!(f1.aisle_side.as_deref(), Some("L"));
        assert_eq!(f7.aisle_side.as_deref(), Some("R"));
    }

    #[test]
    fn deserialize_seat_layout_xxi_seat_rules_only_applies_to_matching_rows() {
        // seat_rules applies to rows A-J only; row "K" should have no aisle_side
        let json = r#"{
            "user_seat_transaction_limit": 6,
            "seat_map": [
                {
                    "seat_code": "K",
                    "max_row": 8,
                    "seat_rows": [{"seat_row": "K4", "status": 1}]
                }
            ],
            "seat_rules": {
                "vertical_lane": [{"start": "A", "end": "J", "before_seat_column": 5}]
            }
        }"#;
        let layout: SeatLayoutData = serde_json::from_str(json).unwrap();
        let k4 = &layout.seat_map[0].seat_rows[0];
        assert_eq!(k4.aisle_side, None);
    }

    // ── SeatLayoutData deserialization: Flat (Cinepolis/CGV) format ────────

    #[test]
    fn deserialize_seat_layout_flat_basic() {
        let json = r#"{
            "user_seat_transaction_limit": 8,
            "seat_map": [
                {"seat_id": "1-A1", "row_name": "A", "seat_no": "1", "seat_yn": "1", "seat_status": 1},
                {"seat_id": "1-A2", "row_name": "A", "seat_no": "2", "seat_yn": "1", "seat_status": 1},
                {"seat_id": "1-A3", "row_name": "A", "seat_no": "3", "seat_yn": "1", "seat_status": 0}
            ]
        }"#;
        let layout: SeatLayoutData = serde_json::from_str(json).unwrap();
        assert_eq!(layout.user_seat_transaction_limit, 8);
        assert_eq!(layout.seat_map.len(), 1);
        assert_eq!(layout.seat_map[0].seat_code, "A");
        assert_eq!(layout.seat_map[0].seat_rows.len(), 3);
    }

    #[test]
    fn deserialize_flat_spacer_skipped_and_aisle_side_tagged() {
        // Spacer after col 2 → A1,A2 = "L";  A3,A4 = "R"
        let json = r#"{
            "user_seat_transaction_limit": 8,
            "seat_map": [
                {"seat_id": "A1", "row_name": "A", "seat_no": "1", "seat_yn": "1", "seat_status": 1},
                {"seat_id": "A2", "row_name": "A", "seat_no": "2", "seat_yn": "1", "seat_status": 1},
                {"seat_id": "sp", "row_name": "A", "seat_no": null, "seat_yn": "0", "seat_status": 6},
                {"seat_id": "A3", "row_name": "A", "seat_no": "3", "seat_yn": "1", "seat_status": 1},
                {"seat_id": "A4", "row_name": "A", "seat_no": "4", "seat_yn": "1", "seat_status": 1}
            ]
        }"#;
        let layout: SeatLayoutData = serde_json::from_str(json).unwrap();
        let row = &layout.seat_map[0];
        assert_eq!(row.seat_rows.len(), 4); // spacer excluded
        let a1 = row.seat_rows.iter().find(|r| r.seat_row == "A1").unwrap();
        let a3 = row.seat_rows.iter().find(|r| r.seat_row == "A3").unwrap();
        assert_eq!(a1.aisle_side.as_deref(), Some("L"));
        assert_eq!(a3.aisle_side.as_deref(), Some("R"));
    }

    #[test]
    fn deserialize_flat_booking_id_set_from_seat_id() {
        let json = r#"{
            "user_seat_transaction_limit": 4,
            "seat_map": [
                {"seat_id": "2-0-0-1", "row_name": "B", "seat_no": "1", "seat_yn": "1", "seat_status": 1}
            ]
        }"#;
        let layout: SeatLayoutData = serde_json::from_str(json).unwrap();
        let sr = &layout.seat_map[0].seat_rows[0];
        assert_eq!(sr.booking_id.as_deref(), Some("2-0-0-1"));
    }

    #[test]
    fn deserialize_flat_multiple_rows() {
        let json = r#"{
            "user_seat_transaction_limit": 6,
            "seat_map": [
                {"seat_id": "A1", "row_name": "A", "seat_no": "1", "seat_yn": "1", "seat_status": 1},
                {"seat_id": "B1", "row_name": "B", "seat_no": "1", "seat_yn": "1", "seat_status": 1},
                {"seat_id": "B2", "row_name": "B", "seat_no": "2", "seat_yn": "1", "seat_status": 0}
            ]
        }"#;
        let layout: SeatLayoutData = serde_json::from_str(json).unwrap();
        assert_eq!(layout.seat_map.len(), 2);
    }

    // ── Payment / Order models ─────────────────────────────────────────────

    #[test]
    fn deserialize_checkout_data() {
        let json = r#"{"payment_method":"NETWORK_PAY","payment_option":"QRIS","checkout_url":"https://pay.example.com","payment_code":"00020101","total_payment":88000}"#;
        let d: CheckoutData = serde_json::from_str(json).unwrap();
        assert_eq!(d.total_payment, 88_000);
        assert_eq!(d.payment_method, "NETWORK_PAY");
        assert_eq!(d.payment_code, "00020101");
    }

    #[test]
    fn serialize_checkout_request() {
        let r = CheckoutRequest {
            request_id: "req-1".into(),
            latitude: "-6.2".into(),
            longitude: "106.8".into(),
            payment_method: "NETWORK_PAY".into(),
            payment_option: "QRIS".into(),
        };
        let s = serde_json::to_string(&r).unwrap();
        assert!(s.contains("req-1"));
        assert!(s.contains("QRIS"));
    }

    #[test]
    fn deserialize_order_data() {
        let json = r#"{
            "id":"ord1","movie_name":"Test","theater_name":"XXI","studio_name":"Studio 1",
            "event_start":1748736000,"quantity":2,"selected_seats":["A1","A2"],
            "total_ticket_price":150000,"convenience_fee":5000,"total":155000,
            "expired_at":1748740000,
            "merchant":{"merchant_id":"m1","merchant_name":"Cinema XXI"}
        }"#;
        let d: OrderData = serde_json::from_str(json).unwrap();
        assert_eq!(d.id, "ord1");
        assert_eq!(d.quantity, 2);
        assert_eq!(d.selected_seats, vec!["A1", "A2"]);
        assert_eq!(d.total, 155_000);
        assert_eq!(d.merchant.merchant_id, "m1");
    }

    #[test]
    fn serialize_order_request_with_seat_data() {
        let r = OrderRequest {
            merchant_id: "m1".into(),
            time_show_id: "st1".into(),
            request_id: "req-1".into(),
            seat_data: vec![SeatData {
                seat_id: "A1".into(),
                seat_name: "A1".into(),
                seat_grd_cd: "GRD001".into(),
            }],
        };
        let s = serde_json::to_string(&r).unwrap();
        assert!(s.contains("seat_data"));
        assert!(s.contains("GRD001"));
    }

    #[test]
    fn deserialize_payment_group_and_option() {
        let json = r#"[{
            "payment_method":"NETWORK_PAY",
            "payment_options":[{
                "name":"QRIS","payment_method":"NETWORK_PAY",
                "payment_option":"QRIS","payment_fee":0,
                "is_disable":false,"slug":"qris"
            }]
        }]"#;
        let groups: Vec<PaymentGroup> = serde_json::from_str(json).unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].payment_options[0].slug, "qris");
        assert!(!groups[0].payment_options[0].is_disable);
    }
}
