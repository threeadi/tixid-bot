mod api;
mod bot;
mod client;
mod config;
mod logger;
mod models;
mod notifier;
mod seat_selector;
mod theater_selector;
mod wizard;

use std::io::{self, Write};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Guard must live until end of main so the background writer flushes on exit.
    let _log_guard = logger::init();

    // Check CLI flags
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--setup" || a == "-s") {
        wizard::run_setup()?;
        print!("\nApakah ingin langsung menjalankan bot? [Y/n]: ");
        let _ = io::stdout().flush();
        let mut ans = String::new();
        let _ = io::stdin().read_line(&mut ans);
        if ans.trim().eq_ignore_ascii_case("n") {
            return Ok(());
        }
        if let Err(e) = bot::run().await {
            tracing::error!(error = %e, "bot terminated with error");
            return Err(e);
        }
        return Ok(());
    }

    if args.iter().any(|a| a == "--no-interactive" || a == "-y") {
        if let Err(e) = bot::run().await {
            tracing::error!(error = %e, "bot terminated with error");
            return Err(e);
        }
        return Ok(());
    }

    // Jika config.toml belum ada sama sekali, jalankan wizard otomatis
    if !config::config_exists() {
        println!();
        println!("⚠️  File config.toml belum ditemukan.");
        println!("Memulai Setup Wizard untuk konfigurasi pertama kali...");
        wizard::run_setup()?;
    }

    // Interactive Menu Loop
    loop {
        println!();
        println!("🎬 TIX.ID Bot");
        println!("========================================");
        println!("  [1] Mulai Bot (War Tiket)");
        println!("  [2] Ubah Pengaturan (Setup Wizard)");
        println!("  [3] Keluar");
        println!("========================================");
        print!("Pilih menu [1-3] (default: 1): ");
        let _ = io::stdout().flush();

        let mut choice = String::new();
        let bytes = io::stdin().read_line(&mut choice)?;
        if bytes == 0 {
            // EOF (e.g. piped stdin), default to running bot
            if let Err(e) = bot::run().await {
                tracing::error!(error = %e, "bot terminated with error");
                return Err(e);
            }
            break;
        }

        let choice = choice.trim();
        match choice {
            "1" | "" => {
                if let Err(e) = bot::run().await {
                    tracing::error!(error = %e, "bot terminated with error");
                    return Err(e);
                }
                break;
            }
            "2" => {
                let _ = wizard::run_setup();
            }
            "3" => {
                println!("Sampai jumpa! 👋");
                return Ok(());
            }
            _ => {
                println!("Pilihan tidak valid, silakan ketik 1, 2, atau 3.");
            }
        }
    }

    Ok(())
}
