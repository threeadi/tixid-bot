use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use chrono::{FixedOffset, NaiveDateTime, TimeZone, Utc};
use qrcode::{QrCode, render::unicode};
use reqwest::Client;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
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
    let (movie, target_date, ranked) = wait_for_target_showtime(&cfg, &mut auth).await?;

    // ── 4 & 5. Find theater with N consecutive seats (try in priority order) ─
    let (selected, layout, seats) =
        try_theaters_for_seats(&auth.http, &cfg, &ranked).await?;

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

    println!();
    println!("🎬 Selected showtime:");
    println!("   Movie:     {}", movie.name);
    println!("   Theater:  {}", selected.theater.name);
    println!("   Time:     {}", selected.showtime.display_time);
    println!("   Studio:   {}", selected.showtime.studio);
    println!("   Date:     {}", target_date);
    println!("   Category: {}", selected.category);
    println!("   Price:    Rp{}", fmt_rupiah(selected.showtime.price));
    println!(
        "✅ Available: {}/{} seats (tx limit: {})          ",
        available_count, total_count, layout.user_seat_transaction_limit
    );
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

    println!("💺 Selected seats: {}", seats.iter().map(|s| s.display.as_str()).collect::<Vec<_>>().join(", "));
    tracing::info!(seats = %seats.iter().map(|s| s.display.as_str()).collect::<Vec<_>>().join(", "), theater = %selected.theater.name, "seats selected");

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

async fn wait_for_target_showtime(
    cfg: &config::Config,
    auth: &mut AuthSession,
) -> Result<(crate::models::MovieData, String, Vec<theater_selector::SelectedShowtime>)> {
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

        let ranked = theater_selector::rank(&schedules.theaters, &cfg.theater, &cfg.showtime, &target_date, &cfg.target.blocked_datetime_ranges);
        if !ranked.is_empty() {
            return Ok((movie, target_date, ranked));
        }

        wait_or_fail(
            cfg,
            "Schedule sudah ada, tapi belum ada showtime yang cocok dengan filter theater/time.",
        )
        .await?;
    }
}

fn normalize_dt_start(s: &str) -> String {
    if s.len() == 10 { format!("{} 00:00", s) } else { s.to_string() }
}

fn normalize_dt_end(s: &str) -> String {
    if s.len() == 10 { format!("{} 23:59", s) } else { s.to_string() }
}

fn pick_target_date(cfg: &config::Config, dates: &[crate::models::ScheduleDate]) -> Option<String> {
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

    if cfg.target.date.is_empty() {
        dates
            .iter()
            .find(|d| d.is_any_schedule && !is_blocked(&d.date))
            .map(|d| d.date.clone())
    } else {
        dates
            .iter()
            .find(|d| d.date == cfg.target.date && d.is_any_schedule && !is_blocked(&d.date))
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
                city_id: "c1".into(),
                date: date.into(),
                blocked_datetime_ranges: blocked,
            },
            theater: TheaterConfig { theater_priority: vec![], blocked_theaters: vec![] },
            showtime: ShowtimeConfig { preferred_time_start: "".into(), preferred_time_end: "".into() },
            seat: SeatConfig { quantity: 2, manual_seats: vec![], avoid_first_rows: 0, preferred_rows: vec![] },
            device: DeviceConfig { device_id: "dev".into(), longitude: "0".into(), latitude: "0".into() },
            payment: PaymentConfig { payment_method: "M".into(), payment_option: "O".into() },
            polling: PollingConfig { enabled: false, interval_secs: 5, refresh_token_before_secs: 300, start_at: "".into() },
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
}
