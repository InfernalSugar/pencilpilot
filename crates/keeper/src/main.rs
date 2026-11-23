//! `pencilpilot-keeper [keeper.toml]`: the cloud's always-on member (ADR-0014).
//!
//! `pencilpilot-keeper --id [keeper.toml]` prints its id (made on the first run): the website
//! signs a pass for it (apps/web/scripts/keeper-pass.mjs), and apps reach it at
//! `<id>@<host>:<port>`.

use pencilpilot_keeper::{Config, Keeper};
use pencilpilot_share::Identity;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "pencilpilot_keeper=info".into()),
        )
        .init();
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let print_id = args.first().is_some_and(|a| a == "--id");
    if print_id {
        args.remove(0);
    }
    let path = args
        .first()
        .cloned()
        .unwrap_or_else(|| "/etc/pencilpilot-keeper/keeper.toml".into());
    let cfg = match Config::read(path.as_ref()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    if print_id {
        match Identity::load_or_create(&cfg.key) {
            Ok(id) => println!("{}", id.id()),
            Err(e) => {
                eprintln!("{}: {e}", cfg.key.display());
                std::process::exit(1);
            }
        }
        return;
    }
    let keeper = match Keeper::start(&cfg).await {
        Ok(k) => k,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    // Ctrl+C, or systemd stopping it
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut term = signal(SignalKind::terminate()).expect("signals");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    let _ = tokio::signal::ctrl_c().await;
    keeper.close().await;
}
