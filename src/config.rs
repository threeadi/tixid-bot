use anyhow::Result;
use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub auth: AuthConfig,
    pub target: TargetConfig,
    pub theater: TheaterConfig,
    pub showtime: ShowtimeConfig,
    pub seat: SeatConfig,
    pub device: DeviceConfig,
    pub payment: PaymentConfig,
    pub polling: PollingConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AuthConfig {
    pub msisdn: String,
    pub password: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct TargetConfig {
    pub movie_id: String,
    pub city_id: String,
    pub date: String,
    /// Datetime ranges to skip. Each entry is ["YYYY-MM-DD HH:MM", "YYYY-MM-DD HH:MM"] (inclusive).
    /// Date-only format ["YYYY-MM-DD", "YYYY-MM-DD"] is also accepted (treated as 00:00–23:59).
    #[serde(default)]
    pub blocked_datetime_ranges: Vec<Vec<String>>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct TheaterConfig {
    pub theater_priority: Vec<String>,
    /// Theaters to always skip (substring match, case-insensitive).
    #[serde(default)]
    pub blocked_theaters: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ShowtimeConfig {
    /// "HH:MM" or "YYYY-MM-DD HH:MM". Empty = no bound.
    pub preferred_time_start: String,
    /// "HH:MM" or "YYYY-MM-DD HH:MM". Empty = no bound.
    pub preferred_time_end: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SeatConfig {
    pub quantity: usize,
    pub manual_seats: Vec<String>,
    pub avoid_first_rows: usize,
    /// Two-element vec: [start_row, end_row] (e.g. ["D", "H"])
    pub preferred_rows: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DeviceConfig {
    pub device_id: String,
    pub longitude: String,
    pub latitude: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct PaymentConfig {
    /// e.g. "NETWORK_PAY"
    pub payment_method: String,
    /// e.g. "NETWORK_PAY_PG_QRIS"
    pub payment_option: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct PollingConfig {
    /// Keep polling instead of failing immediately when schedule is not ready.
    pub enabled: bool,
    /// Seconds between retries when movie schedule is not yet available
    pub interval_secs: u64,
    /// Re-login after this many seconds while waiting in polling loop
    pub refresh_token_before_secs: u64,
    /// Optional WIB time ("YYYY-MM-DD HH:MM:SS") before polling starts
    pub start_at: String,
}

pub fn load() -> Result<Config> {
    let text = std::fs::read_to_string("config.toml")
        .map_err(|e| anyhow::anyhow!("Cannot read config.toml: {}", e))?;
    let config: Config = toml::from_str(&text)
        .map_err(|e| anyhow::anyhow!("Cannot parse config.toml: {}", e))?;
    Ok(config)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = r#"
[auth]
msisdn = "08123456789"
password = "testpass"

[target]
movie_id = "m123"
city_id = "c456"
date = "2025-06-01"

[theater]
theater_priority = ["XXI", "CGV"]

[showtime]
preferred_time_start = "12:00"
preferred_time_end = "22:00"

[seat]
quantity = 2
manual_seats = []
avoid_first_rows = 1
preferred_rows = ["D", "H"]

[device]
device_id = "dev-001"
longitude = "106.8"
latitude = "-6.2"

[payment]
payment_method = "NETWORK_PAY"
payment_option = "NETWORK_PAY_PG_QRIS"

[polling]
enabled = false
interval_secs = 30
refresh_token_before_secs = 300
start_at = ""
"#;

    #[test]
    fn deserialize_auth_fields() {
        let c: Config = toml::from_str(MINIMAL).unwrap();
        assert_eq!(c.auth.msisdn, "08123456789");
        assert_eq!(c.auth.password, "testpass");
    }

    #[test]
    fn deserialize_target_fields() {
        let c: Config = toml::from_str(MINIMAL).unwrap();
        assert_eq!(c.target.movie_id, "m123");
        assert_eq!(c.target.city_id, "c456");
        assert_eq!(c.target.date, "2025-06-01");
    }

    #[test]
    fn blocked_datetime_ranges_defaults_to_empty() {
        let c: Config = toml::from_str(MINIMAL).unwrap();
        assert!(c.target.blocked_datetime_ranges.is_empty());
    }

    #[test]
    fn deserialize_theater_fields() {
        let c: Config = toml::from_str(MINIMAL).unwrap();
        assert_eq!(c.theater.theater_priority, vec!["XXI", "CGV"]);
        assert!(c.theater.blocked_theaters.is_empty());
    }

    #[test]
    fn deserialize_showtime_fields() {
        let c: Config = toml::from_str(MINIMAL).unwrap();
        assert_eq!(c.showtime.preferred_time_start, "12:00");
        assert_eq!(c.showtime.preferred_time_end, "22:00");
    }

    #[test]
    fn deserialize_seat_fields() {
        let c: Config = toml::from_str(MINIMAL).unwrap();
        assert_eq!(c.seat.quantity, 2);
        assert_eq!(c.seat.avoid_first_rows, 1);
        assert_eq!(c.seat.preferred_rows, vec!["D", "H"]);
        assert!(c.seat.manual_seats.is_empty());
    }

    #[test]
    fn deserialize_device_fields() {
        let c: Config = toml::from_str(MINIMAL).unwrap();
        assert_eq!(c.device.device_id, "dev-001");
        assert_eq!(c.device.longitude, "106.8");
        assert_eq!(c.device.latitude, "-6.2");
    }

    #[test]
    fn deserialize_payment_fields() {
        let c: Config = toml::from_str(MINIMAL).unwrap();
        assert_eq!(c.payment.payment_method, "NETWORK_PAY");
        assert_eq!(c.payment.payment_option, "NETWORK_PAY_PG_QRIS");
    }

    #[test]
    fn deserialize_polling_fields() {
        let c: Config = toml::from_str(MINIMAL).unwrap();
        assert!(!c.polling.enabled);
        assert_eq!(c.polling.interval_secs, 30);
        assert_eq!(c.polling.refresh_token_before_secs, 300);
        assert_eq!(c.polling.start_at, "");
    }

    #[test]
    fn deserialize_with_blocked_ranges_and_blocked_theaters() {
        let toml_str = r#"
[auth]
msisdn = "08123"
password = "pw"

[target]
movie_id = "m1"
city_id = "c1"
date = ""
blocked_datetime_ranges = [["2025-06-01", "2025-06-01"], ["2025-06-02 10:00", "2025-06-02 12:00"]]

[theater]
theater_priority = []
blocked_theaters = ["Cinepolis", "TGV"]

[showtime]
preferred_time_start = ""
preferred_time_end = ""

[seat]
quantity = 1
manual_seats = ["D5"]
avoid_first_rows = 0
preferred_rows = []

[device]
device_id = "d"
longitude = "0"
latitude = "0"

[payment]
payment_method = "M"
payment_option = "O"

[polling]
enabled = true
interval_secs = 5
refresh_token_before_secs = 60
start_at = ""
"#;
        let c: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(c.target.blocked_datetime_ranges.len(), 2);
        assert_eq!(c.target.blocked_datetime_ranges[0], vec!["2025-06-01", "2025-06-01"]);
        assert_eq!(c.theater.blocked_theaters, vec!["Cinepolis", "TGV"]);
        assert_eq!(c.seat.manual_seats, vec!["D5"]);
        assert!(c.polling.enabled);
    }

    #[test]
    fn invalid_toml_returns_err() {
        let result: Result<Config, _> = toml::from_str("this is [[[not valid toml");
        assert!(result.is_err());
    }

    #[test]
    fn config_implements_clone() {
        let c: Config = toml::from_str(MINIMAL).unwrap();
        let c2 = c.clone();
        assert_eq!(c.auth.msisdn, c2.auth.msisdn);
        assert_eq!(c.seat.quantity, c2.seat.quantity);
    }

    #[test]
    fn load_returns_err_when_file_absent() {
        // Verify the error-path of std::fs::read_to_string for a missing file
        let result = std::fs::read_to_string("__no_such_config_file__.toml");
        assert!(result.is_err());
    }
}
