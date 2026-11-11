//! Default browser entry uses installed policy after validating its reserved session slot.
use cyrus_app::launch::GameOptions;
use cyrus_assets::startup::StartupEntry;
use cyrus_startup::{LaunchAction, LaunchRequest};

#[cfg(any(target_arch = "wasm32", test))]
pub fn automatic(options: &GameOptions) -> bool {
    options.scene.is_none() && !options.skip_startup && !options.new_game && options.movie.is_none()
}

pub fn launch(options: &GameOptions, entry: Option<StartupEntry>) -> LaunchRequest {
    match entry {
        Some(StartupEntry::Introduction) => LaunchRequest::automatic_new_game(options.skip_movies),
        Some(StartupEntry::World) => {
            let mut launch = LaunchRequest::automatic_new_game(options.skip_movies);
            launch.action = Some(LaunchAction::NewGameWithoutMovie);
            launch
        }
        _ => LaunchRequest::new(options.new_game, options.movie, options.skip_movies),
    }
}

#[cfg(target_arch = "wasm32")]
pub async fn installed_entry() -> anyhow::Result<StartupEntry> {
    use anyhow::Context;
    let seed = js_sys::Reflect::get(&js_sys::global(), &"cyrusStartupPolicy".into())
        .map_err(|error| anyhow::anyhow!("browser startup policy: {error:?}"))?
        .as_string()
        .context("browser startup policy is not text")?;
    let policy: cyrus_assets::startup::StartupPolicy =
        serde_json::from_str(&seed).context("browser installed startup policy")?;
    let valid = if policy.entry(false) == StartupEntry::Introduction {
        super::session::has_valid_autosave().await?
    } else {
        false
    };
    Ok(policy.entry(valid))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn installed_entry_dispatch_preserves_explicit_shortcuts_and_movie_preference() {
        let options = GameOptions::default();
        assert!(automatic(&options));
        assert_eq!(
            launch(&options, Some(StartupEntry::Introduction)).action,
            Some(LaunchAction::NewGame)
        );
        assert_eq!(launch(&options, Some(StartupEntry::Book)).action, None);
        let world = launch(&options, Some(StartupEntry::World));
        assert_eq!(world.action, Some(LaunchAction::NewGameWithoutMovie));
        assert!(!world.skip_movies);
        let skip = GameOptions {
            skip_movies: true,
            ..Default::default()
        };
        assert!(launch(&skip, Some(StartupEntry::Introduction)).skip_movies);
        for options in [
            GameOptions {
                skip_startup: true,
                ..Default::default()
            },
            GameOptions {
                new_game: true,
                ..Default::default()
            },
            GameOptions {
                movie: Some(2),
                ..Default::default()
            },
            GameOptions {
                scene: Some("START".into()),
                ..Default::default()
            },
        ] {
            assert!(!automatic(&options));
            assert_eq!(
                launch(&options, None).action,
                LaunchRequest::new(options.new_game, options.movie, options.skip_movies).action
            );
        }
    }
}
