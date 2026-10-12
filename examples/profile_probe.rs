//! Diagnostic: the profile view Spotify's clients read for a user, run
//! with the stored playback credential.
//!
//!   cargo run --example profile_probe -- spotify 50

use librespot_core::{Session, SessionConfig, cache::Cache};

fn main() -> anyhow::Result<()> {
    fastframe_log::Logging::new("spotifast", env!("CARGO_PKG_VERSION"))
        .filter("warn")
        .init()?;
    let user = std::env::args().nth(1);
    let limit = std::env::args()
        .nth(2)
        .and_then(|limit| limit.parse().ok())
        .unwrap_or(50);

    let dirs = spotifast::paths::AppDirs::discover();
    let cache = Cache::new::<&std::path::Path>(None, None, None, None)?.with_memory_credentials();

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async move {
        let store = spotifast::credentials::Store::new(dirs);
        let loaded = store
            .lease(spotifast::credentials::Slot::Playback)
            .load()
            .await?;
        if let Some(warning) = loaded.warning {
            eprintln!("{warning}");
        }
        let Some(spotifast::credentials::Grant::Playback(credentials)) = loaded.grant else {
            anyhow::bail!("Enable playback in Spotifast first");
        };
        let session = Session::new(SessionConfig::default(), Some(cache));
        session.connect(credentials, false).await?;
        let user = user.unwrap_or_else(|| session.username());
        let bytes = session
            .spclient()
            .get_user_profile(&user, Some(limit), Some(0))
            .await?;
        let json: serde_json::Value = serde_json::from_slice(&bytes)?;
        println!("{}", serde_json::to_string_pretty(&json)?);
        anyhow::Ok(())
    })?;
    Ok(())
}
