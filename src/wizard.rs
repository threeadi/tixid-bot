use std::io::{self, Write};

use crate::config::{self, Config};

/// Helper to read input from terminal with a default value.
fn prompt(label: &str, current: &str) -> String {
    if current.is_empty() {
        print!("   {} : ", label);
    } else {
        print!("   {} [default: {}]: ", label, current);
    }
    let _ = io::stdout().flush();

    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_ok() {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            current.to_string()
        } else {
            trimmed.to_string()
        }
    } else {
        current.to_string()
    }
}

/// Helper to read boolean input (y/n).
fn prompt_bool(label: &str, current: bool) -> bool {
    let def_str = if current { "Y/n" } else { "y/N" };
    print!("   {} [{}]: ", label, def_str);
    let _ = io::stdout().flush();

    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_ok() {
        let trimmed = input.trim().to_lowercase();
        if trimmed.is_empty() {
            current
        } else if trimmed == "y" || trimmed == "yes" || trimmed == "1" {
            true
        } else if trimmed == "n" || trimmed == "no" || trimmed == "0" {
            false
        } else {
            current
        }
    } else {
        current
    }
}

/// Helper to read numeric input.
fn prompt_usize(label: &str, current: usize) -> usize {
    let current_str = current.to_string();
    let val_str = prompt(label, &current_str);
    val_str.parse().unwrap_or(current)
}

/// Run interactive setup wizard and save configuration to disk.
pub fn run_setup() -> anyhow::Result<Config> {
    println!();
    println!("====================================================");
    println!("⚙️  TIX.ID Bot — Setup Wizard");
    println!("====================================================");
    println!("Tekan ENTER langsung untuk mempertahankan nilai [default].");
    println!();

    // Load existing config if available, otherwise use defaults
    let mut cfg = config::load().unwrap_or_default();

    // ── 1. Auth ───────────────────────────────────────────
    println!("🔑 [1/5] Akun TIX.ID");
    cfg.auth.msisdn = prompt("Nomor HP (+628...)", &cfg.auth.msisdn);
    cfg.auth.password = prompt("Password token (RSA encrypted dari DevTools)", &cfg.auth.password);
    println!();

    // ── 2. Target ─────────────────────────────────────────
    println!("🎯 [2/5] Target Film & Jadwal");
    cfg.target.movie_id = prompt("Movie ID (dari URL / API tix.id)", &cfg.target.movie_id);
    cfg.target.city_id = prompt("City ID (ID Kota)", &cfg.target.city_id);
    cfg.target.date = prompt("Tanggal nonton (YYYY-MM-DD, kosong = otomatis)", &cfg.target.date);
    cfg.showtime.preferred_time_start = prompt("Jam mulai paling awal (contoh: 12:00)", &cfg.showtime.preferred_time_start);
    cfg.showtime.preferred_time_end = prompt("Jam selesai paling akhir (contoh: 22:00)", &cfg.showtime.preferred_time_end);
    println!();

    // ── 3. Kursi ──────────────────────────────────────────
    println!("💺 [3/5] Pemilihan Kursi");
    cfg.seat.quantity = prompt_usize("Jumlah tiket", cfg.seat.quantity);
    cfg.seat.avoid_first_rows = prompt_usize("Lewati N baris terdepan (dekat layar)", cfg.seat.avoid_first_rows);

    let pref_rows_str = cfg.seat.preferred_rows.join(", ");
    let pref_input = prompt("Rentang baris disukai (contoh: D, H)", &pref_rows_str);
    cfg.seat.preferred_rows = pref_input
        .split(',')
        .map(|s| s.trim().to_uppercase())
        .filter(|s| !s.is_empty())
        .collect();
    println!();

    // ── 4. Bioskop ────────────────────────────────────────
    println!("🏢 [4/5] Prioritas Bioskop");
    let priority_str = cfg.theater.theater_priority.join(", ");
    let priority_input = prompt(
        "Urutan bioskop dipisah koma (contoh: TRANSMART XXI, CINEPOLIS, CGV)",
        &priority_str,
    );
    cfg.theater.theater_priority = priority_input
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    println!();

    // ── 5. Notifikasi ─────────────────────────────────────
    println!("🔔 [5/5] Pengaturan Notifikasi");
    cfg.notification.desktop_enabled = prompt_bool("Aktifkan notifikasi pop-up Windows Desktop", cfg.notification.desktop_enabled);
    cfg.notification.desktop_sound = prompt_bool("Aktifkan suara beep/bell terminal", cfg.notification.desktop_sound);
    cfg.notification.discord_enabled = prompt_bool("Kirim notifikasi ke Discord Webhook", cfg.notification.discord_enabled);

    if cfg.notification.discord_enabled {
        cfg.notification.discord_webhook_url = prompt("Discord Webhook URL", &cfg.notification.discord_webhook_url);
        cfg.notification.discord_mention = prompt("Discord Mention (opsional: @everyone, <@USER_ID>)", &cfg.notification.discord_mention);
    }
    println!();

    // Simpan ke config.toml
    config::save(&cfg)?;

    println!("====================================================");
    println!("✅ Pengaturan berhasil disimpan ke: {}", config::get_config_path().display());
    println!("====================================================");
    println!();

    Ok(cfg)
}
