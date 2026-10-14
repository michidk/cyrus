//! Public combat transport preserves class, receiver and independent cursor ownership.
use cyrus_game::script::{CombatMessage, CombatMessages, ScriptActorId};

fn actor(index: usize) -> ScriptActorId {
    ScriptActorId {
        generation: 1,
        index,
    }
}
fn message(receiver: Option<ScriptActorId>, code: i32) -> CombatMessage {
    CombatMessage {
        sender: actor(0),
        receiver,
        code,
    }
}

#[test]
fn class_and_address_filtering_preserve_independent_actor_reads() {
    let mut queues = CombatMessages::default();
    queues.send(7, message(Some(actor(1)), 10));
    queues.send(7, message(Some(actor(2)), 20));
    queues.send(7, message(None, 30));
    queues.send(8, message(Some(actor(1)), 40));
    let mut first = 0;
    let mut second = 0;
    assert_eq!(
        queues.consume(7, actor(1), &mut first).unwrap(),
        Some(message(Some(actor(1)), 10))
    );
    assert_eq!(first, 1);
    assert_eq!(
        queues.consume(7, actor(1), &mut first).unwrap(),
        Some(message(None, 30))
    );
    assert_eq!(first, 3);
    assert_eq!(
        queues.consume(7, actor(2), &mut second).unwrap(),
        Some(message(Some(actor(2)), 20))
    );
    assert_eq!(second, 2);
    assert_eq!(
        queues.consume(7, actor(2), &mut second).unwrap(),
        Some(message(None, 30))
    );
    assert_eq!(queues.consume(7, actor(1), &mut first).unwrap(), None);
    let mut other = 0;
    assert_eq!(
        queues.consume(8, actor(1), &mut other).unwrap(),
        Some(message(Some(actor(1)), 40))
    );
}

#[test]
fn eight_entry_wrap_zero_send_and_reset_follow_original_cursor_rules() {
    let mut queues = CombatMessages::default();
    let mut read = 0;
    for code in 1..=7 {
        queues.send(0, message(Some(actor(1)), code));
        assert_eq!(
            queues
                .consume(0, actor(1), &mut read)
                .unwrap()
                .unwrap()
                .code,
            code
        );
    }
    assert_eq!(read, 7);
    queues.send(0, message(Some(actor(1)), 0));
    assert_eq!(queues.consume(0, actor(1), &mut read).unwrap(), None);
    queues.send(0, message(Some(actor(1)), 8));
    assert_eq!(
        queues
            .consume(0, actor(1), &mut read)
            .unwrap()
            .unwrap()
            .code,
        8
    );
    assert_eq!(read, 0);
    for code in 9..=16 {
        queues.send(0, message(Some(actor(1)), code));
    }
    assert_eq!(queues.consume(0, actor(1), &mut read).unwrap(), None);
    queues.reset();
    queues.send(0, message(Some(actor(1)), 17));
    assert_eq!(
        queues
            .consume(0, actor(1), &mut read)
            .unwrap()
            .unwrap()
            .code,
        17
    );
    let mut invalid = 8;
    assert!(
        queues
            .consume(0, actor(1), &mut invalid)
            .unwrap_err()
            .to_string()
            .contains("read cursor 8")
    );
}
