mod api;
mod beacon;
mod bot;
mod client;
mod config;
mod fuzzy;
mod logger;
mod models;
mod notifier;
mod seat_selector;
mod theater_selector;
mod wizard;
pub mod metrics;

use std::io::{self, Write};

async fn run_bot_safe() -> anyhow::Result<()> {
    tokio::select! {
        res = bot::run() => {
            if let Err(e) = res {
                tracing::error!(error = %e, "bot terminated with error");
                return Err(e);
            }
        }
        _ = tokio::signal::ctrl_c() => {
            println!("\n🛑 Bot dihentikan (Ctrl + C). Sampai jumpa! 👋");
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Guard must live until end of main so the background writer flushes on exit.
    let _log_guard = logger::init();

    // Check CLI flags
    let args: Vec<String> = std::env::args().collect();
    let verbose_cli = args.iter().any(|a| a == "--verbose" || a == "-v");
    let cfg_initial = config::load().unwrap_or_default();
    api::set_verbose_timing(verbose_cli || cfg_initial.debug.verbose_timing);

    if args.iter().any(|a| a == "--setup" || a == "-s") {
        if wizard::run_setup()? {
            run_bot_safe().await?;
        }
        return Ok(());
    }

    if args.iter().any(|a| a == "--no-interactive" || a == "-y") {
        run_bot_safe().await?;
        return Ok(());
    }

    // Jika config.toml belum ada sama sekali, jalankan wizard otomatis
    if !config::config_exists() {
        println!();
        println!("⚠️  File config.toml belum ditemukan.");
        println!("Memulai Setup Wizard untuk konfigurasi pertama kali...");
        if wizard::run_setup()? {
            return run_bot_safe().await;
        }
    }

    // Interactive Menu Loop
    loop {
        let cfg = config::load().unwrap_or_default();
        api::set_verbose_timing(verbose_cli || cfg.debug.verbose_timing);

        println!();
        println!("🎬 TIX.ID Bot [v1.0.0-beta]");
        println!("--------------------------------------------------");
        wizard::print_config_status(&cfg);
        println!("--------------------------------------------------");
        println!("  [1] Mulai Bot");
        println!("  [2] Quick Setup Target (Ganti Film / Mode)");
        println!("  [3] Full Setup Wizard (Konfigurasi Lengkap)");
        println!("  [4] Keluar");
        println!("--------------------------------------------------");
        print!("Pilih menu [1-4] (default: 1): ");
        let _ = io::stdout().flush();

        let mut choice = String::new();
        let bytes = io::stdin().read_line(&mut choice)?;
        if bytes == 0 {
            // EOF (e.g. piped stdin), default to running bot
            run_bot_safe().await?;
            break;
        }

        let choice = choice.trim();
        match choice {
            "1" | "" => {
                run_bot_safe().await?;
                break;
            }
            "2" => {
                if wizard::run_quick_setup()? {
                    run_bot_safe().await?;
                    break;
                }
            }
            "3" => {
                if wizard::run_setup()? {
                    run_bot_safe().await?;
                    break;
                }
            }
            "4" => {
                println!("Sampai jumpa! 👋");
                return Ok(());
            }
            _ => {
                println!("Pilihan tidak valid, silakan ketik 1, 2, 3, atau 4.");
            }
        }
    }

    Ok(())
}
