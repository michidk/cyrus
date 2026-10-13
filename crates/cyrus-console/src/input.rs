//! Keyboard focus and hidden-name capture, dispatching each Enter before the next event.
use crate::Console;
use bevy::ecs::message::MessageCursor;
use bevy::{
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput},
    },
    prelude::*,
};

pub(crate) fn capture(world: &mut World, mut cursor: Local<MessageCursor<KeyboardInput>>) {
    let events: Vec<_> = cursor
        .read(world.resource::<Messages<KeyboardInput>>())
        .cloned()
        .collect();
    let was_open = {
        let mut console = world.resource_mut::<Console>();
        if !console.enabled || console.blocked {
            console.reset_input();
            return;
        }
        if console.cursor_forced {
            console.cursor_forced = false;
        }
        console.open
    };
    for event in events {
        {
            let mut console = world.resource_mut::<Console>();
            if event.state == ButtonState::Released {
                console.release();
                continue;
            }
            let text = match &event.logical_key {
                Key::Character(text) => Some(text.as_str()),
                Key::Space => Some(" "),
                _ => None,
            };
            if !console.open && event.key_code == KeyCode::F12 && !event.repeat {
                console.reset_input();
                console.open = true;
            } else if console.open
                && !(event.repeat && matches!(event.key_code, KeyCode::F12 | KeyCode::Escape))
            {
                console.key(event.key_code, text);
            } else if !event.repeat {
                console.gameplay(event.key_code, text);
            }
        }
        crate::dispatch(world);
    }
    if (was_open || world.resource::<Console>().open)
        && let Some(mut keys) = world.get_resource_mut::<ButtonInput<KeyCode>>()
    {
        keys.reset_all();
    }
}
