use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use chrono::{FixedOffset, NaiveDateTime, TimeZone, Utc};
use qrcode::{QrCode, render::unicode};
use reqwest::Client;
use tokio::time::sleep;

use crate::{api, client, config, seat_selector, theater_selector};

struct AuthSession {
    http: Client,
    user_name: String,
    refresh_token: String,
    authenticated_at: Instant,
}

pub async fn run() -> Result<()> {
    tracing::info!("bot starting");
    println!("🎬 TIX.ID Bot");
    println!("========================================");

    // ── 1. Load config ───────────────────────────────────────────────────────
    let cfg = config::load()?;
    wait_until_start(&cfg.polling.start_at).await?;

    let bot_start = Instant::now();

    // ── 2. Auth: guest token → login ────────────────────────────────────────
    let mut auth = authenticate(&cfg).await?;

    // ── 3. Poll until movie schedule/showtime is ready ──────────────────────
    let (movie, target_date, selected) = wait_for_target_showtime(&cfg, &mut auth).await?;

    println!();
    println!("🎬 Selected showtime:");
    println!("   Movie:     {}", movie.name);
    println!("   Theater:  {}", selected.theater.name);
    println!("   Time:     {}", selected.showtime.display_time);
    println!("   Studio:   {}", selected.showtime.studio);
    println!("   Date:     {}", target_date);
    println!("   Category: {}", selected.category);
    println!("   Price:    Rp{}", fmt_rupiah(selected.showtime.price));
    tracing::info!(
        movie = %movie.name,
        theater = %selected.theater.name,
        time = %selected.showtime.display_time,
        studio = %selected.showtime.studio,
        date = %target_date,
        category = %selected.category,
        price = selected.showtime.price,
        showtime_id = %selected.showtime.id,
        "showtime selected"
    );

    // ── 4. Get seat layout ────────────────────────────────────────────────────
    print!("\n💺 Fetching seat layout...");
    let layout =
        api::get_seat_layout(&auth.http, &selected.merchant_slug, &selected.showtime.id).await?;

    let available_count: usize = layout
        .seat_map
        .iter()
        .flat_map(|sm| sm.seat_rows.iter())
        .filter(|sr| sr.status == 1)
        .count();
    let total_count: usize = layout
        .seat_map
        .iter()
        .flat_map(|sm| sm.seat_rows.iter())
        .count();
    println!(
        "\r✅ Available: {}/{} seats (tx limit: {})          ",
        available_count, total_count, layout.user_seat_transaction_limit
    );

    // ── 5. Select seats ───────────────────────────────────────────────────────
    let seats = seat_selector::select(&layout.seat_map, &cfg.seat).ok_or_else(|| {
        anyhow::anyhow!(
            "Could not find {} suitable consecutive seats. Try reducing quantity or adjusting preferred_rows.",
            cfg.seat.quantity
        )
    })?;
    println!("💺 Selected seats: {}", seats.join(", "));
    tracing::info!(seats = %seats.join(", "), "seats selected");

    // ── 6. Create order ───────────────────────────────────────────────────────
    println!("\n🛒 Placing order...");
    let order = api::create_order(
        &auth.http,
        &selected.theater.merchant.merchant_id,
        &selected.showtime.id,
        &seats,
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
    println!("🎉 ORDER SUCCESSFUL!");
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
        "order created"
    );
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
    let elapsed = bot_start.elapsed();
    let elapsed_str = if elapsed.as_secs() >= 60 {
        format!("{}m {:.3}s", elapsed.as_secs() / 60, (elapsed.as_millis() % 60_000) as f64 / 1000.0)
    } else {
        format!("{:.3}s", elapsed.as_secs_f64())
    };
    tracing::info!(
        order_id = %order.id,
        payment_option = %payment.payment_option,
        total = payment.total_payment,
        payment_code = %payment.payment_code,
        elapsed_ms = elapsed.as_millis(),
        "checkout completed"
    );
    println!();
    println!("========================================");
    println!("💳 PAYMENT INFO");
    println!("========================================");
    println!("   Method:    {}", payment.payment_option);
    println!("   Amount:    Rp{}", fmt_rupiah(payment.total_payment));
    println!("   ⏱️  Total time: {}", elapsed_str);
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

async fn wait_for_target_showtime(
    cfg: &config::Config,
    auth: &mut AuthSession,
) -> Result<(crate::models::MovieData, String, theater_selector::SelectedShowtime)> {
    loop {
        refresh_auth_if_needed(cfg, auth).await?;

        print!("🎥 Fetching movie {}...", cfg.target.movie_id);
        let movie = api::get_movie(&auth.http, &cfg.target.movie_id).await?;
        println!(
            "\r✅ {} ({} min, {})               ",
            movie.name, movie.duration, movie.status
        );

        if movie.status.eq_ignore_ascii_case("UPCOMING") {
            let release = movie
                .release_date
                .map(format_unix_wib)
                .unwrap_or_else(|| "unknown".to_string());
            wait_or_fail(
                cfg,
                &format!(
                    "Film masih UPCOMING (presale_flag={:?}, release={}).",
                    movie.presale_flag, release
                ),
            )
            .await?;
            continue;
        }

        let dates = match api::get_schedule_dates(&auth.http, &movie.id, &cfg.target.city_id).await?
        {
            Some(dates) => dates,
            None => {
                wait_or_fail(cfg, "Schedule belum tersedia (DATA_NOT_FOUND).").await?;
                continue;
            }
        };

        let target_date = match pick_target_date(cfg, &dates) {
            Some(date) => date,
            None => {
                let msg = if cfg.target.date.is_empty() {
                    "Belum ada tanggal schedule yang aktif."
                } else {
                    "Tanggal target belum tersedia di schedule."
                };
                wait_or_fail(cfg, msg).await?;
                continue;
            }
        };

        print!("🏟️  Getting schedules for {}...", target_date);
        let schedules =
            api::get_showtimes(&auth.http, &movie.id, &cfg.target.city_id, &target_date).await?;
        println!(
            "\r✅ Found {} theater(s)                 ",
            schedules.theaters.len()
        );

        if let Some(selected) =
            theater_selector::select(&schedules.theaters, &cfg.theater, &cfg.showtime)
        {
            return Ok((movie, target_date, selected));
        }

        wait_or_fail(
            cfg,
            "Schedule sudah ada, tapi belum ada showtime yang cocok dengan filter theater/time.",
        )
        .await?;
    }
}

fn pick_target_date(cfg: &config::Config, dates: &[crate::models::ScheduleDate]) -> Option<String> {
    if cfg.target.date.is_empty() {
        dates
            .iter()
            .find(|d| d.is_any_schedule)
            .map(|d| d.date.clone())
    } else {
        dates
            .iter()
            .find(|d| d.date == cfg.target.date && d.is_any_schedule)
            .map(|d| d.date.clone())
    }
}

async fn wait_or_fail(cfg: &config::Config, reason: &str) -> Result<()> {
    if !cfg.polling.enabled {
        return Err(anyhow::anyhow!(reason.to_string()));
    }

    println!(
        "⏱️  {} Retry in {}s...",
        reason, cfg.polling.interval_secs
    );
    tracing::warn!(reason = %reason, retry_in_secs = cfg.polling.interval_secs, "polling retry");
    sleep(Duration::from_secs(cfg.polling.interval_secs)).await;
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
