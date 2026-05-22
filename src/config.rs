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
}

#[derive(Debug, Deserialize, Clone)]
pub struct TheaterConfig {
    pub theater_priority: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ShowtimeConfig {
    pub preferred_time_start: String,
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
