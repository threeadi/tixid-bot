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

/// Menampilkan status ringkas konfigurasi aktif saat ini di layar menu.
pub fn print_config_status(cfg: &Config) {
    let clean_id = crate::bot::clean_movie_id(&cfg.target.movie_id);
    let mode_str = if !clean_id.is_empty() {
        format!("WAR LANGSUNG (Movie ID: {})", clean_id)
    } else if !cfg.target.movie_title.trim().is_empty() {
        format!("BEACONING (Judul: \"{}\" • Cek: {}m)", cfg.target.movie_title.trim(), cfg.polling.beacon_interval_mins)
    } else {
        "(Belum dikonfigurasi)".to_string()
    };

    let date_str = if cfg.target.date.trim().is_empty() {
        "Otomatis (hari pertama)"
    } else {
        cfg.target.date.trim()
    };

    let time_str = format!(
        "{} - {}",
        if cfg.showtime.preferred_time_start.is_empty() { "Awal" } else { &cfg.showtime.preferred_time_start },
        if cfg.showtime.preferred_time_end.is_empty() { "Akhir" } else { &cfg.showtime.preferred_time_end }
    );

    println!("   Status Target : {}", mode_str);
    println!("   Kota & Tanggal: ID {} • {}", cfg.target.city_id, date_str);
    println!("   Preferensi    : {} Tiket • Jam {}", cfg.seat.quantity, time_str);
}

/// Prompt terpadu untuk memilih Mode War Langsung vs Mode Beaconing serta detail target film.
fn prompt_target_mode(cfg: &mut Config) {
    let current_mode = if !crate::bot::clean_movie_id(&cfg.target.movie_id).is_empty() {
        "1"
    } else {
        "2"
    };

    println!("   Pilih Mode Operasi:");
    println!("     [1] Mode War Langsung (Sudah punya Movie ID / Presale aktif)");
    println!("     [2] Mode Beacon (Cari film belum rilis berdasarkan judul)");
    print!("   Pilihan mode [1/2] [default: {}]: ", current_mode);
    let _ = io::stdout().flush();

    let mut mode_input = String::new();
    let _ = io::stdin().read_line(&mut mode_input);
    let mode_choice = mode_input.trim();
    let chosen_mode = if mode_choice.is_empty() { current_mode } else { mode_choice };

    println!();
    if chosen_mode == "2" {
        println!("   📡 Setup Mode Beacon (Pencarian Katalog Otomatis):");
        cfg.target.movie_id = String::new(); // kosongkan ID agar masuk Beacon
        cfg.target.movie_title = prompt("Judul film (contoh: Avenger Doomsday)", &cfg.target.movie_title);
        cfg.polling.beacon_interval_mins = prompt_usize("Interval cek berkala (menit)", cfg.polling.beacon_interval_mins as usize) as u64;
    } else {
        println!("   ⚡ Setup Mode War Langsung:");
        cfg.target.movie_title = String::new(); // kosongkan judul agar langsung war
        let raw_movie_id = prompt("Movie ID (angka dari URL tix.id)", &cfg.target.movie_id);
        cfg.target.movie_id = crate::bot::clean_movie_id(&raw_movie_id);
    }

    cfg.target.city_id = prompt("City ID (ID Kota)", &cfg.target.city_id);
    cfg.target.date = prompt("Tanggal nonton (YYYY-MM-DD, kosong = otomatis)", &cfg.target.date);

    let default_start = if cfg.showtime.preferred_time_start.contains(' ') {
        cfg.showtime.preferred_time_start.split_whitespace().last().unwrap_or("12:00").to_string()
    } else {
        cfg.showtime.preferred_time_start.clone()
    };
    let default_end = if cfg.showtime.preferred_time_end.contains(' ') {
        cfg.showtime.preferred_time_end.split_whitespace().last().unwrap_or("22:00").to_string()
    } else {
        cfg.showtime.preferred_time_end.clone()
    };

    cfg.showtime.preferred_time_start = prompt("Jam mulai paling awal (contoh: 12:00)", &default_start);
    cfg.showtime.preferred_time_end = prompt("Jam selesai paling akhir (contoh: 22:00)", &default_end);
}

/// Menampilkan ringkasan konfigurasi, menyimpan ke disk, dan menanyakan apakah ingin langsung jalan.
fn print_summary_and_confirm(cfg: &Config) -> anyhow::Result<bool> {
    println!();
    println!("----------------------------------------------------");
    println!("📋 Ringkasan Konfigurasi:");
    print_config_status(cfg);
    println!("----------------------------------------------------");

    config::save(cfg)?;
    println!("✅ Pengaturan berhasil disimpan ke: {}", config::get_config_path().display());
    println!();

    print!("Langsung mulai bot sekarang? [Y/n]: ");
    let _ = io::stdout().flush();
    let mut ans = String::new();
    let _ = io::stdin().read_line(&mut ans);
    let trimmed = ans.trim().to_lowercase();
    let run_now = trimmed.is_empty() || trimmed == "y" || trimmed == "yes";

    Ok(run_now)
}

/// Quick Setup: Hanya mengganti target film dan mode operasi tanpa mengulang Akun, Bioskop, Kursi.
pub fn run_quick_setup() -> anyhow::Result<bool> {
    println!();
    println!("====================================================");
    println!("🎯 Quick Setup — Target Film & Mode");
    println!("====================================================");
    println!("Tekan ENTER langsung untuk mempertahankan nilai [default].");
    println!();

    let mut cfg = config::load().unwrap_or_default();
    prompt_target_mode(&mut cfg);

    print_summary_and_confirm(&cfg)
}

/// Full Setup Wizard: Mengatur seluruh konfigurasi dari Akun sampai Notifikasi secara lengkap.
pub fn run_setup() -> anyhow::Result<bool> {
    println!();
    println!("====================================================");
    println!("⚙️  TIX.ID Bot — Full Setup Wizard");
    println!("====================================================");
    println!("Tekan ENTER langsung untuk mempertahankan nilai [default].");
    println!();

    let mut cfg = config::load().unwrap_or_default();

    // ── 1. Auth ───────────────────────────────────────────
    println!("🔑 [1/5] Akun TIX.ID");
    cfg.auth.msisdn = prompt("Nomor HP (+628...)", &cfg.auth.msisdn);
    cfg.auth.password = prompt("Password token (RSA encrypted dari DevTools)", &cfg.auth.password);
    println!();

    // ── 2. Target ─────────────────────────────────────────
    println!("🎯 [2/5] Target Film & Mode");
    prompt_target_mode(&mut cfg);
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
        println!("   💡 Tip Mention: Discord Webhook butuh ID Pengguna (bukan @username biasa).");
        println!("      Gunakan: <@USER_ID>, angka ID, @everyone, atau @here.");
        cfg.notification.discord_mention = prompt("Discord Mention (opsional)", &cfg.notification.discord_mention);
    }

    print_summary_and_confirm(&cfg)
}
