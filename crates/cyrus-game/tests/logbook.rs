//! Public reading-session contracts from the original logbook layout and input order.
use cyrus_game::logbook::{Logbook, LogbookInput};

#[test]
fn reading_starts_at_newest_text_and_clamps_both_scroll_steps() {
    let entries = vec!["An accepted journal entry.".to_owned(); 20];
    let mut book = Logbook::new(&entries).unwrap();
    assert_eq!((book.maximum(), book.offset()), (60, 60));
    assert_eq!(book.lines().first().unwrap().y, 70);
    assert_eq!(book.lines().last().unwrap().y, 450);
    book.update(LogbookInput {
        up: true,
        ..Default::default()
    });
    assert_eq!(book.offset(), 50);
    book.update(LogbookInput {
        page_up: true,
        ..Default::default()
    });
    assert_eq!(book.offset(), 0);
    book.update(LogbookInput {
        up: true,
        ..Default::default()
    });
    assert_eq!(book.offset(), 0);
    book.update(LogbookInput {
        page_down: true,
        ..Default::default()
    });
    assert_eq!(book.offset(), 50);
    book.update(LogbookInput {
        page_down: true,
        ..Default::default()
    });
    assert_eq!(book.offset(), 60);
}

#[test]
fn wrapping_searches_after_65_bytes_without_splitting_words_or_final_spaces() {
    let long_word = "a".repeat(65);
    let entries = vec![
        format!("7,{long_word} second line"),
        format!("{long_word} "),
    ];
    let book = Logbook::new(&entries).unwrap();
    let lines: Vec<_> = book
        .lines()
        .iter()
        .map(|line| (line.text.as_str(), line.style, line.y))
        .collect();
    assert_eq!(
        lines,
        vec![
            (long_word.as_str(), Some(b'7'), 70),
            ("second line", Some(b'7'), 82),
            (entries[1].as_str(), None, 102)
        ]
    );
    assert_eq!(book.maximum(), 0);
    let long_word = Logbook::new(&["x".repeat(200)]).unwrap();
    assert_eq!(long_word.lines().len(), 1);
}

#[test]
fn later_wraps_skip_65_bytes_after_the_replaced_space() {
    let first = "x".repeat(65);
    let second = "y".repeat(65);
    let book = Logbook::new(&[format!("{first} {second} tail")]).unwrap();
    assert_eq!(
        book.lines()
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>(),
        vec![first.as_str(), second.as_str(), "tail"]
    );
    assert_eq!(
        book.lines().iter().map(|line| line.y).collect::<Vec<_>>(),
        vec![70, 82, 94]
    );
    let early_space = format!("{} tail", "y".repeat(64));
    let book = Logbook::new(&[format!("{first} {early_space}")]).unwrap();
    assert_eq!(
        book.lines()
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>(),
        vec![first.as_str(), early_space.as_str()]
    );
}

#[test]
fn simultaneous_controls_follow_original_order_and_close_after_navigation() {
    let mut book = Logbook::new(&vec!["entry".to_owned(); 20]).unwrap();
    book.update(LogbookInput {
        home: true,
        end: true,
        up: true,
        ..Default::default()
    });
    assert_eq!(book.offset(), 50, "End follows Home, then up");
    book.update(LogbookInput {
        home: true,
        up: true,
        down: true,
        ..Default::default()
    });
    assert_eq!(book.offset(), 10, "up clamps before down");
    book.update(LogbookInput {
        home: true,
        up: true,
        page_down: true,
        ..Default::default()
    });
    assert_eq!(
        book.offset(),
        50,
        "a page signal changes both directions to 50"
    );
    book.update(LogbookInput {
        end: true,
        up: true,
        close: true,
        ..Default::default()
    });
    assert_eq!(book.offset(), 50);
    assert!(!book.is_open());
    book.update(LogbookInput {
        home: true,
        ..Default::default()
    });
    assert_eq!(book.offset(), 50);
}

#[test]
fn short_and_empty_books_do_not_scroll_and_storage_separators_need_decoding() {
    for entries in [vec![], vec![String::new()], vec!["one entry".to_owned()]] {
        let mut book = Logbook::new(&entries).unwrap();
        book.update(LogbookInput {
            down: true,
            page_down: true,
            end: true,
            ..Default::default()
        });
        assert_eq!((book.maximum(), book.offset()), (0, 0));
    }
    for entry in ["first\nsecond", "first\rsecond", "first\0second"] {
        let error = Logbook::new(&[entry.to_owned()]).err().unwrap().to_string();
        assert!(error.contains("logbook entry 0"));
    }
}
