//! Ordinary HUD update orchestration, input preferences, item admission and sprite projection.
use super::*;
use cyrus_script::player::input::Action;

#[allow(clippy::too_many_arguments)] // One Bevy system owning the overlay's input, state and nodes.
pub(super) fn update(
    script_hud: Res<cyrus_script::script::ScriptHud>,
    preferences_store: Res<cyrus_game::preferences::Preferences>,
    mut commands: Commands,
    assets: Res<AssetServer>,
    handle: Option<Res<HudHandle>>,
    data: Res<Assets<HudAsset>>,
    (state, toggles): (
        Option<ResMut<HudState>>,
        Option<ResMut<cyrus_script::player::input::PlayerToggles>>,
    ),
    viewport: Query<Entity, With<HudViewport>>,
    mut sprites: Query<(&HudSprite, &mut ImageNode, &mut Node, &mut Visibility)>,
    text_layer: Query<Entity, With<HudText>>,
    (keys, time, sound_settings): (
        Res<ButtonInput<KeyCode>>,
        Res<Time>,
        Option<Res<cyrus_assets::sound::SoundSettings>>,
    ),
    mut images: ResMut<Assets<Image>>,
    (mut actors, activation): (Option<ResMut<ScriptActors>>, Option<Res<ActivationInputs>>),
    compass: CompassInputs,
    controls: cyrus_script::player::input::Inputs,
) {
    let (Some(handle), Ok(viewport)) = (handle, viewport.single()) else {
        return;
    };
    let Some(hud) = data.get(&handle.0) else {
        return;
    };
    let Some(mut state) = state else {
        let frames = HudPresentationAssets::request(hud, &assets);
        let preferences = HudPreferences::load(&preferences_store).unwrap_or_else(|e| {
            warn!("hud: {e:#}");
            None
        });
        let mut state = HudState::new(hud, preferences, frames);
        if let Some(toggles) = toggles.as_ref() {
            state.walk_mode = toggles.walk_mode;
            state.auto_defend = toggles.auto_defend;
        } else {
            commands.insert_resource(cyrus_script::player::input::PlayerToggles {
                walk_mode: state.walk_mode,
                auto_defend: state.auto_defend,
            });
        }
        commands.insert_resource(state);
        commands.entity(viewport).with_children(|viewport| {
            for (z, sprite) in SPRITES.into_iter().enumerate() {
                viewport.spawn((
                    sprite,
                    StoredArtwork,
                    ImageNode::default(),
                    Node::default(),
                    Visibility::Hidden,
                    ZIndex(z as i32),
                ));
            }
        });
        return;
    };
    if compass.console.as_ref().is_some_and(|console| console.open) {
        return;
    }
    let inventory_draw = compass
        .inventory
        .as_ref()
        .is_some_and(|screen| screen.draw_admitted);
    if inventory_draw
        || compass
            .logbook
            .as_ref()
            .is_some_and(|screen| screen.inventory_open)
    {
        // Inventory advances only the candle once per admitted draw, even on the exit update.
        if !inventory_draw {
            return;
        }
        let health = state.health;
        state.candle.advance(health);
        // Each inventory draw pins visibility before closing can clear the logical timer.
        state.candle_visible = state.lockout == 0;
        for (sprite, mut image, mut node, mut visibility) in &mut sprites {
            if *sprite != HudSprite::Candle {
                continue;
            }
            if state.candle_visible {
                image.image = state.frames.candle[usize::from(state.candle.frame)].clone();
                node.set_if_neq(place(hud.candle, hud.candle_size));
            }
            visibility.set_if_neq(if state.candle_visible {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            });
        }
        return;
    }
    if compass
        .logbook
        .as_ref()
        .is_some_and(|screen| screen.reading())
    {
        state.logbook_frame = None;
        for (_, _, _, mut visibility) in &mut sprites {
            *visibility = Visibility::Hidden;
        }
        return;
    }
    let (powerups, notification) = script_hud.presentation();
    state.powerups = powerups;
    let start_notification = notification && state.logbook_frame.is_none();

    let Some(mut toggles) = toggles else {
        return;
    };
    let state = &mut *state;

    // Fixed, non-configurable keys: F8 candle mode, Tab walk mode, F7 auto-defend.
    let shortcuts_enabled = !compass
        .console
        .as_ref()
        .is_some_and(|console| console.blocked)
        && !compass.suppressed.as_ref().is_some_and(|input| input.0);
    update_shortcuts(
        state,
        &mut toggles,
        &preferences_store,
        &keys,
        shortcuts_enabled,
    );

    let dialogue_text = actors
        .as_ref()
        .and_then(|actors| actors.current.as_ref())
        .and_then(|world| world.globals().dialogue())
        .and_then(|dialogue| dialogue.presented_line())
        .filter(|_| {
            sound_settings
                .as_ref()
                .is_some_and(|settings| settings.subtitles)
        })
        .map(|(slot, text)| (slot, text.to_owned()));
    let mut world = actors.as_mut().and_then(|actors| actors.current.as_mut());
    if world
        .as_mut()
        .is_some_and(|world| world.globals_mut().take_candle_show_request())
    {
        script_hud.reset_health_feedback();
    }
    // The menu is drawn while its active word is set; each draw also remembers the highlighted text.
    let mut menu_text = Vec::new();
    if let Some(world) = world.as_mut() {
        let globals = world.globals();
        let mut remembered = None;
        if globals.menu_state() == 1
            && let Some(dialogue) = globals.dialogue()
        {
            menu_text = menu_lines(hud, dialogue.settings().menu_start, globals.menu());
            remembered = globals.menu().highlighted_text().map(str::to_owned);
        }
        if let Some(text) = remembered {
            world.globals_mut().remember_choice(text);
        }
    }
    let activation = activation.as_deref().filter(|inputs| {
        world
            .as_ref()
            .is_some_and(|world| world.generation() == inputs.player.generation)
    });
    state.lockout = world
        .as_ref()
        .and_then(|world| world.globals().lockout())
        .unwrap_or_else(|| activation.map_or(0, |inputs| inputs.lockout));
    let player_state = activation
        .and_then(|inputs| {
            inputs
                .actors
                .get(&inputs.player.index)
                .copied()
                .map(|actor| (inputs.player, actor))
        })
        .or_else(|| {
            if activation.is_some() {
                return None;
            }
            world.as_ref()?.player_activation_state().ok()
        });
    let on_edge = player_state.is_some_and(|(_, actor)| actor.state.is_ledge());
    if let Some((id, actor)) = player_state
        && let Some(player) = world
            .as_ref()
            .and_then(|world| world.actors().get(id.index))
    {
        let attributes = player.attributes();
        state.player_type = attributes[Attribute::PlayerType];
        let displayed = world
            .as_ref()
            .and_then(|world| world.globals().selected_health())
            .map_or(attributes[Attribute::Health], |health| {
                health.displayed() as u8
            });
        state.set_health(displayed, &script_hud);
        state.set_sword_drawn(
            player
                .control_state()
                .map_or(actor.weapon_drawn, |control| control.weapon_drawn),
            &script_hud,
        );
    }
    let compass_eligible = match (world.as_ref(), activation, player_state) {
        (Some(world), Some(inputs), _) => world
            .actors()
            .get(inputs.player.index)
            .is_some_and(|player| inputs.compass_eligible(player).unwrap_or(false)),
        (Some(world), None, Some((id, actor))) => actor.compass_eligible(
            world.actors()[id.index].attributes()[Attribute::HoldingWeapon],
            script_hud.ready(),
            false,
        ),
        _ => false,
    };

    // Preservation HUD facts: Glide swaps at 60 Hz; keep that animation rate independent of rendering.
    state.elapsed += time.delta_secs();
    let period = 1.0 / 60.0;
    while state.elapsed >= period {
        state.elapsed -= period;
        state.advance_frame();
    }
    if start_notification {
        state.logbook_entry();
    }
    let ticks = compass
        .clock
        .as_ref()
        .filter(|clock| clock.is_changed())
        .map_or(0, |clock| clock.step().nominal_ticks);

    state.candle_visible = script_hud.advance_health_feedback(
        state.sword_drawn,
        state.lockout,
        state.candle_mode,
        ticks,
    );

    // Select the overlay path before cycling; timer zero crossings keep their entry path.
    use cyrus_script::player::input::Action;
    let cycling = controls.pressed(Action::NextItem) || controls.pressed(Action::PreviousItem);
    let overlay = activation_overlay(state, hud, &script_hud, world, activation, ticks, cycling);
    let (inventory, live_items) = inventory_items(state, hud, &script_hud, &controls, &overlay);

    let item_path = matches!(&overlay, ActivationOverlay::Item { .. });
    let (shown_item, prompt) = match overlay {
        ActivationOverlay::Hidden | ActivationOverlay::Item { visible: false, .. } => (None, None),
        ActivationOverlay::Prompt(text) => (None, Some(text)),
        ActivationOverlay::Item { visible: true, .. } => (
            live_items.iter().find(|i| Some(i.id) == state.selected),
            None,
        ),
    };
    state.prompt = prompt.clone();
    state.count = shown_item.map(|i| i.count);

    // The original consumes use once per hold, independently of drawing its compass.
    let owns_compass = inventory
        .as_ref()
        .is_some_and(|items| items.counts.get(&0).is_some_and(|count| *count > 0));
    let attempt = if state.lockout == 1
        || on_edge
        || compass
            .suppressed
            .as_ref()
            .is_some_and(|suppressed| suppressed.0)
        || compass.console.as_ref().is_some_and(|console| console.open)
        // Readiness rejects after consuming use, even when it hides the item overlay.
        || (!item_path && activation.is_none_or(|inputs| inputs.item_system_ready))
    {
        cyrus_script::script::CompassUse::Blocked
    } else if state.selected == Some(0)
        && owns_compass
        && state.player_type != 1
        && compass_eligible
    {
        cyrus_script::script::CompassUse::Accepted
    } else {
        cyrus_script::script::CompassUse::Rejected
    };
    state.compass_on = script_hud.compass_input(controls.pressed(Action::Use), attempt);

    // Compass inputs: the placed player's heading and the loaded world's WORLD.INI ID.
    state.heading = compass.player.single().map_or(0, |pose| pose.heading);
    state.world = compass
        .request
        .as_deref()
        .zip(compass.scenes.as_deref())
        .and_then(|(request, scenes)| scenes.get(&request.0))
        .map(|scene| scene.world);

    // Compass: owning item 0 and its display flag.
    state.compass_frame =
        (state.lockout != 1 && state.player_type != 1 && owns_compass && state.compass_on).then(
            || {
                let world = state.world.and_then(|w| hud.world_compass.get(&w));
                compass_frame(state.heading, world.copied().unwrap_or(0))
            },
        );

    for notice in &mut state.notices {
        *notice = notice.saturating_sub(ticks);
    }

    for (sprite, mut image, mut node, mut visibility) in &mut sprites {
        let target = sprite_target(*sprite, state, hud, shown_item, &assets);
        let Some((frame, at, size)) = target else {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        };
        if image.image != frame {
            image.image = frame;
        }
        node.set_if_neq(place(at, size));
        visibility.set_if_neq(Visibility::Inherited);
    }

    // Text drawn last: none of its lines overlaps a later bitmap in the original order.
    let health = (state.candle_visible && state.candle_mode == 2).then_some(state.health);
    let mut text = text_lines(hud, health, shown_item);
    for (notice, y) in [(0, 10), (1, 30)] {
        if state.notices[notice] > 0 {
            let on = [state.walk_mode, state.auto_defend][notice];
            let line = hud.notices[notice][usize::from(on)].clone();
            text.push((line.clone(), centred(hud, &line, 320, y)));
        }
    }
    if let Some(prompt) = prompt {
        text.push((prompt, hud.prompt));
    }
    if state.redraw_text
        || text != state.text
        || dialogue_text != state.dialogue_text
        || menu_text != state.menu_text
    {
        for entity in &text_layer {
            commands.entity(entity).despawn();
        }
        if !text.is_empty() || dialogue_text.is_some() || !menu_text.is_empty() {
            let image = images.add(text_image(hud, &text, dialogue_text.as_ref(), &menu_text));
            commands.entity(viewport).with_child((
                HudText,
                ImageNode::new(image),
                place([0, 0], [640, 480]),
                ZIndex(SPRITES.len() as i32),
            ));
        }
        state.redraw_text = false;
        state.text = text;
        state.dialogue_text = dialogue_text;
        state.menu_text = menu_text;
    }
}

fn update_shortcuts(
    state: &mut HudState,
    toggles: &mut ResMut<cyrus_script::player::input::PlayerToggles>,
    preferences_store: &cyrus_game::preferences::Preferences,
    keys: &ButtonInput<KeyCode>,
    shortcuts_enabled: bool,
) {
    let mut toggled = false;
    if shortcuts_enabled && keys.just_pressed(KeyCode::F8) {
        state.candle_mode = (state.candle_mode + 1) % 3;
        toggled = true;
    }
    for (notice, key) in [KeyCode::Tab, KeyCode::F7].into_iter().enumerate() {
        if shortcuts_enabled && keys.just_pressed(key) {
            let flag = if notice == 0 {
                &mut toggles.walk_mode
            } else {
                &mut toggles.auto_defend
            };
            *flag = !*flag;
            state.notices[notice] = NOTICE_TICKS;
            toggled = true;
        }
    }
    state.walk_mode = toggles.walk_mode;
    state.auto_defend = toggles.auto_defend;
    if toggled {
        let preferences = HudPreferences {
            candle_mode: state.candle_mode,
            walk_mode: state.walk_mode,
            auto_defend: state.auto_defend,
        };
        if let Err(e) = preferences.save(preferences_store) {
            warn!("hud: {e:#}");
        }
    }
}

fn activation_overlay(
    state: &mut HudState,
    hud: &HudAsset,
    script_hud: &cyrus_script::script::ScriptHud,
    world: Option<&mut cyrus_script::script::ScriptWorld>,
    activation: Option<&ActivationInputs>,
    ticks: u32,
    cycling: bool,
) -> ActivationOverlay {
    let fallback_overlay = if state.player_type == 1 {
        ActivationOverlay::Hidden
    } else {
        ActivationOverlay::Item {
            visible: true,
            cycle_allowed: !state.sword_drawn,
        }
    };
    match (world, activation) {
        (Some(world), Some(inputs)) => {
            match world.activation_overlay(inputs, ticks, cycling, hud.override_ticks) {
                Ok(overlay) => {
                    let attributes = world.actors()[inputs.player.index].attributes();
                    state.player_type = attributes[Attribute::PlayerType];
                    let displayed = world
                        .globals()
                        .selected_health()
                        .map_or(attributes[Attribute::Health], |health| {
                            health.displayed() as u8
                        });
                    state.set_health(displayed, script_hud);
                    overlay
                }
                Err(error) => {
                    warn_once!("HUD activation: {error:#}");
                    ActivationOverlay::Hidden
                }
            }
        }
        (Some(world), None) => {
            let pending = world.activation().prompt().is_some();
            world
                .ordinary_activation_overlay(script_hud, ticks, cycling, hud.override_ticks)
                .unwrap_or_else(|error| {
                    if pending {
                        warn_once!("HUD ordinary activation: {error:#}");
                        ActivationOverlay::Hidden
                    } else {
                        fallback_overlay
                    }
                })
        }
        _ => fallback_overlay,
    }
}

fn inventory_items(
    state: &mut HudState,
    hud: &HudAsset,
    script_hud: &cyrus_script::script::ScriptHud,
    controls: &cyrus_script::player::input::Inputs,
    overlay: &ActivationOverlay,
) -> (Option<cyrus_script::script::InventoryView>, Vec<HudItem>) {
    // Item keys step once per press. A press held while a prompt is drawn steps at the later
    // item-path update (preservation HUD rules); a mouse binding reads for one update only.
    let held = (
        controls.pressed(Action::PreviousItem),
        controls.pressed(Action::NextItem),
    );
    if held == (false, false) {
        state.item_press_consumed = false;
    }
    let cycle = if !state.item_press_consumed
        && matches!(
            overlay,
            ActivationOverlay::Item {
                cycle_allowed: true,
                ..
            }
        ) {
        match held {
            (true, false) => Some(false),
            (false, true) => Some(true),
            _ => None,
        }
    } else {
        None
    };
    state.item_press_consumed |= cycle.is_some();
    let inventory = script_hud.inventory(cycle);
    state.selected = inventory.as_ref().and_then(|items| items.selected);
    let live_items: Vec<_> = hud
        .items
        .iter()
        .filter_map(|item| {
            let count = inventory.as_ref()?.counts.get(&item.id).copied()?;
            (count > 0).then(|| HudItem {
                count,
                ..item.clone()
            })
        })
        .collect();

    (inventory, live_items)
}

fn sprite_target(
    sprite: HudSprite,
    state: &HudState,
    hud: &HudAsset,
    shown_item: Option<&HudItem>,
    assets: &AssetServer,
) -> Option<(Handle<Image>, [i32; 2], [u16; 2])> {
    let frames = &state.frames;
    match sprite {
        HudSprite::Compass => state.compass_frame.map(|f| {
            (
                frames.compass[usize::from(f)].clone(),
                hud.compass,
                hud.compass_size,
            )
        }),
        HudSprite::Candle => state.candle_visible.then(|| {
            let frame = usize::from(state.candle.frame);
            (frames.candle[frame].clone(), hud.candle, hud.candle_size)
        }),
        HudSprite::Logbook => state.logbook_frame.map(|f| {
            (
                frames.logbook[usize::from(f)].clone(),
                hud.logbook,
                hud.logbook_size,
            )
        }),
        HudSprite::Powerup(slot) => {
            state.powerups[slot]
                .filter(|_| state.lockout == 0)
                .map(|(_, f)| {
                    let f = usize::from(f);
                    (
                        frames.powerup[f].clone(),
                        hud.powerup_slots[slot],
                        hud.powerup_sizes[f],
                    )
                })
        }
        HudSprite::Item => shown_item.map(|item| {
            let path = format!("cache://hud/item-{:02}.png", item.bitmap);
            (
                assets
                    .load_builder()
                    .with_settings(|settings: &mut ImageLoaderSettings| {
                        settings.sampler = ImageSampler::nearest();
                    })
                    .load(path),
                hud.pickup,
                item.size,
            )
        }),
    }
}
