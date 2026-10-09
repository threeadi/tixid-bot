use std::path::PathBuf;
use std::time::SystemTime;

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Config {
    pub auth: AuthConfig,
    pub target: TargetConfig,
    pub theater: TheaterConfig,
    pub showtime: ShowtimeConfig,
    pub seat: SeatConfig,
    pub device: DeviceConfig,
    pub payment: PaymentConfig,
    pub polling: PollingConfig,
    #[serde(default)]
    pub notification: NotificationConfig,
    #[serde(default)]
    pub debug: DebugConfig,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct DebugConfig {
    /// Tampilkan rincian metrik timing HTTP per request di terminal (RTT, Server Upstream, Download, Parse)
    #[serde(default)]
    pub verbose_timing: bool,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AuthConfig {
    pub msisdn: String,
    pub password: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TargetConfig {
    #[serde(default)]
    pub movie_id: String,
    #[serde(default)]
    pub movie_title: String,
    pub city_id: String,
    pub date: String,
    /// Datetime ranges to skip. Each entry is ["YYYY-MM-DD HH:MM", "YYYY-MM-DD HH:MM"] (inclusive).
    /// Date-only format ["YYYY-MM-DD", "YYYY-MM-DD"] is also accepted (treated as 00:00–23:59).
    #[serde(default)]
    pub blocked_datetime_ranges: Vec<Vec<String>>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TheaterConfig {
    pub theater_priority: Vec<String>,
    /// Theaters to always skip (substring match, case-insensitive).
    #[serde(default)]
    pub blocked_theaters: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ShowtimeConfig {
    /// "HH:MM" or "YYYY-MM-DD HH:MM". Empty = no bound.
    pub preferred_time_start: String,
    /// "HH:MM" or "YYYY-MM-DD HH:MM". Empty = no bound.
    pub preferred_time_end: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SeatConfig {
    pub quantity: usize,
    pub manual_seats: Vec<String>,
    pub avoid_first_rows: usize,
    /// Two-element vec: [start_row, end_row] (e.g. ["D", "H"])
    pub preferred_rows: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DeviceConfig {
    pub device_id: String,
    pub longitude: String,
    pub latitude: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PaymentConfig {
    /// e.g. "NETWORK_PAY"
    pub payment_method: String,
    /// e.g. "NETWORK_PAY_PG_QRIS"
    pub payment_option: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PollingConfig {
    /// Keep polling instead of failing immediately when schedule is not ready.
    pub enabled: bool,
    /// Seconds between retries when movie schedule is not yet available
    pub interval_secs: u64,
    /// Re-login after this many seconds while waiting in polling loop
    pub refresh_token_before_secs: u64,
    /// Optional WIB time ("YYYY-MM-DD HH:MM:SS") before polling starts
    pub start_at: String,
    /// Minutes between periodic checks when searching for a movie in Beacon mode
    #[serde(default = "default_beacon_interval_mins")]
    pub beacon_interval_mins: u64,
}

fn default_beacon_interval_mins() -> u64 {
    15
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct NotificationConfig {
    /// Show native desktop notification (Windows toast)
    #[serde(default = "default_true")]
    pub desktop_enabled: bool,
    /// Play terminal bell sound upon checkout
    #[serde(default = "default_true")]
    pub desktop_sound: bool,
    /// Send notification to Discord webhook
    #[serde(default)]
    pub discord_enabled: bool,
    /// Discord webhook URL
    #[serde(default)]
    pub discord_webhook_url: String,
    /// Optional Discord mention string (e.g. "@everyone", "<@USER_ID>", "<@&ROLE_ID>")
    #[serde(default)]
    pub discord_mention: String,
}

fn default_true() -> bool {
    true
}

impl Default for NotificationConfig {
    fn default() -> Self {
        Self {
            desktop_enabled: true,
            desktop_sound: true,
            discord_enabled: false,
            discord_webhook_url: String::new(),
            discord_mention: String::new(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            auth: AuthConfig {
                msisdn: String::new(),
                password: String::new(),
            },
            target: TargetConfig {
                movie_id: String::new(),
                movie_title: String::new(),
                city_id: String::new(),
                date: String::new(),
                blocked_datetime_ranges: Vec::new(),
            },
            theater: TheaterConfig {
                theater_priority: Vec::new(),
                blocked_theaters: Vec::new(),
            },
            showtime: ShowtimeConfig {
                preferred_time_start: String::new(),
                preferred_time_end: String::new(),
            },
            seat: SeatConfig {
                quantity: 2,
                manual_seats: Vec::new(),
                avoid_first_rows: 3,
                preferred_rows: vec!["D".to_string(), "H".to_string()],
            },
            device: DeviceConfig {
                device_id: "019e4e4e-f638-7fca-b747-69e48a9eef32".to_string(),
                longitude: "112.76613449641758".to_string(),
                latitude: "-8.210285710686566".to_string(),
            },
            payment: PaymentConfig {
                payment_method: "NETWORK_PAY".to_string(),
                payment_option: "NETWORK_PAY_PG_QRIS".to_string(),
            },
            polling: PollingConfig {
                enabled: true,
                interval_secs: 2,
                refresh_token_before_secs: 1500,
                start_at: String::new(),
                beacon_interval_mins: 15,
            },
            notification: NotificationConfig::default(),
            debug: DebugConfig::default(),
        }
    }
}

/// Menentukan lokasi config.toml secara dinamis (mengutamakan folder yang sama dengan binary .exe).
pub fn get_config_path() -> PathBuf {
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(parent) = exe_path.parent() {
            let candidate = parent.join("config.toml");
            if candidate.exists() {
                return candidate;
            }
        }
    }

    let cwd_candidate = PathBuf::from("config.toml");
    if cwd_candidate.exists() {
        return cwd_candidate;
    }

    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(parent) = exe_path.parent() {
            return parent.join("config.toml");
        }
    }

    PathBuf::from("config.toml")
}

/// Cek apakah config.toml ada.
pub fn config_exists() -> bool {
    get_config_path().exists()
}

pub fn load() -> Result<Config> {
    let path = get_config_path();
    let text = std::fs::read_to_string(&path)
        .map_err(|e| anyhow::anyhow!("Cannot read {}: {}", path.display(), e))?;
    let config: Config = toml::from_str(&text)
        .map_err(|e| anyhow::anyhow!("Cannot parse {}: {}", path.display(), e))?;
    Ok(config)
}

pub fn save(config: &Config) -> Result<()> {
    let path = get_config_path();
    let text = toml::to_string_pretty(config)
        .map_err(|e| anyhow::anyhow!("Cannot serialize config: {}", e))?;
    std::fs::write(&path, text)
        .map_err(|e| anyhow::anyhow!("Cannot write {}: {}", path.display(), e))?;
    Ok(())
}

/// Updates `movie_id = "..."` under `[target]` in `config.toml` on disk without destroying comments.
pub fn update_movie_id_in_file(path: &std::path::Path, new_movie_id: &str) -> Result<()> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(error = %e, path = %path.display(), "cannot read config file to update movie_id");
            return Ok(());
        }
    };

    let mut in_target = false;
    let mut updated = false;
    let mut new_lines = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_target = trimmed == "[target]";
        }

        if in_target && (trimmed.starts_with("movie_id") || trimmed.starts_with("movie_id ")) {
            new_lines.push(format!("movie_id = \"{}\"", new_movie_id));
            updated = true;
        } else {
            new_lines.push(line.to_string());
        }
    }

    if updated {
        let mut out = new_lines.join("\n");
        if content.ends_with('\n') {
            out.push('\n');
        }
        std::fs::write(path, out)?;
        tracing::info!(path = %path.display(), movie_id = new_movie_id, "updated movie_id in config.toml");
    } else {
        // Fallback: load, modify, and serialize
        if let Ok(mut cfg) = toml::from_str::<Config>(&content) {
            cfg.target.movie_id = new_movie_id.to_string();
            let text = toml::to_string_pretty(&cfg)?;
            std::fs::write(path, text)?;
            tracing::info!(path = %path.display(), movie_id = new_movie_id, "updated movie_id via fallback serialization");
        }
    }

    Ok(())
}

/// Memeriksa apakah config.toml telah dimodifikasi di disk.
/// Jika ya dan parsing berhasil, memperbarui `current_config` dan mengembalikan `true`.
pub fn check_and_reload(
    last_modified: &mut Option<SystemTime>,
    current_config: &mut Config,
) -> bool {
    let path = get_config_path();
    let metadata = match std::fs::metadata(&path) {
        Ok(m) => m,
        Err(_) => return false,
    };

    let modified = match metadata.modified() {
        Ok(t) => t,
        Err(_) => return false,
    };

    if let Some(prev) = *last_modified {
        if modified <= prev {
            return false;
        }
    } else {
        *last_modified = Some(modified);
        return false;
    }

    match std::fs::read_to_string(&path) {
        Ok(text) => {
            match toml::from_str::<Config>(&text) {
                Ok(new_cfg) => {
                    *current_config = new_cfg;
                    *last_modified = Some(modified);
                    println!(
                        "\n🔄 [Hot-Reload] config.toml telah diperbarui dan berhasil dimuat ulang!"
                    );
                    tracing::info!("config.toml hot-reloaded successfully");
                    true
                }
                Err(e) => {
                    println!("\n⚠️  [Hot-Reload] Perubahan pada config.toml memiliki kesalahan sintaks: {}", e);
                    println!("    Menggunakan konfigurasi valid sebelumnya tanpa berhenti.");
                    tracing::warn!(error = %e, "config.toml reload failed due to syntax error");
                    *last_modified = Some(modified);
                    false
                }
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, "Failed to read modified config.toml");
            false
        }
    }
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
    fn deserialize_notification_defaults() {
        let c: Config = toml::from_str(MINIMAL).unwrap();
        assert!(c.notification.desktop_enabled);
        assert!(c.notification.desktop_sound);
        assert!(!c.notification.discord_enabled);
        assert_eq!(c.notification.discord_webhook_url, "");
        assert_eq!(c.notification.discord_mention, "");
    }

    #[test]
    fn deserialize_notification_custom_fields() {
        let toml_str = r#"
[auth]
msisdn = "08123"
password = "pw"

[target]
movie_id = "m1"
city_id = "c1"
date = ""

[theater]
theater_priority = []

[showtime]
preferred_time_start = ""
preferred_time_end = ""

[seat]
quantity = 1
manual_seats = []
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
enabled = false
interval_secs = 5
refresh_token_before_secs = 60
start_at = ""

[notification]
desktop_enabled = false
desktop_sound = false
discord_enabled = true
discord_webhook_url = "https://discord.com/api/webhooks/test"
discord_mention = "<@123456789>"
"#;
        let c: Config = toml::from_str(toml_str).unwrap();
        assert!(!c.notification.desktop_enabled);
        assert!(!c.notification.desktop_sound);
        assert!(c.notification.discord_enabled);
        assert_eq!(
            c.notification.discord_webhook_url,
            "https://discord.com/api/webhooks/test"
        );
        assert_eq!(c.notification.discord_mention, "<@123456789>");
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
        assert!(!c.debug.verbose_timing);
    }

    #[test]
    fn deserialize_debug_fields() {
        let toml_str = r#"
[auth]
msisdn = "08123"
password = "pw"

[target]
city_id = "c1"
date = ""

[theater]
theater_priority = []

[showtime]
preferred_time_start = ""
preferred_time_end = ""

[seat]
quantity = 1
manual_seats = []
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
enabled = false
interval_secs = 5
refresh_token_before_secs = 60
start_at = ""

[debug]
verbose_timing = true
"#;
        let c: Config = toml::from_str(toml_str).unwrap();
        assert!(c.debug.verbose_timing);
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
        assert_eq!(
            c.target.blocked_datetime_ranges[0],
            vec!["2025-06-01", "2025-06-01"]
        );
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

    #[test]
    fn config_default_has_expected_values() {
        let def = Config::default();
        assert_eq!(def.seat.quantity, 2);
        assert_eq!(def.seat.avoid_first_rows, 3);
        assert_eq!(def.payment.payment_option, "NETWORK_PAY_PG_QRIS");
        assert!(def.notification.desktop_enabled);
        assert!(def.notification.desktop_sound);
    }

    #[test]
    fn test_beacon_config_deserialization_and_update() {
        let toml_str = r#"
[auth]
msisdn = "+6281234567890"
password = "secret_password"

[target]
movie_id = ""
movie_title = "Avengers: Doomsday"
city_id = "973818515335155712"
date = "2026-10-10"

[theater]
theater_priority = ["ARAYA XXI"]

[showtime]
preferred_time_start = "12:00"
preferred_time_end = "21:00"

[seat]
quantity = 2
manual_seats = []
avoid_first_rows = 2
preferred_rows = ["C", "F"]

[device]
device_id = "test-device"
longitude = "112.0"
latitude = "-8.0"

[payment]
payment_method = "NETWORK_PAY"
payment_option = "NETWORK_PAY_PG_QRIS"

[polling]
enabled = true
interval_secs = 2
refresh_token_before_secs = 1500
start_at = ""
beacon_interval_mins = 20
"#;
        let c: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(c.target.movie_id, "");
        assert_eq!(c.target.movie_title, "Avengers: Doomsday");
        assert_eq!(c.polling.beacon_interval_mins, 20);

        // Test update_movie_id_in_file
        let temp_dir = std::env::temp_dir();
        let temp_file = temp_dir.join(format!("test_config_{}.toml", uuid::Uuid::new_v4()));
        std::fs::write(&temp_file, toml_str).unwrap();

        update_movie_id_in_file(&temp_file, "999888777").unwrap();
        let reloaded = std::fs::read_to_string(&temp_file).unwrap();
        assert!(reloaded.contains("movie_id = \"999888777\""));
        assert!(reloaded.contains("movie_title = \"Avengers: Doomsday\""));

        let parsed: Config = toml::from_str(&reloaded).unwrap();
        assert_eq!(parsed.target.movie_id, "999888777");

        let _ = std::fs::remove_file(temp_file);
    }
}
