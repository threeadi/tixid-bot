use std::io::Write;

use chrono::Utc;
use serde_json::json;

use crate::config::NotificationConfig;

/// Payload holding the checkout and order details to notify.
#[derive(Debug, Clone)]
pub struct OrderNotificationPayload {
    pub order_id: String,
    pub movie_name: String,
    pub theater_name: String,
    pub studio_name: String,
    pub selected_seats: Vec<String>,
    pub quantity: usize,
    pub ticket_price: i64,
    pub convenience_fee: i64,
    pub total_payment: i64,
    pub payment_option: String,
    pub expired_at_wib: String,
    pub payment_code: String,
    pub qr_image_url: String,
}

/// Format an integer as Indonesian thousands separator: 88000 → "88.000"
pub fn fmt_rupiah(amount: i64) -> String {
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

/// Normalize and sanitize Discord mention string.
/// Supports single or multiple mentions separated by commas, semicolons, or whitespace.
/// Converts numeric IDs (with or without '@') to '<@ID>', preserves '<@...>', '<@&...>',
/// '@everyone', and '@here'.
pub fn normalize_discord_mention(mention: &str) -> String {
    let trimmed = mention.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    // Split tokens by comma, semicolon, or whitespace
    let tokens: Vec<&str> = trimmed
        .split(|c: char| c == ',' || c == ';' || c.is_whitespace())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    if tokens.is_empty() {
        return String::new();
    }

    let mut normalized_tokens = Vec::with_capacity(tokens.len());

    for token in tokens {
        if token == "@everyone" || token == "@here" {
            normalized_tokens.push(token.to_string());
        } else if (token.starts_with("<@") || token.starts_with("<@&")) && token.ends_with('>') {
            normalized_tokens.push(token.to_string());
        } else {
            let digits_part = token.strip_prefix('@').unwrap_or(token);
            if !digits_part.is_empty() && digits_part.chars().all(|c| c.is_ascii_digit()) {
                normalized_tokens.push(format!("<@{}>", digits_part));
            } else {
                tracing::warn!(
                    mention = %token,
                    "Discord mention '{}' appears to be a username handle. Discord Webhook requires a numeric User ID format: <@USER_ID> or @everyone.",
                    token
                );
                normalized_tokens.push(token.to_string());
            }
        }
    }

    normalized_tokens.join(" ")
}

/// Build the Discord webhook JSON body.
pub fn build_discord_payload(
    mention: &str,
    payload: &OrderNotificationPayload,
) -> serde_json::Value {
    let seats_str = if payload.selected_seats.is_empty() {
        "-".to_string()
    } else {
        payload.selected_seats.join(", ")
    };

    let fields = vec![
        json!({
            "name": "🎬 Film",
            "value": payload.movie_name,
            "inline": true
        }),
        json!({
            "name": "🏢 Bioskop",
            "value": format!("{} ({})", payload.theater_name, payload.studio_name),
            "inline": true
        }),
        json!({
            "name": "💺 Kursi",
            "value": format!("{} ({} tiket)", seats_str, payload.quantity),
            "inline": true
        }),
        json!({
            "name": "💵 Total Bayar",
            "value": format!(
                "Rp{} (Tiket: Rp{} + Fee: Rp{})",
                fmt_rupiah(payload.total_payment),
                fmt_rupiah(payload.ticket_price),
                fmt_rupiah(payload.convenience_fee)
            ),
            "inline": true
        }),
        json!({
            "name": "⏰ Batas Bayar",
            "value": format!("{} WIB", payload.expired_at_wib),
            "inline": true
        }),
        json!({
            "name": "💳 Metode",
            "value": payload.payment_option,
            "inline": true
        }),
    ];

    let description = if !payload.payment_code.is_empty() {
        format!(
            "Segera scan QRIS berikut untuk menyelesaikan pembayaran.\n`{}`",
            payload.payment_code
        )
    } else {
        "Segera selesaikan pembayaran tiket Anda.".to_string()
    };

    let mut embed = json!({
        "title": "🎟️ TIX.ID Order Berhasil Di-checkout!",
        "description": description,
        "color": 3066993, // #2ECC71 Green
        "fields": fields,
        "footer": {
            "text": format!("Order ID: {}", payload.order_id)
        },
        "timestamp": Utc::now().to_rfc3339()
    });

    if !payload.qr_image_url.is_empty() {
        embed["image"] = json!({ "url": payload.qr_image_url });
    }

    let mut body = json!({
        "embeds": [embed],
        "allowed_mentions": {
            "parse": ["users", "roles", "everyone"]
        }
    });

    let normalized_mention = normalize_discord_mention(mention);
    if !normalized_mention.is_empty() {
        body["content"] = json!(normalized_mention);
    }

    body
}

/// Build the desktop notification summary and body strings.
pub fn build_desktop_notification_text(payload: &OrderNotificationPayload) -> (String, String) {
    let seats_str = if payload.selected_seats.is_empty() {
        "-".to_string()
    } else {
        payload.selected_seats.join(", ")
    };

    let summary = "🎟️ TIX.ID — Checkout Berhasil!".to_string();
    let body = format!(
        "Film: {}\nBioskop: {} ({})\nKursi: {}\nTotal: Rp{}\nBatas Bayar: {} WIB",
        payload.movie_name,
        payload.theater_name,
        payload.studio_name,
        seats_str,
        fmt_rupiah(payload.total_payment),
        payload.expired_at_wib
    );

    (summary, body)
}

/// Send native OS desktop toast notification.
fn send_desktop_notification(payload: &OrderNotificationPayload) {
    let (summary, body) = build_desktop_notification_text(payload);
    match notify_rust::Notification::new()
        .appname("TIX.ID Bot")
        .summary(&summary)
        .body(&body)
        .urgency(notify_rust::Urgency::Critical)
        .timeout(notify_rust::Timeout::Never)
        .show()
    {
        Ok(_) => {
            tracing::info!("Desktop notification displayed");
        }
        Err(e) => {
            tracing::warn!(error = %e, "Failed to display desktop notification");
        }
    }
}

/// Trigger terminal bell audio alert.
fn play_desktop_sound() {
    print!("\x07\x07\x07");
    let _ = std::io::stdout().flush();
}

/// Send Discord webhook notification.
async fn send_discord_webhook(
    client: &reqwest::Client,
    webhook_url: &str,
    mention: &str,
    payload: &OrderNotificationPayload,
) {
    let body = build_discord_payload(mention, payload);

    match client.post(webhook_url).json(&body).send().await {
        Ok(res) => {
            if res.status().is_success() {
                println!("📢 Discord notification sent successfully!");
                tracing::info!("Discord webhook sent successfully");
            } else {
                let status = res.status();
                let text = res.text().await.unwrap_or_default();
                println!("⚠️  Discord notification failed (HTTP {}): {}", status, text);
                tracing::warn!(status = %status, response = %text, "Discord webhook returned non-success status");
            }
        }
        Err(e) => {
            println!("⚠️  Discord notification request error: {}", e);
            tracing::warn!(error = %e, "Failed to send Discord webhook");
        }
    }
}

/// Main entry point to send all configured checkout notifications.
pub async fn notify_checkout(cfg: &NotificationConfig, payload: &OrderNotificationPayload) {
    if cfg.desktop_sound {
        play_desktop_sound();
    }

    if cfg.desktop_enabled {
        send_desktop_notification(payload);
    }

    if cfg.discord_enabled && !cfg.discord_webhook_url.trim().is_empty() {
        print!("📢 Sending Discord webhook notification...");
        let _ = std::io::stdout().flush();
        let client = reqwest::Client::new();
        send_discord_webhook(&client, &cfg.discord_webhook_url, &cfg.discord_mention, payload).await;
    }
}

/// Send notification when a movie is discovered in the catalog via Beacon mode.
pub async fn notify_beacon_found(
    cfg: &NotificationConfig,
    query_title: &str,
    matched_title: &str,
    movie_id: &str,
    similarity: f64,
    status: &str,
    next_action: &str,
) {
    if cfg.desktop_sound {
        play_desktop_sound();
    }

    if cfg.desktop_enabled {
        let summary = "📡 TIX.ID — Film Ditemukan! (Beacon)".to_string();
        let body = format!(
            "Pencarian: {}\nFilm: {}\nMovie ID: {}\nKemiripan: {:.0}%\nStatus: {}\n{}",
            query_title,
            matched_title,
            movie_id,
            similarity * 100.0,
            status,
            next_action
        );
        let _ = notify_rust::Notification::new()
            .appname("TIX.ID Bot")
            .summary(&summary)
            .body(&body)
            .urgency(notify_rust::Urgency::Normal)
            .timeout(notify_rust::Timeout::Milliseconds(8000))
            .show();
    }

    if cfg.discord_enabled && !cfg.discord_webhook_url.trim().is_empty() {
        let client = reqwest::Client::new();
        let embed = json!({
            "title": "📡 Film Ditemukan di Katalog TIX.ID! (Beacon Mode)",
            "description": format!("Bot berhasil menemukan film yang cocok dengan target `{}`.", query_title),
            "color": 3447003,
            "fields": [
                { "name": "🎬 Judul Film", "value": matched_title, "inline": true },
                { "name": "🆔 Movie ID", "value": movie_id, "inline": true },
                { "name": "🎯 Kemiripan", "value": format!("{:.1}%", similarity * 100.0), "inline": true },
                { "name": "📌 Status", "value": status, "inline": true },
                { "name": "⚡ Aksi Berikutnya", "value": next_action, "inline": false }
            ],
            "footer": {
                "text": "TIX.ID Ticket Bot • Beaconing System"
            },
            "timestamp": Utc::now().to_rfc3339()
        });

        let mut body = json!({
            "embeds": [embed],
            "allowed_mentions": { "parse": ["users", "roles", "everyone"] }
        });
        let normalized = normalize_discord_mention(&cfg.discord_mention);
        if !normalized.is_empty() {
            body["content"] = json!(normalized);
        }
        let _ = client.post(&cfg.discord_webhook_url).json(&body).send().await;
    }
}

/// Send notification when presale showtimes open for a beacon-tracked movie.
pub async fn notify_beacon_presale_opened(
    cfg: &NotificationConfig,
    movie_name: &str,
    movie_id: &str,
    target_date: &str,
) {
    if cfg.desktop_sound {
        play_desktop_sound();
    }

    if cfg.desktop_enabled {
        let summary = "⚡ TIX.ID — Jadwal Presale Terbuka!".to_string();
        let body = format!(
            "Film: {}\nTanggal: {}\nMovie ID: {}\nBot segera meluncurkan War Checkout!",
            movie_name, target_date, movie_id
        );
        let _ = notify_rust::Notification::new()
            .appname("TIX.ID Bot")
            .summary(&summary)
            .body(&body)
            .urgency(notify_rust::Urgency::Critical)
            .timeout(notify_rust::Timeout::Never)
            .show();
    }

    if cfg.discord_enabled && !cfg.discord_webhook_url.trim().is_empty() {
        let client = reqwest::Client::new();
        let embed = json!({
            "title": "⚡ Jadwal / Presale Tiket Telah Dibuka!",
            "description": format!("Jadwal tayang untuk **{}** sudah tersedia. Memulai proses War Checkout...", movie_name),
            "color": 15105570,
            "fields": [
                { "name": "🎬 Film", "value": movie_name, "inline": true },
                { "name": "📅 Tanggal Tayang", "value": target_date, "inline": true },
                { "name": "🆔 Movie ID", "value": movie_id, "inline": true }
            ],
            "footer": {
                "text": "TIX.ID Ticket Bot • War Alert"
            },
            "timestamp": Utc::now().to_rfc3339()
        });

        let mut body = json!({
            "embeds": [embed],
            "allowed_mentions": { "parse": ["users", "roles", "everyone"] }
        });
        let normalized = normalize_discord_mention(&cfg.discord_mention);
        if !normalized.is_empty() {
            body["content"] = json!(normalized);
        }
        let _ = client.post(&cfg.discord_webhook_url).json(&body).send().await;
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_payload() -> OrderNotificationPayload {
        OrderNotificationPayload {
            order_id: "ORD-12345".into(),
            movie_name: "Captain America: Brave New World".into(),
            theater_name: "Grand Indonesia CGV".into(),
            studio_name: "Studio 1".into(),
            selected_seats: vec!["D5".into(), "D6".into()],
            quantity: 2,
            ticket_price: 50000,
            convenience_fee: 6000,
            total_payment: 106000,
            payment_option: "NETWORK_PAY_PG_QRIS".into(),
            expired_at_wib: "14:30:00".into(),
            payment_code: "000201010212...".into(),
            qr_image_url: "https://api.qrserver.com/v1/create-qr-code/?size=300x300&data=test".into(),
        }
    }

    #[test]
    fn test_fmt_rupiah() {
        assert_eq!(fmt_rupiah(0), "0");
        assert_eq!(fmt_rupiah(500), "500");
        assert_eq!(fmt_rupiah(1000), "1.000");
        assert_eq!(fmt_rupiah(106000), "106.000");
        assert_eq!(fmt_rupiah(1250000), "1.250.000");
    }

    #[test]
    fn test_build_desktop_notification_text() {
        let p = sample_payload();
        let (summary, body) = build_desktop_notification_text(&p);
        assert!(summary.contains("Checkout Berhasil"));
        assert!(body.contains("Captain America"));
        assert!(body.contains("Grand Indonesia CGV (Studio 1)"));
        assert!(body.contains("D5, D6"));
        assert!(body.contains("Rp106.000"));
        assert!(body.contains("14:30:00 WIB"));
    }

    #[test]
    fn test_build_discord_payload_without_mention() {
        let p = sample_payload();
        let json = build_discord_payload("", &p);

        assert!(json.get("content").is_none());
        let embeds = json["embeds"].as_array().expect("embeds array");
        assert_eq!(embeds.len(), 1);
        let embed = &embeds[0];
        assert_eq!(embed["title"], "🎟️ TIX.ID Order Berhasil Di-checkout!");
        assert_eq!(embed["image"]["url"], p.qr_image_url);

        let fields = embed["fields"].as_array().expect("fields array");
        assert_eq!(fields.len(), 6);
        assert_eq!(fields[0]["name"], "🎬 Film");
        assert_eq!(fields[0]["value"], "Captain America: Brave New World");
    }

    #[test]
    fn test_build_discord_payload_with_mention() {
        let p = sample_payload();
        let json = build_discord_payload("<@&123456789>", &p);

        assert_eq!(json["content"], "<@&123456789>");
        assert_eq!(
            json["allowed_mentions"]["parse"],
            serde_json::json!(["users", "roles", "everyone"])
        );
        let embeds = json["embeds"].as_array().unwrap();
        assert_eq!(embeds.len(), 1);
    }

    #[test]
    fn test_normalize_discord_mention() {
        assert_eq!(normalize_discord_mention(""), "");
        assert_eq!(normalize_discord_mention("   "), "");
        assert_eq!(normalize_discord_mention("@everyone"), "@everyone");
        assert_eq!(normalize_discord_mention("@here"), "@here");
        assert_eq!(normalize_discord_mention("<@123456789>"), "<@123456789>");
        assert_eq!(normalize_discord_mention("<@&987654321>"), "<@&987654321>");
        // Raw numbers
        assert_eq!(normalize_discord_mention("123456789"), "<@123456789>");
        assert_eq!(normalize_discord_mention("@123456789"), "<@123456789>");
        // Multiple mentions with commas and @
        assert_eq!(
            normalize_discord_mention("@407480862545018881, @689971343042936867"),
            "<@407480862545018881> <@689971343042936867>"
        );
        assert_eq!(
            normalize_discord_mention("407480862545018881, 689971343042936867"),
            "<@407480862545018881> <@689971343042936867>"
        );
        assert_eq!(
            normalize_discord_mention("<@407480862545018881>, <@689971343042936867>"),
            "<@407480862545018881> <@689971343042936867>"
        );
        assert_eq!(
            normalize_discord_mention("@everyone, @407480862545018881"),
            "@everyone <@407480862545018881>"
        );
        // Username string returns trimmed as is (with log warning)
        assert_eq!(normalize_discord_mention("@triadi"), "@triadi");
    }
}
