//! Public process regression for unattended New Game and ordinary input admission.
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct Game(Child);
impl Drop for Game {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn request(stream: &mut BufReader<TcpStream>, command: Value) -> Value {
    writeln!(stream.get_mut(), "{}", json!({"id":1,"cmd":command})).unwrap();
    let mut line = String::new();
    stream.read_line(&mut line).unwrap();
    let response: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(response["ok"], true, "{response}");
    response["result"].clone()
}

#[test]
fn new_game_headless_reaches_installed_world_and_advances_under_input() {
    run_new_game(true);
}

#[test]
fn new_game_headless_plays_intro_accepts_skip_and_reaches_installed_world() {
    run_new_game(false);
}

fn run_new_game(skip_movies: bool) {
    let Ok(install) = cyrus_assets::install::GameInstall::locate(None) else {
        eprintln!("skipped: no install (headless New Game)");
        return;
    };
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let config = std::env::temp_dir().join(format!(
        "cyrus-headless-new-game-{}-{skip_movies}",
        std::process::id()
    ));
    std::fs::create_dir_all(&config).unwrap();
    let log = std::fs::File::create(config.join("process.log")).unwrap();
    let mut game = Game(
        Command::new(env!("CARGO_BIN_EXE_cyrus"))
            .args(["--headless", "--new-game", "--volume", "0"])
            .args(if skip_movies {
                vec!["--replay-step-micros", "16667"]
            } else {
                vec![]
            })
            .args(if skip_movies {
                vec!["--skip-movies"]
            } else {
                vec![]
            })
            .arg("--control")
            .arg(addr.to_string())
            .arg("--install")
            .arg(&install.root)
            .env("CYRUS_CONFIG_DIR", &config)
            .env_remove("DISPLAY")
            .env_remove("WAYLAND_DISPLAY")
            .stdout(Stdio::from(log.try_clone().unwrap()))
            .stderr(Stdio::from(log))
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(240);
    let stream = loop {
        assert!(
            game.0.try_wait().unwrap().is_none(),
            "headless exited; see {}",
            config.display()
        );
        if let Ok(stream) = TcpStream::connect(addr) {
            break stream;
        }
        assert!(
            Instant::now() < deadline,
            "control timeout; see {}",
            config.display()
        );
        std::thread::sleep(Duration::from_millis(100));
    };
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .unwrap();
    let mut stream = BufReader::new(stream);
    let mut intro_frame = 0;
    let mut last_skip = Instant::now();
    let initial = loop {
        let state = request(&mut stream, json!({"type":"state"}));
        intro_frame = intro_frame.max(state["menu"]["movie"]["frame"].as_u64().unwrap_or(0));
        if !skip_movies && intro_frame > 1 && last_skip.elapsed() > Duration::from_secs(1) {
            request(
                &mut stream,
                json!({"type":"input","key":"Space","action":"tap"}),
            );
            last_skip = Instant::now();
        }
        if state["state"] == "InGame"
            && state["scripts"]["ordinary_updates"].as_u64().unwrap_or(0) > 0
        {
            break state;
        }
        assert!(
            Instant::now() < deadline,
            "world timeout: {state}; see {}",
            config.display()
        );
        std::thread::sleep(Duration::from_millis(100));
    };
    assert!(skip_movies || intro_frame > 1, "intro did not advance");
    eprintln!("skip_movies={skip_movies}, observed intro frame {intro_frame}");
    // The installed New Game menu definition chooses the world, rather than a test-owned scene table.
    let menu: cyrus_assets::menu::MenuAsset =
        serde_json::from_slice(&std::fs::read(config.join("cache/menu/book.menu")).unwrap())
            .unwrap();
    assert_eq!(initial["scripts"]["world"], menu.new_game.world);
    assert!(initial["player"].is_object(), "{initial}");
    let inspected = request(&mut stream, json!({"type":"scripts","offset":0,"limit":1}));
    assert_eq!(inspected["world"], initial["scripts"]["world"]);
    assert_eq!(inspected["flags"]["version"], 1);
    assert_eq!(inspected["player_position_q8"].as_array().unwrap().len(), 3);
    assert!(!inspected["flags"]["flags"].as_array().unwrap().is_empty());
    let advanced = if skip_movies {
        // A replay world is held between scheduled input boundaries, so updates run only while a
        // schedule owns the frame loop: hold the key for twelve updates through one.
        request(
            &mut stream,
            json!({"type":"schedule_input","events":[
                {"after_updates":0,"key":"ArrowUp","action":"press"},
                {"after_updates":12,"key":"ArrowUp","action":"release"}
            ]}),
        );
        let state = request(&mut stream, json!({"type":"state"}));
        assert!(
            state["scripts"]["ordinary_updates"].as_u64().unwrap()
                > initial["scripts"]["ordinary_updates"].as_u64().unwrap() + 2,
            "the scheduled hold did not advance the world: {state}"
        );
        state
    } else {
        request(
            &mut stream,
            json!({"type":"input","key":"ArrowUp","action":"press"}),
        );
        loop {
            let state = request(&mut stream, json!({"type":"state"}));
            if state["player_input"]["forward"] == true
                && state["scripts"]["ordinary_updates"].as_u64().unwrap_or(0)
                    > initial["scripts"]["ordinary_updates"].as_u64().unwrap() + 2
                && state["clock"]["game_seconds"].as_f64()
                    > initial["clock"]["game_seconds"].as_f64()
            {
                break state;
            }
            assert!(Instant::now() < deadline, "input/clock timeout: {state}");
            std::thread::sleep(Duration::from_millis(100));
        }
    };
    if skip_movies {
        let updates = advanced["scripts"]["ordinary_updates"].as_u64().unwrap()
            - initial["scripts"]["ordinary_updates"].as_u64().unwrap();
        let elapsed = advanced["clock"]["game_seconds"].as_f64().unwrap()
            - initial["clock"]["game_seconds"].as_f64().unwrap();
        // The clock advances in whole supplied steps. A held replay world runs its first update
        // of a schedule without a step, so the step count may trail the update count by one.
        let steps = elapsed / 0.016667;
        assert!(
            (steps - steps.round()).abs() < 0.02
                && steps.round() >= 12.0
                && steps.round() <= updates as f64,
            "replay clock did not follow supplied timing: {updates} updates, {elapsed} seconds"
        );
    }
    if !skip_movies {
        request(
            &mut stream,
            json!({"type":"input","key":"ArrowUp","action":"release"}),
        );
    }
    eprintln!(
        "headless New Game world {}, updates {} -> {}, clock {} -> {}",
        menu.new_game.world,
        initial["scripts"]["ordinary_updates"],
        advanced["scripts"]["ordinary_updates"],
        initial["clock"]["game_seconds"],
        advanced["clock"]["game_seconds"]
    );
    request(&mut stream, json!({"type":"quit"}));
    // Drop owns termination even when an assertion fails. Successful runs remove only their state.
    drop(game);
    std::fs::remove_dir_all(config).unwrap();
}
