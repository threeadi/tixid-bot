mod api;
mod bot;
mod client;
mod config;
mod logger;
mod models;
mod seat_selector;
mod theater_selector;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Guard must live until end of main so the background writer flushes on exit.
    let _log_guard = logger::init();

    if let Err(e) = bot::run().await {
        tracing::error!(error = %e, "bot terminated with error");
        return Err(e);
    }
    Ok(())
}
