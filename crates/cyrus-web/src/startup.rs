//! Browser launch opens the book unless an explicit shortcut requests an action.
use cyrus_app::launch::GameOptions;
use cyrus_startup::LaunchRequest;

pub fn launch(options: &GameOptions) -> LaunchRequest {
    LaunchRequest::new(options.new_game, options.movie, options.skip_movies)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cyrus_startup::LaunchAction;

    #[test]
    fn default_launch_opens_book_and_shortcuts_remain_explicit() {
        assert_eq!(launch(&GameOptions::default()).action, None);
        assert_eq!(
            launch(&GameOptions {
                skip_movies: true,
                ..Default::default()
            })
            .action,
            None
        );
        assert_eq!(
            launch(&GameOptions {
                new_game: true,
                ..Default::default()
            })
            .action,
            Some(LaunchAction::NewGame)
        );
        assert!(
            launch(&GameOptions {
                new_game: true,
                skip_movies: true,
                ..Default::default()
            })
            .skip_movies
        );
    }
}
