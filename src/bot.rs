use std::collections::HashSet;
use std::io::Write;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use chrono::{FixedOffset, NaiveDateTime, TimeZone, Utc};
use qrcode::{QrCode, render::unicode};
use reqwest::Client;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::sleep;

use crate::{api, beacon, client, config, notifier, seat_selector, theater_selector};

struct AuthSession {
    http: Client,
    user_name: String,
    refresh_token: String,
    authenticated_at: Instant,
}

pub async fn run() -> Result<()> {
    tracing::info!("bot starting");
    println!("🎬 TIX.ID Bot [v{}]", env!("CARGO_PKG_VERSION"));
    println!("========================================");

    // ── 1. Load config ───────────────────────────────────────────────────────
    let mut cfg = config::load()?;
    if cfg.debug.verbose_timing {
        api::set_verbose_timing(true);
    }

    if api::is_verbose_timing() {
        print!("📡 Mengukur latensi koneksi (Ping TCP ke api-b2b.tix.id)...");
        let _ = std::io::stdout().flush();
        match api::probe_network_rtt().await {
            Ok(rtt) => {
                let rtt_ms = rtt.as_millis();
                let quality = if rtt_ms < 35 {
                    "Sangat Cepat 🚀"
                } else if rtt_ms < 85 {
                    "Bagus & Stabil 👍"
                } else {
                    "Tinggi / Berpotensi Lambat ⚠️"
                };
                println!("\r📡 Base Network Ping (TCP RTT): {}ms ({})          ", rtt_ms, quality);
            }
            Err(e) => {
                println!("\r⚠️  Gagal mengukur TCP ping: {}          ", e);
            }
        }
        let metrics = crate::metrics::sample_metrics();
        println!("💻 Bot System Footprint        : RAM: {:.1} MB • CPU: {:.1}%", metrics.ram_mb, metrics.cpu_percent);
    }

    // ── 2. Pre-Authentication (Strategi A) ──────────────────────────────────
    // Login SEBELUM standby agar token dan koneksi TCP/TLS sudah siap saat war!
    let mut auth = authenticate(&cfg).await?;

    // ── 3. Resolve Target Movie (Beacon Mode vs Direct Movie ID) ────────────
    let clean_id = clean_movie_id(&cfg.target.movie_id);
    let mut movie = if clean_id.is_empty() {
        if cfg.target.movie_title.trim().is_empty() {
            return Err(anyhow::anyhow!(
                "movie_id dan movie_title kosong! Harap isi target.movie_id atau target.movie_title di config.toml."
            ));
        }
        run_beacon_mode(&mut cfg, &mut auth).await?
    } else {
        print!("🎥 Pre-fetching movie {}...", clean_id);
        let m = api::get_movie(&auth.http, &clean_id).await?;
        if m.id.trim().is_empty() {
            return Err(anyhow::anyhow!(
                "Film tidak ditemukan di TIX ID (ID '{}' tidak menghasilkan data). Pastikan movie_id berupa ID film yang benar (contoh: 2093187333460410368).",
                cfg.target.movie_id
            ));
        }
        m
    };

    println!(
        "\r✅ {} ({} min, {})               ",
        movie.name, movie.duration, movie.status
    );

    // ── 4. Standby jika polling.start_at disetel (Pre-authenticated & Warm) ──
    wait_until_start_with_keepalive(&cfg, &mut auth, &cfg.polling.start_at).await?;

    // ── 5. Poll until movie showtime is ready (Bypass redundant metadata) ────
    let (target_date, ranked, strike_start) =
        wait_for_target_showtime(&mut cfg, &mut auth, &mut movie).await?;

    // ── 6, 7 & 8. Find theater & lock seats (with Auto Seat Re-Roll on Conflict) ──
    let (_selected, order, _seats, seat_lock_elapsed) = lock_order_with_reroll(
        &auth.http,
        &cfg,
        &ranked,
        &target_date,
        &movie,
        strike_start,
    )
    .await?;

    // Format expiry time in WIB (UTC+7)
    let wib = FixedOffset::east_opt(7 * 3600).unwrap();
    let expiry_str = Utc
        .timestamp_opt(order.expired_at, 0)
        .single()
        .map(|dt| dt.with_timezone(&wib).format("%H:%M:%S").to_string())
        .unwrap_or_else(|| format!("(ts={})", order.expired_at));

    let price_per = if order.quantity > 0 {
        order.total_ticket_price / order.quantity as i64
    } else {
        0
    };

    println!();
    println!("========================================");
    println!("🎉 ORDER SUCCESSFUL! (SEATS LOCKED)");
    println!("========================================");
    println!("   Order ID:  {}", order.id);
    println!("   Movie:     {}", order.movie_name);
    println!("   Theater:   {} | {}", order.theater_name, order.studio_name);
    println!("   Seats:     {}", order.selected_seats.join(", "));
    println!(
        "   Ticket:    {} x Rp{} = Rp{}",
        order.quantity,
        fmt_rupiah(price_per),
        fmt_rupiah(order.total_ticket_price)
    );
    println!("   Fee:       Rp{}", fmt_rupiah(order.convenience_fee));
    println!("   TOTAL:     Rp{}", fmt_rupiah(order.total));
    println!("   Expires:   {} WIB", expiry_str);
    println!("========================================");
    tracing::info!(
        order_id = %order.id,
        movie = %order.movie_name,
        theater = %order.theater_name,
        studio = %order.studio_name,
        seats = %order.selected_seats.join(", "),
        quantity = order.quantity,
        ticket_price = order.total_ticket_price,
        fee = order.convenience_fee,
        total = order.total,
        expires_at = order.expired_at,
        seat_lock_ms = seat_lock_elapsed.as_millis(),
        "order created (seats locked)"
    );

    // ── 9. Checkout ──────────────────────────────────────────────────────────
    print!("💳 Checking out with {}...", cfg.payment.payment_option);
    let payment = api::checkout(
        &auth.http,
        &order.id,
        &cfg.device.latitude,
        &cfg.device.longitude,
        &cfg.payment.payment_method,
        &cfg.payment.payment_option,
    )
    .await?;
    println!("\r✅ Checkout OK                               ");

    let total_elapsed = strike_start.elapsed();
    let seat_lock_str = format!("{:.3}s", seat_lock_elapsed.as_secs_f64());
    let total_str = format!("{:.3}s", total_elapsed.as_secs_f64());

    tracing::info!(
        order_id = %order.id,
        payment_option = %payment.payment_option,
        total = payment.total_payment,
        payment_code = %payment.payment_code,
        seat_lock_ms = seat_lock_elapsed.as_millis(),
        total_ms = total_elapsed.as_millis(),
        "checkout completed"
    );
    println!();
    println!("========================================");
    println!("💳 PAYMENT INFO");
    println!("========================================");
    println!("   Method:    {}", payment.payment_option);
    println!("   Amount:    Rp{}", fmt_rupiah(payment.total_payment));
    println!("   ⚡ Seat Lock Time: {} (Waktu Penguncian Kursi)", seat_lock_str);
    println!("   ⏱️  Total Checkout: {} (Termasuk QRIS Gateway)", total_str);
    if !payment.checkout_url.is_empty() {
        println!("   URL:       {}", payment.checkout_url);
    }
    if !payment.payment_code.is_empty() {
        println!("   QRIS Code:");
        println!();
        println!("{}", payment.payment_code);
        println!();
        
        let qr_url = format!(
            "https://api.qrserver.com/v1/create-qr-code/?size=300x300&data={}",
            urlencoding::encode(&payment.payment_code)
        );
        println!("   QR URL:    {}", qr_url);
        println!();
        
        render_qr_terminal(&payment.payment_code);
        println!();
        println!("   ⚠️  Scan QRIS above before {} WIB", expiry_str);
    }
    println!("========================================");

    let qr_url = if !payment.payment_code.is_empty() {
        format!(
            "https://api.qrserver.com/v1/create-qr-code/?size=300x300&data={}",
            urlencoding::encode(&payment.payment_code)
        )
    } else {
        String::new()
    };

    let notif_payload = notifier::OrderNotificationPayload {
        order_id: order.id,
        movie_name: order.movie_name,
        theater_name: order.theater_name,
        studio_name: order.studio_name,
        selected_seats: order.selected_seats,
        quantity: order.quantity,
        ticket_price: order.total_ticket_price,
        convenience_fee: order.convenience_fee,
        total_payment: payment.total_payment,
        payment_option: payment.payment_option,
        expired_at_wib: expiry_str,
        payment_code: payment.payment_code,
        qr_image_url: qr_url,
    };

    notifier::notify_checkout(&cfg.notification, &notif_payload).await;

    Ok(())
}

async fn authenticate(cfg: &config::Config) -> Result<AuthSession> {
    print!("🔑 Getting guest token...");
    let anon = client::build(None, &cfg.device.device_id)?;
    let guest = api::get_guest_token(&anon).await?;
    println!("\r✅ Guest token OK (expires in {}min)     ", guest.expires_in);

    print!("🔑 Logging in as {}...", cfg.auth.msisdn);
    let guest_http = client::build(Some(&guest.token), &cfg.device.device_id)?;
    let login = api::login(&guest_http, &cfg.auth.msisdn, &cfg.auth.password).await?;
    println!("\r✅ Welcome, {}!                     ", login.name);
    tracing::info!(user = %login.name, msisdn = %cfg.auth.msisdn, "login ok");

    Ok(AuthSession {
        http: client::build(Some(&login.token), &cfg.device.device_id)?,
        user_name: login.name,
        refresh_token: login.refresh_token.unwrap_or_default(),
        authenticated_at: Instant::now(),
    })
}

async fn refresh_auth_if_needed(cfg: &config::Config, auth: &mut AuthSession) -> Result<()> {
    if auth.authenticated_at.elapsed() < Duration::from_secs(cfg.polling.refresh_token_before_secs)
    {
        return Ok(());
    }

    // Prefer refresh token endpoint (no password needed, lighter call)
    if !auth.refresh_token.is_empty() {
        println!("\n♻️  Refreshing token for {}...", auth.user_name);
        tracing::info!(user = %auth.user_name, "refreshing auth token via refresh_token endpoint");
        let refresh_client = client::build(Some(&auth.refresh_token), &cfg.device.device_id)?;
        match api::refresh_user_token(&refresh_client).await {
            Ok(refreshed) => {
                auth.http = client::build(Some(&refreshed.token), &cfg.device.device_id)?;
                auth.refresh_token = refreshed.refresh_token;
                auth.authenticated_at = Instant::now();
                println!("✅ Token refreshed OK                  ");
                tracing::info!(user = %auth.user_name, "token refreshed ok");
                return Ok(());
            }
            Err(e) => {
                tracing::warn!(user = %auth.user_name, error = %e, "refresh token failed, falling back to full re-login");
            }
        }
    }

    // Fallback: full re-login with credentials
    println!("\n♻️  Re-logging in as {}...", auth.user_name);
    tracing::info!(user = %auth.user_name, "re-authenticating via full login");
    *auth = authenticate(cfg).await?;
    Ok(())
}

#[allow(dead_code)]
async fn wait_until_start(start_at: &str) -> Result<()> {
    let start_at = start_at.trim();
    if start_at.is_empty() {
        return Ok(());
    }

    let naive = NaiveDateTime::parse_from_str(start_at, "%Y-%m-%d %H:%M:%S")
        .with_context(|| format!("Invalid polling.start_at format: {}", start_at))?;
    let wib = wib();
    let start = wib
        .from_local_datetime(&naive)
        .single()
        .ok_or_else(|| anyhow::anyhow!("Cannot resolve polling.start_at in WIB: {}", start_at))?;
    let now = Utc::now().with_timezone(&wib);

    if start > now {
        let wait_secs = (start - now).num_seconds().max(0) as u64;
        println!(
            "⏳ Standby until {} WIB ({}s)",
            start.format("%Y-%m-%d %H:%M:%S"),
            wait_secs
        );
        sleep(Duration::from_secs(wait_secs)).await;
    }

    Ok(())
}

/// Standby until `polling.start_at` with pre-authenticated warm connection and auto token refresh.
async fn wait_until_start_with_keepalive(
    cfg: &config::Config,
    auth: &mut AuthSession,
    start_at: &str,
) -> Result<()> {
    let start_at = start_at.trim();
    if start_at.is_empty() {
        return Ok(());
    }

    let naive = NaiveDateTime::parse_from_str(start_at, "%Y-%m-%d %H:%M:%S")
        .with_context(|| format!("Invalid polling.start_at format: {}", start_at))?;
    let wib = wib();
    let start = wib
        .from_local_datetime(&naive)
        .single()
        .ok_or_else(|| anyhow::anyhow!("Cannot resolve polling.start_at in WIB: {}", start_at))?;
    let now = Utc::now().with_timezone(&wib);

    if start > now {
        let wait_secs = (start - now).num_seconds().max(0) as u64;
        println!(
            "⏳ Standby until {} WIB ({}s) [Pre-authenticated & Warm Socket]...",
            start.format("%Y-%m-%d %H:%M:%S"),
            wait_secs
        );
        while Utc::now().with_timezone(&wib) < start {
            let remaining = (start - Utc::now().with_timezone(&wib)).num_seconds().max(0) as u64;
            let sleep_step = remaining.min(5);
            sleep(Duration::from_secs(sleep_step)).await;
            refresh_auth_if_needed(cfg, auth).await?;
        }
        println!("🚀 Waktu war telah tiba! Memulai serangan tiket...");
    }

    Ok(())
}

/// Beacon Mode: Periodically discovers movie by title from TIX.ID catalog,
/// resolves `movie_id`, updates `config.toml`, alerts the user, and monitors
/// for showtime/presale availability.
async fn run_beacon_mode(
    cfg: &mut config::Config,
    auth: &mut AuthSession,
) -> Result<crate::models::MovieData> {
    let query_title = cfg.target.movie_title.trim().to_string();
    println!("📡 Mode Beaconing / Reckoning Aktif");
    println!("   Target Judul : \"{}\"", query_title);
    println!("   Interval Cek : {} menit", cfg.polling.beacon_interval_mins);
    println!("========================================");
    tracing::info!(query = %query_title, interval_mins = cfg.polling.beacon_interval_mins, "starting beacon discovery mode");

    let mut last_modified = None;
    let beacon_step_secs = (cfg.polling.beacon_interval_mins * 60).max(10);

    loop {
        config::check_and_reload(&mut last_modified, cfg);

        // If user manually set movie_id in config.toml during live reload
        let clean_id = clean_movie_id(&cfg.target.movie_id);
        if !clean_id.is_empty() {
            println!("\r✅ Live reload: movie_id terisi manual ({})", clean_id);
            let movie = api::get_movie(&auth.http, &clean_id).await?;
            if !movie.id.trim().is_empty() {
                return Ok(movie);
            }
        }

        refresh_auth_if_needed(cfg, auth).await?;

        print!("📡 [BEACON] Mencari \"{}\" di katalog TIX.ID...", query_title);
        let _ = std::io::stdout().flush();

        match beacon::fetch_catalog(&auth.http).await {
            Ok(candidates) => {
                if let Some((best, score)) = beacon::find_best_match(&candidates, &query_title, beacon::DEFAULT_MATCH_THRESHOLD) {
                    println!(
                        "\r✅ [BEACON] Film Ditemukan: \"{}\" (ID: {}, Kemiripan: {:.0}%)",
                        best.display_name, best.id, score * 100.0
                    );
                    tracing::info!(
                        matched = %best.display_name,
                        movie_id = %best.id,
                        similarity = score,
                        "beacon movie match found"
                    );

                    // Fetch complete metadata from TIX ID
                    let movie = api::get_movie(&auth.http, &best.id).await?;
                    if !movie.id.trim().is_empty() {
                        // Persist movie_id back into config.toml
                        cfg.target.movie_id = best.id.clone();
                        let cfg_path = config::get_config_path();
                        let _ = config::update_movie_id_in_file(&cfg_path, &best.id);

                        // Check whether schedules are already open
                        let dates = api::get_schedule_dates(&auth.http, &movie.id, &cfg.target.city_id).await.ok().flatten();
                        let has_active_schedule = dates.as_ref().map(|d| d.iter().any(|s| s.is_any_schedule)).unwrap_or(false);

                        if has_active_schedule {
                            notifier::notify_beacon_found(
                                &cfg.notification,
                                &query_title,
                                &movie.name,
                                &movie.id,
                                score,
                                &movie.status,
                                "Jadwal tayang aktif! Melanjutkan ke proses War Checkout..."
                            ).await;
                            return Ok(movie);
                        } else {
                            // Status UPCOMING or no schedules yet
                            notifier::notify_beacon_found(
                                &cfg.notification,
                                &query_title,
                                &movie.name,
                                &movie.id,
                                score,
                                &movie.status,
                                &format!("Jadwal belum buka. Memantau pembukaan tiket setiap {} menit...", cfg.polling.beacon_interval_mins)
                            ).await;

                            println!(
                                "⏳ [BEACON] Menunggu jadwal tayang/presale dibuka (cek setiap {} menit)...",
                                cfg.polling.beacon_interval_mins
                            );

                            // Schedule monitoring loop
                            loop {
                                config::check_and_reload(&mut last_modified, cfg);
                                refresh_auth_if_needed(cfg, auth).await?;

                                if let Ok(Some(dates)) = api::get_schedule_dates(&auth.http, &movie.id, &cfg.target.city_id).await {
                                    if let Some(target_d) = pick_target_date(cfg, &dates) {
                                        println!("\n⚡ [BEACON] Jadwal Presale Terbuka untuk {} ({})!", movie.name, target_d);
                                        notifier::notify_beacon_presale_opened(
                                            &cfg.notification,
                                            &movie.name,
                                            &movie.id,
                                            &target_d,
                                        ).await;
                                        return Ok(movie);
                                    }
                                }

                                // Sleep before next schedule probe with keepalive
                                let steps = beacon_step_secs / 5;
                                for _ in 0..steps {
                                    sleep(Duration::from_secs(5)).await;
                                    config::check_and_reload(&mut last_modified, cfg);
                                }
                            }
                        }
                    }
                } else {
                    if api::is_verbose_timing() {
                        let chip = crate::metrics::format_metrics_chip();
                        println!("\r⏳ [BEACON] Belum terdaftar di katalog. Coba lagi dalam {} menit... [{}]", cfg.polling.beacon_interval_mins, chip);
                    } else {
                        println!("\r⏳ [BEACON] Belum terdaftar di katalog. Coba lagi dalam {} menit...", cfg.polling.beacon_interval_mins);
                    }
                }
            }
            Err(e) => {
                println!("\r⚠️  [BEACON] Gagal mengambil katalog: {}. Coba lagi dalam {} menit...", e, cfg.polling.beacon_interval_mins);
                tracing::warn!(error = %e, "beacon catalog fetch failed");
            }
        }

        // Sleep before retrying catalog check with keepalive
        let steps = beacon_step_secs / 5;
        for _ in 0..steps {
            sleep(Duration::from_secs(5)).await;
            config::check_and_reload(&mut last_modified, cfg);
        }
    }
}

/// Fire ALL seat layout requests in parallel, then resolve to the highest-ranked
/// theater that has N consecutive seats.
///
/// Signaling logic:
///   - Each spawned task sends `(rank, Option<(layout, seats)>)` on a channel.
///   - As results arrive, we check: "do we have seats at rank R, AND have ALL ranks
///     0..R already responded (without seats)?" → that's the confirmed winner.
///   - The moment a winner is confirmed, all remaining in-flight JoinHandles are
///     aborted — cancelling their HTTP requests immediately.
async fn try_theaters_for_seats(
    http: &Client,
    cfg: &config::Config,
    ranked: &[theater_selector::SelectedShowtime],
) -> Result<(theater_selector::SelectedShowtime, crate::models::SeatLayoutData, Vec<crate::models::SelectedSeat>)> {
    if ranked.is_empty() {
        return Err(anyhow::anyhow!("No theaters available"));
    }

    let n = ranked.len();

    println!("\n💺 Checking {} theater(s) for {} consecutive seats (parallel)...", n, cfg.seat.quantity);

    // Channel: (rank, payload) — payload is None when no seats or error
    let (tx, mut rx) = mpsc::unbounded_channel::<(usize, Option<(crate::models::SeatLayoutData, Vec<crate::models::SelectedSeat>)>)>();

    // Spawn all seat layout requests simultaneously
    let handles: Vec<JoinHandle<()>> = ranked
        .iter()
        .enumerate()
        .map(|(rank, candidate)| {
            let tx = tx.clone();
            let http = http.clone();
            let merchant_slug = candidate.merchant_slug.clone();
            let showtime_id = candidate.showtime.id.clone();
            let theater_name = candidate.theater.name.clone();
            let seat_config = cfg.seat.clone();
            tokio::spawn(async move {
                tracing::debug!(rank, theater = %theater_name, "fetching seat layout (parallel)");
                let payload = match api::get_seat_layout(&http, &merchant_slug, &showtime_id).await {
                    Ok(layout) => {
                        let seats = seat_selector::select(&layout.seat_map, &seat_config);
                        seats.map(|s| (layout, s))
                    }
                    Err(e) => {
                        tracing::warn!(rank, theater = %theater_name, error = %e, "seat layout fetch failed");
                        None
                    }
                };
                let _ = tx.send((rank, payload));
            })
        })
        .collect();
    drop(tx); // drop original; channel closes when all tasks finish

    // Indexed by rank: None = not yet responded, Some(None) = responded, no seats,
    // Some(Some(_)) = responded with seats
    let mut received: Vec<Option<Option<(crate::models::SeatLayoutData, Vec<crate::models::SelectedSeat>)>>> =
        (0..n).map(|_| None).collect();
    let mut responded_count = 0;

    while let Some((rank, payload)) = rx.recv().await {
        let theater_name = &ranked[rank].theater.name;
        let display_time = &ranked[rank].showtime.display_time;

        if payload.is_some() {
            println!(
                "  ✅ Rank#{} {} ({}) — {} consecutive seats available",
                rank + 1, theater_name, display_time, cfg.seat.quantity
            );
            tracing::info!(rank, theater = %theater_name, "consecutive seats found");
        } else {
            println!(
                "  ⏭️  Rank#{} {} ({}) — no {} consecutive seats",
                rank + 1, theater_name, display_time, cfg.seat.quantity
            );
            tracing::warn!(rank, theater = %theater_name, "no consecutive seats");
        }

        received[rank] = Some(payload);
        responded_count += 1;

        // Find the best (lowest rank = highest priority) that has seats
        if let Some(best_rank) = (0..n).find(|&r| {
            received[r].as_ref().is_some_and(|p| p.is_some())
        }) {
            // Confirm: all ranks with higher priority (lower index) have responded without seats
            if (0..best_rank).all(|r| received[r].is_some()) {
                // Abort any still-running lower-priority requests
                let aborted: usize = handles
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| received[*i].is_none())
                    .map(|(_, h)| { h.abort(); 1 })
                    .sum();
                if aborted > 0 {
                    tracing::info!(aborted, best_rank, "aborted lower-priority seat layout requests");
                }

                let (layout, seats) = received[best_rank].take().unwrap().unwrap();
                println!(
                    "🏆 Winner: Rank#{} {} — seats: {}",
                    best_rank + 1, ranked[best_rank].theater.name,
                    seats.iter().map(|s| s.display.as_str()).collect::<Vec<_>>().join(", ")
                );
                return Ok((ranked[best_rank].clone(), layout, seats));
            }
        }

        if responded_count == n {
            break;
        }
    }

    Err(anyhow::anyhow!(
        "No theater has {} consecutive available seats. Try reducing quantity or adjusting preferred_rows.",
        cfg.seat.quantity
    ))
}

/// Mengunci kursi dan membuat order dengan fitur Auto Seat Re-Roll on Conflict.
///
/// Jika `api::create_order` gagal (misalnya karena kursi bentrok / diambil orang lain milidetik sebelumnya),
/// kursi yang bermasalah akan di-blacklist secara otomatis dan bot langsung mencoba kursi alternatif
/// terbaik berikutnya di studio yang sama (tanpa jeda).
/// Jika semua kursi di bioskop/studio tersebut habis (atau mencapai `max_rerolls`),
/// bot akan otomatis fallback ke bioskop peringkat berikutnya dalam `ranked`.
async fn lock_order_with_reroll(
    http: &Client,
    cfg: &config::Config,
    ranked: &[theater_selector::SelectedShowtime],
    target_date: &str,
    movie: &crate::models::MovieData,
    strike_start: Instant,
) -> Result<(
    theater_selector::SelectedShowtime,
    crate::models::OrderData,
    Vec<crate::models::SelectedSeat>,
    Duration,
)> {
    if ranked.is_empty() {
        return Err(anyhow::anyhow!("Daftar bioskop ranked kosong"));
    }

    let mut current_ranked_slice = ranked;

    // 1. Dapatkan kandidat bioskop terbaik pertama yang memiliki kursi
    let (mut current_selected, mut current_layout, mut current_seats) =
        try_theaters_for_seats(http, cfg, current_ranked_slice).await?;

    let max_rerolls = cfg.seat.max_rerolls;
    let mut excluded_seats: HashSet<String> = HashSet::new();
    let mut current_rerolls: usize = 0;

    loop {
        let available_count: usize = current_layout
            .seat_map
            .iter()
            .flat_map(|sm| sm.seat_rows.iter())
            .filter(|sr| sr.status == 1 && !excluded_seats.contains(&sr.seat_row))
            .count();
        let total_count: usize = current_layout
            .seat_map
            .iter()
            .flat_map(|sm| sm.seat_rows.iter())
            .count();

        println!();
        println!("🎬 Selected showtime:");
        println!("   Movie:     {}", movie.name);
        println!("   Theater:   {}", current_selected.theater.name);
        println!("   Time:      {}", current_selected.showtime.display_time);
        println!("   Studio:    {}", current_selected.showtime.studio);
        println!("   Date:      {}", target_date);
        println!("   Category:  {}", current_selected.category);
        println!("   Price:     Rp{}", fmt_rupiah(current_selected.showtime.price));
        println!(
            "✅ Available: {}/{} seats (tx limit: {})          ",
            available_count, total_count, current_layout.user_seat_transaction_limit
        );
        let seat_displays = current_seats
            .iter()
            .map(|s| s.display.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        println!("💺 Selected seats: {}", seat_displays);

        tracing::info!(
            movie = %movie.name,
            theater = %current_selected.theater.name,
            time = %current_selected.showtime.display_time,
            studio = %current_selected.showtime.studio,
            date = %target_date,
            seats = %seat_displays,
            reroll_attempt = current_rerolls,
            "attempting create_order (locking seats)"
        );

        println!("\n🛒 Placing order (Locking seats: {})...", seat_displays);

        match api::create_order(
            http,
            &current_selected.theater.merchant.merchant_id,
            &current_selected.showtime.id,
            &current_seats,
        )
        .await
        {
            Ok(order) => {
                let seat_lock_elapsed = strike_start.elapsed();
                return Ok((current_selected, order, current_seats, seat_lock_elapsed));
            }
            Err(e) => {
                let failed_seat_str = seat_displays.clone();
                tracing::warn!(
                    theater = %current_selected.theater.name,
                    seats = %failed_seat_str,
                    error = %e,
                    attempt = current_rerolls + 1,
                    max_rerolls = max_rerolls,
                    "kursi bentrok / gagal create_order"
                );
                println!(
                    "⚠️  Kursi [{}] bentrok / gagal dilock: {}",
                    failed_seat_str, e
                );

                // Blacklist kursi yang bentrok
                for s in &current_seats {
                    excluded_seats.insert(s.display.clone());
                    excluded_seats.insert(s.seat_id.clone());
                }

                current_rerolls += 1;

                // Coba auto re-roll kursi berikutnya di layout bioskop yang sama
                let next_seats = if current_rerolls < max_rerolls {
                    seat_selector::select_with_exclusions(
                        &current_layout.seat_map,
                        &cfg.seat,
                        &excluded_seats,
                    )
                } else {
                    None
                };

                if let Some(next) = next_seats {
                    let next_display = next
                        .iter()
                        .map(|s| s.display.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    println!(
                        "🔄 [Auto Seat Re-Roll #{}/{}] Memilih alternatif di {}: {}",
                        current_rerolls,
                        max_rerolls,
                        current_selected.theater.name,
                        next_display
                    );
                    current_seats = next;
                    continue;
                }

                // Jika kursi di studio ini habis atau batas re-roll tercapai, fallback ke bioskop berikutnya
                println!(
                    "⏭️  Studio {} tidak lagi memiliki {} kursi berurutan (atau max reroll {} tercapai).",
                    current_selected.theater.name,
                    cfg.seat.quantity,
                    max_rerolls
                );

                let current_idx = current_ranked_slice
                    .iter()
                    .position(|r| r.showtime.id == current_selected.showtime.id)
                    .unwrap_or(0);
                let remaining = &current_ranked_slice[(current_idx + 1)..];

                if remaining.is_empty() {
                    return Err(anyhow::anyhow!(
                        "Semua bioskop yang cocok telah dicoba dan gagal dilock (kursi bentrok / habis)."
                    ));
                }

                println!(
                    "\n🔀 Fallback ke {} bioskop prioritas berikutnya...",
                    remaining.len()
                );

                current_ranked_slice = remaining;
                let (next_selected, next_layout, next_seats) =
                    try_theaters_for_seats(http, cfg, current_ranked_slice).await?;

                current_selected = next_selected;
                current_layout = next_layout;
                current_seats = next_seats;
                excluded_seats.clear();
                current_rerolls = 0;
            }
        }
    }
}

async fn wait_for_target_showtime(
    cfg: &mut config::Config,
    auth: &mut AuthSession,
    movie: &mut crate::models::MovieData,
) -> Result<(String, Vec<theater_selector::SelectedShowtime>, Instant)> {
    let mut last_modified = None;
    let mut current_movie_id = clean_movie_id(&cfg.target.movie_id);

    loop {
        // Cek jika config.toml diedit di background saat standby/polling
        config::check_and_reload(&mut last_modified, cfg);

        // Jika user mengubah movie_id di config.toml secara live
        let clean_id = clean_movie_id(&cfg.target.movie_id);
        if clean_id != current_movie_id {
            print!("🎥 Fetching updated movie {}...", clean_id);
            let updated = api::get_movie(&auth.http, &clean_id).await?;
            if !updated.id.trim().is_empty() {
                *movie = updated;
                current_movie_id = clean_id;
                println!("\r✅ Live reload: {} ({})          ", movie.name, movie.status);
            }
        }

        refresh_auth_if_needed(cfg, auth).await?;

        // Jika film masih upcoming, tunggu polling santai sesuai beacon_interval_mins
        if movie.status.eq_ignore_ascii_case("UPCOMING") {
            let release = movie
                .release_date
                .map(format_unix_wib)
                .unwrap_or_else(|| "unknown".to_string());
            wait_upcoming_or_fail(
                cfg,
                &format!(
                    "Film masih UPCOMING (presale_flag={:?}, release={}).",
                    movie.presale_flag, release
                ),
            )
            .await?;
            // Coba fetch status film lagi untuk mengecek apakah sudah NOW_PLAYING
            if let Ok(updated) = api::get_movie(&auth.http, &current_movie_id).await {
                if !updated.id.trim().is_empty() {
                    *movie = updated;
                }
            }
            continue;
        }

        // Tentukan daftar target_date berdasarkan prioritas (Multi-Date Priority):
        // Strategi B: Jika preferred_dates kosong dan target.date terisi, langsung gunakan (tanpa hit get_schedule_dates)!
        let target_dates: Vec<String> = if cfg.target.preferred_dates.is_empty() && !cfg.target.date.trim().is_empty() {
            vec![cfg.target.date.trim().to_string()]
        } else {
            // Ambil daftar tanggal aktif dari API
            let dates = match api::get_schedule_dates(&auth.http, &movie.id, &cfg.target.city_id).await? {
                Some(dates) => dates,
                None => {
                    wait_or_fail(cfg, "Schedule belum tersedia (DATA_NOT_FOUND).").await?;
                    continue;
                }
            };
            let prioritized = get_prioritized_target_dates(cfg, &dates);
            if prioritized.is_empty() {
                wait_or_fail(cfg, "Belum ada tanggal schedule yang aktif sesuai preferensi.").await?;
                continue;
            }
            prioritized
        };

        // War Strike Measurement: mulai tepat saat request jadwal jam ditembak
        let strike_start = Instant::now();
        let mut matched_target: Option<(String, Vec<theater_selector::SelectedShowtime>)> = None;

        for target_date in &target_dates {
            print!("🏟️  Checking schedules for {}...", target_date);
            let schedules = match api::get_showtimes(&auth.http, &movie.id, &cfg.target.city_id, target_date).await {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!(date = %target_date, error = %e, "Jadwal belum tersedia untuk tanggal");
                    continue;
                }
            };

            let ranked = theater_selector::rank(
                &schedules.theaters,
                &cfg.theater,
                &cfg.showtime,
                target_date,
                &cfg.target.blocked_datetime_ranges,
            );

            if !ranked.is_empty() {
                println!(
                    "\r✅ Found {} theater(s) pada tanggal {}                 ",
                    ranked.len(),
                    target_date
                );
                matched_target = Some((target_date.clone(), ranked));
                break;
            } else {
                println!(
                    "\r⏭️  Tanggal {} tidak ada showtime yang cocok.",
                    target_date
                );
            }
        }

        if let Some((target_date, ranked)) = matched_target {
            return Ok((target_date, ranked, strike_start));
        }

        wait_or_fail(
            cfg,
            "Schedule sudah ada, tapi belum ada showtime yang cocok dengan filter theater/time pada tanggal target.",
        )
        .await?;
    }
}

pub fn clean_movie_id(raw: &str) -> String {
    let trimmed = raw.trim();
    // Jika user paste URL atau slug seperti "hasut-2093187333460410368"
    for part in trimmed.rsplit(|c| c == '/' || c == '-' || c == '?' || c == '#') {
        if !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()) && part.len() >= 10 {
            return part.to_string();
        }
    }
    // Fallback: ambil semua digit jika ada
    let digits: String = trimmed.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() >= 10 {
        digits
    } else {
        trimmed.to_string()
    }
}

fn normalize_dt_start(s: &str) -> String {
    if s.len() == 10 { format!("{} 00:00", s) } else { s.to_string() }
}

fn normalize_dt_end(s: &str) -> String {
    if s.len() == 10 { format!("{} 23:59", s) } else { s.to_string() }
}

fn get_prioritized_target_dates(cfg: &config::Config, dates: &[crate::models::ScheduleDate]) -> Vec<String> {
    let is_blocked = |date: &str| -> bool {
        let day_start = format!("{} 00:00", date);
        let day_end   = format!("{} 23:59", date);
        cfg.target.blocked_datetime_ranges.iter().any(|range| {
            if range.len() == 2 {
                let start = normalize_dt_start(&range[0]);
                let end   = normalize_dt_end(&range[1]);
                start <= day_start && end >= day_end
            } else {
                false
            }
        })
    };

    if !cfg.target.preferred_dates.is_empty() {
        cfg.target
            .preferred_dates
            .iter()
            .filter(|pref| {
                dates
                    .iter()
                    .any(|d| d.date == **pref && d.is_any_schedule && !is_blocked(&d.date))
            })
            .cloned()
            .collect()
    } else if cfg.target.date.is_empty() {
        dates
            .iter()
            .filter(|d| d.is_any_schedule && !is_blocked(&d.date))
            .map(|d| d.date.clone())
            .collect()
    } else {
        dates
            .iter()
            .filter(|d| d.date == cfg.target.date && d.is_any_schedule && !is_blocked(&d.date))
            .map(|d| d.date.clone())
            .collect()
    }
}

fn pick_target_date(cfg: &config::Config, dates: &[crate::models::ScheduleDate]) -> Option<String> {
    get_prioritized_target_dates(cfg, dates).into_iter().next()
}

async fn wait_or_fail(cfg: &config::Config, reason: &str) -> Result<()> {
    if !cfg.polling.enabled {
        return Err(anyhow::anyhow!(reason.to_string()));
    }

    if api::is_verbose_timing() {
        let chip = crate::metrics::format_metrics_chip();
        println!(
            "⏱️  {} Retry in {}s... [{}]",
            reason, cfg.polling.interval_secs, chip
        );
    } else {
        println!(
            "⏱️  {} Retry in {}s...",
            reason, cfg.polling.interval_secs
        );
    }
    tracing::warn!(reason = %reason, retry_in_secs = cfg.polling.interval_secs, "polling retry");
    sleep(Duration::from_secs(cfg.polling.interval_secs)).await;
    Ok(())
}

async fn wait_upcoming_or_fail(cfg: &config::Config, reason: &str) -> Result<()> {
    if !cfg.polling.enabled {
        return Err(anyhow::anyhow!(reason.to_string()));
    }

    let interval_secs = (cfg.polling.beacon_interval_mins * 60).max(cfg.polling.interval_secs);
    if api::is_verbose_timing() {
        let chip = crate::metrics::format_metrics_chip();
        println!(
            "⏱️  {} Cek jadwal ulang dalam {} menit... [{}]",
            reason, cfg.polling.beacon_interval_mins, chip
        );
    } else {
        println!(
            "⏱️  {} Cek jadwal ulang dalam {} menit...",
            reason, cfg.polling.beacon_interval_mins
        );
    }
    tracing::warn!(reason = %reason, retry_in_secs = interval_secs, "upcoming polling retry");

    let steps = interval_secs / 5;
    let rem = interval_secs % 5;
    for _ in 0..steps {
        sleep(Duration::from_secs(5)).await;
    }
    if rem > 0 {
        sleep(Duration::from_secs(rem)).await;
    }
    Ok(())
}

fn wib() -> FixedOffset {
    FixedOffset::east_opt(7 * 3600).expect("valid WIB offset")
}

fn format_unix_wib(ts: i64) -> String {
    Utc.timestamp_opt(ts, 0)
        .single()
        .map(|dt| dt.with_timezone(&wib()).format("%Y-%m-%d %H:%M:%S WIB").to_string())
        .unwrap_or_else(|| format!("(ts={})", ts))
}

/// Format an integer as Indonesian thousands separator: 88000 → "88.000"
fn fmt_rupiah(amount: i64) -> String {
    let s = amount.to_string();
    let mut result = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push('.');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}

/// Render QRIS payload as terminal-friendly QR code using Unicode block characters.
fn render_qr_terminal(payload: &str) {
    if let Ok(code) = QrCode::new(payload.as_bytes()) {
        let image = code
            .render::<unicode::Dense1x2>()
            .dark_color(unicode::Dense1x2::Dark)
            .light_color(unicode::Dense1x2::Light)
            .build();
        println!("   QRIS (Terminal):");
        for line in image.lines() {
            println!("   {}", line);
        }
    } else {
        println!("   ⚠️  Failed to render QR code.");
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        AuthConfig, Config, DeviceConfig, PaymentConfig, PollingConfig,
        SeatConfig, ShowtimeConfig, TargetConfig, TheaterConfig,
    };
    use crate::models::ScheduleDate;

    fn make_config(date: &str, blocked: Vec<Vec<String>>) -> Config {
        Config {
            auth: AuthConfig { msisdn: "08123".into(), password: "pw".into() },
            target: TargetConfig {
                movie_id: "m1".into(),
                movie_title: "".into(),
                city_id: "c1".into(),
                date: date.into(),
                preferred_dates: vec![],
                blocked_datetime_ranges: blocked,
            },
            theater: TheaterConfig { theater_priority: vec![], blocked_theaters: vec![] },
            showtime: ShowtimeConfig { preferred_time_start: "".into(), preferred_time_end: "".into() },
            seat: SeatConfig { quantity: 2, manual_seats: vec![], avoid_first_rows: 0, preferred_rows: vec![], max_rerolls: 3 },
            device: DeviceConfig { device_id: "dev".into(), longitude: "0".into(), latitude: "0".into() },
            payment: PaymentConfig { payment_method: "M".into(), payment_option: "O".into() },
            polling: PollingConfig { enabled: false, interval_secs: 5, refresh_token_before_secs: 300, start_at: "".into(), beacon_interval_mins: 15 },
            notification: Default::default(),
            debug: Default::default(),
        }
    }

    // ── fmt_rupiah ────────────────────────────────────────────────────────

    #[test] fn fmt_rupiah_zero() { assert_eq!(fmt_rupiah(0), "0"); }
    #[test] fn fmt_rupiah_under_thousand() { assert_eq!(fmt_rupiah(999), "999"); }
    #[test] fn fmt_rupiah_exact_thousand() { assert_eq!(fmt_rupiah(1_000), "1.000"); }
    #[test] fn fmt_rupiah_tens_of_thousands() { assert_eq!(fmt_rupiah(88_000), "88.000"); }
    #[test] fn fmt_rupiah_hundreds_of_thousands() { assert_eq!(fmt_rupiah(150_000), "150.000"); }
    #[test] fn fmt_rupiah_millions() { assert_eq!(fmt_rupiah(1_500_000), "1.500.000"); }
    #[test] fn fmt_rupiah_large_number() { assert_eq!(fmt_rupiah(10_000_000), "10.000.000"); }

    // ── wib ───────────────────────────────────────────────────────────────

    #[test]
    fn wib_is_utc_plus_7_hours() {
        assert_eq!(wib().local_minus_utc(), 7 * 3600);
    }

    // ── format_unix_wib ───────────────────────────────────────────────────

    #[test]
    fn format_unix_wib_has_wib_suffix() {
        let s = format_unix_wib(1_748_736_000);
        assert!(s.ends_with("WIB"), "got: {s}");
    }

    #[test]
    fn format_unix_wib_correct_hour_for_midnight_utc() {
        // 1748736000 = 2025-06-01 00:00:00 UTC → 2025-06-01 07:00:00 WIB
        let s = format_unix_wib(1_748_736_000);
        assert!(s.contains("07:00:00"), "got: {s}");
    }

    #[test]
    fn format_unix_wib_correct_date() {
        let s = format_unix_wib(1_748_736_000);
        assert!(s.contains("2025-06-01"), "got: {s}");
    }

    // ── normalize_dt_start / normalize_dt_end ─────────────────────────────

    #[test]
    fn normalize_dt_start_date_only_appends_midnight() {
        assert_eq!(normalize_dt_start("2025-06-01"), "2025-06-01 00:00");
    }

    #[test]
    fn normalize_dt_start_datetime_unchanged() {
        assert_eq!(normalize_dt_start("2025-06-01 12:30"), "2025-06-01 12:30");
    }

    #[test]
    fn normalize_dt_end_date_only_appends_end_of_day() {
        assert_eq!(normalize_dt_end("2025-06-01"), "2025-06-01 23:59");
    }

    #[test]
    fn normalize_dt_end_datetime_unchanged() {
        assert_eq!(normalize_dt_end("2025-06-01 18:00"), "2025-06-01 18:00");
    }

    // ── pick_target_date ──────────────────────────────────────────────────

    #[test]
    fn pick_target_date_specific_match_found() {
        let cfg = make_config("2025-06-01", vec![]);
        let dates = vec![
            ScheduleDate { date: "2025-05-31".into(), is_any_schedule: true },
            ScheduleDate { date: "2025-06-01".into(), is_any_schedule: true },
        ];
        assert_eq!(pick_target_date(&cfg, &dates), Some("2025-06-01".into()));
    }

    #[test]
    fn pick_target_date_specific_not_in_list_returns_none() {
        let cfg = make_config("2025-06-05", vec![]);
        let dates = vec![ScheduleDate { date: "2025-06-01".into(), is_any_schedule: true }];
        assert_eq!(pick_target_date(&cfg, &dates), None);
    }

    #[test]
    fn pick_target_date_specific_exists_but_not_active_returns_none() {
        let cfg = make_config("2025-06-01", vec![]);
        let dates = vec![ScheduleDate { date: "2025-06-01".into(), is_any_schedule: false }];
        assert_eq!(pick_target_date(&cfg, &dates), None);
    }

    #[test]
    fn pick_target_date_empty_date_picks_first_active() {
        let cfg = make_config("", vec![]);
        let dates = vec![
            ScheduleDate { date: "2025-06-01".into(), is_any_schedule: false },
            ScheduleDate { date: "2025-06-02".into(), is_any_schedule: true },
        ];
        assert_eq!(pick_target_date(&cfg, &dates), Some("2025-06-02".into()));
    }

    #[test]
    fn pick_target_date_blocked_date_skipped_picks_next() {
        let blocked = vec![vec!["2025-06-01".into(), "2025-06-01".into()]];
        let cfg = make_config("", blocked);
        let dates = vec![
            ScheduleDate { date: "2025-06-01".into(), is_any_schedule: true },
            ScheduleDate { date: "2025-06-02".into(), is_any_schedule: true },
        ];
        assert_eq!(pick_target_date(&cfg, &dates), Some("2025-06-02".into()));
    }

    #[test]
    fn pick_target_date_specific_blocked_returns_none() {
        let blocked = vec![vec!["2025-06-01".into(), "2025-06-01".into()]];
        let cfg = make_config("2025-06-01", blocked);
        let dates = vec![ScheduleDate { date: "2025-06-01".into(), is_any_schedule: true }];
        assert_eq!(pick_target_date(&cfg, &dates), None);
    }

    #[test]
    fn pick_target_date_empty_schedule_list_returns_none() {
        let cfg = make_config("", vec![]);
        assert_eq!(pick_target_date(&cfg, &[]), None);
    }

    #[test]
    fn pick_target_date_partial_day_block_does_not_block_whole_day() {
        // Partial-day range covers only noon—14:00, should not block the whole date
        let blocked = vec![vec!["2025-06-01 12:00".into(), "2025-06-01 14:00".into()]];
        let cfg = make_config("2025-06-01", blocked);
        let dates = vec![ScheduleDate { date: "2025-06-01".into(), is_any_schedule: true }];
        // day_start="2025-06-01 00:00" < block_start="2025-06-01 12:00" → not fully covered
        assert_eq!(pick_target_date(&cfg, &dates), Some("2025-06-01".into()));
    }

    #[test]
    fn pick_target_date_with_preferred_dates_picks_first_available() {
        let mut cfg = make_config("", vec![]);
        cfg.target.preferred_dates = vec!["2025-06-02".into(), "2025-06-03".into()];
        let dates = vec![
            ScheduleDate { date: "2025-06-01".into(), is_any_schedule: true },
            ScheduleDate { date: "2025-06-03".into(), is_any_schedule: true },
        ];
        // 2025-06-02 is preferred first but not in list -> falls back to 2025-06-03
        assert_eq!(pick_target_date(&cfg, &dates), Some("2025-06-03".into()));
    }

    #[test]
    fn pick_target_date_with_preferred_dates_prioritizes_order() {
        let mut cfg = make_config("", vec![]);
        cfg.target.preferred_dates = vec!["2025-06-02".into(), "2025-06-03".into()];
        let dates = vec![
            ScheduleDate { date: "2025-06-03".into(), is_any_schedule: true },
            ScheduleDate { date: "2025-06-02".into(), is_any_schedule: true },
        ];
        // Both are active -> picks 2025-06-02 because it's first in preferred_dates
        assert_eq!(pick_target_date(&cfg, &dates), Some("2025-06-02".into()));
    }

    #[test]
    fn pick_target_date_preferred_dates_skips_blocked_date() {
        let blocked = vec![vec!["2025-06-02".into(), "2025-06-02".into()]];
        let mut cfg = make_config("", blocked);
        cfg.target.preferred_dates = vec!["2025-06-02".into(), "2025-06-03".into()];
        let dates = vec![
            ScheduleDate { date: "2025-06-02".into(), is_any_schedule: true },
            ScheduleDate { date: "2025-06-03".into(), is_any_schedule: true },
        ];
        // 2025-06-02 is blocked -> falls back to 2025-06-03
        assert_eq!(pick_target_date(&cfg, &dates), Some("2025-06-03".into()));
    }

    #[test]
    fn get_prioritized_target_dates_returns_all_active_matching_dates() {
        let mut cfg = make_config("", vec![]);
        cfg.target.preferred_dates = vec!["2025-06-02".into(), "2025-06-03".into(), "2025-06-04".into()];
        let dates = vec![
            ScheduleDate { date: "2025-06-01".into(), is_any_schedule: true },
            ScheduleDate { date: "2025-06-02".into(), is_any_schedule: true },
            ScheduleDate { date: "2025-06-03".into(), is_any_schedule: false },
            ScheduleDate { date: "2025-06-04".into(), is_any_schedule: true },
        ];
        // 2025-06-03 is inactive -> returns ["2025-06-02", "2025-06-04"]
        assert_eq!(
            get_prioritized_target_dates(&cfg, &dates),
            vec!["2025-06-02", "2025-06-04"]
        );
    }

    // ── wait_until_start ──────────────────────────────────────────────────

    #[tokio::test]
    async fn wait_until_start_empty_string_returns_ok() {
        assert!(wait_until_start("").await.is_ok());
    }

    #[tokio::test]
    async fn wait_until_start_whitespace_returns_ok() {
        assert!(wait_until_start("   ").await.is_ok());
    }

    #[tokio::test]
    async fn wait_until_start_invalid_format_returns_err() {
        assert!(wait_until_start("not-a-valid-datetime").await.is_err());
    }

    #[tokio::test]
    async fn wait_until_start_past_datetime_returns_ok_immediately() {
        // 2020-01-01 is well in the past → sleep(0s) → instant return
        assert!(wait_until_start("2020-01-01 00:00:00").await.is_ok());
    }

    // ── wait_or_fail ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn wait_or_fail_disabled_returns_err_with_reason() {
        // make_config sets polling.enabled = false
        let cfg = make_config("", vec![]);
        let result = wait_or_fail(&cfg, "no showtimes found").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("no showtimes found"));
    }

    #[tokio::test]
    async fn wait_or_fail_enabled_zero_interval_returns_ok() {
        let mut cfg = make_config("", vec![]);
        cfg.polling.enabled = true;
        cfg.polling.interval_secs = 0; // zero sleep → returns instantly
        let result = wait_or_fail(&cfg, "retrying").await;
        assert!(result.is_ok());
    }

    // ── render_qr_terminal ────────────────────────────────────────────────

    #[test]
    fn render_qr_terminal_short_payload_does_not_panic() {
        render_qr_terminal("HELLO_WORLD");
    }

    #[test]
    fn render_qr_terminal_empty_payload_does_not_panic() {
        // empty → QrCode::new succeeds with empty content
        render_qr_terminal("");
    }

    #[test]
    fn render_qr_terminal_realistic_qris_does_not_panic() {
        render_qr_terminal("00020101021226580013ID.CO.BNI.WWW01189360050400015743700203BNI51440014ID.CO.QRIS.WWW0215ID20230705183270303UMI5204599953033605802ID5911Test Store6013Jakarta Pusat63043E2A");
    }

    // ── refresh_auth_if_needed (early return path) ────────────────────────

    #[tokio::test]
    async fn refresh_auth_if_needed_returns_ok_immediately_when_recently_authed() {
        // authenticated_at = Instant::now() → elapsed ≈ 0 < refresh_token_before_secs (300)
        // → function returns Ok(()) without any network call
        let cfg = make_config("", vec![]);
        let mut auth = AuthSession {
            http: crate::client::build(None, "test-device").unwrap(),
            user_name: "tester".into(),
            refresh_token: "".into(),
            authenticated_at: std::time::Instant::now(),
        };
        assert!(refresh_auth_if_needed(&cfg, &mut auth).await.is_ok());
    }

    // ── clean_movie_id ───────────────────────────────────────────────────

    #[test]
    fn clean_movie_id_plain_numeric() {
        assert_eq!(clean_movie_id("2093187333460410368"), "2093187333460410368");
    }

    #[test]
    fn clean_movie_id_with_slug_prefix() {
        assert_eq!(clean_movie_id("hasut-2093187333460410368"), "2093187333460410368");
    }

    #[test]
    fn clean_movie_id_with_full_url() {
        assert_eq!(
            clean_movie_id("https://www.tix.id/movie/hasut-2093187333460410368"),
            "2093187333460410368"
        );
    }

    #[test]
    fn clean_movie_id_with_url_query_param() {
        assert_eq!(
            clean_movie_id("https://tix.id/movie/hasut-2093187333460410368?ref=share"),
            "2093187333460410368"
        );
    }
}
