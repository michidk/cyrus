//! Equipped weapon models, placed from the retained logical attachment pose.
use crate::{player_model::model_source, script_transform::scene_transform_at};
use bevy::{asset::AssetPath, prelude::*};
use cyrus_script::script::{Pose, ScriptActors, ScriptBinding};
use cyrus_world::{animation::AnimationPose, lifecycle::PreparedWorld};

/// Draws the hand or hilt model the script world retained for each bound actor, and the frame
/// transient models its display natives placed (they vanish when the next ordinary update empties
/// the list).
pub struct WeaponModelPlugin;
impl Plugin for WeaponModelPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            bind.after(cyrus_script::script::OrdinaryUpdates)
                .after(cyrus_world::animation::AnimationSystems),
        );
    }
}

/// A weapon model owned by one bound actor; the transform is in scene axes, outside the actor.
#[derive(Component)]
struct WeaponRoot {
    owner: Entity,
    source: AssetPath<'static>,
}

struct Wanted {
    source: AssetPath<'static>,
    transform: Transform,
    hidden: bool,
}

/// The models drawn for one actor: its equipped weapon and the frame's display attachments.
fn wanted(prepared: &PreparedWorld, actors: &ScriptActors, owner: &ScriptBinding) -> Vec<Wanted> {
    let Some(world) = actors.current.as_ref() else {
        return Vec::new();
    };
    if owner.actor.generation != world.generation() {
        return Vec::new();
    }
    let hidden = world
        .actors()
        .get(owner.actor.index)
        .and_then(|actor| actor.control_state())
        .is_some_and(|control| control.hidden);
    let equipped = world
        .equipped_weapon(owner.actor)
        .ok()
        .flatten()
        .map(|weapon| (&weapon.model, &weapon.geometry));
    let displayed = world
        .display_attachments()
        .iter()
        .filter(|display| display.actor == owner.actor)
        .map(|display| (&display.model, &display.geometry));
    equipped
        .into_iter()
        .chain(displayed)
        .map(|(model, geometry)| {
            let pose = Pose {
                position: [0; 3],
                angles: [0; 3],
                matrix: geometry.matrix,
            };
            Wanted {
                source: model_source(prepared, model, None),
                transform: Transform::from_matrix(Mat4::from(scene_transform_at(
                    pose,
                    geometry.position,
                ))),
                hidden,
            }
        })
        .collect()
}

fn bind(
    mut commands: Commands,
    assets: Res<AssetServer>,
    prepared: Option<Res<PreparedWorld>>,
    actors: Option<Res<ScriptActors>>,
    owners: Query<(Entity, &ScriptBinding), With<AnimationPose>>,
    mut roots: Query<(Entity, &WeaponRoot, &mut Transform, &mut Visibility)>,
) {
    let wanted: Vec<_> = prepared
        .as_deref()
        .zip(actors.as_deref())
        .map(|(prepared, actors)| {
            owners
                .iter()
                .flat_map(|(entity, binding)| {
                    wanted(prepared, actors, binding)
                        .into_iter()
                        .map(move |want| (entity, want))
                })
                .collect()
        })
        .unwrap_or_default();
    for (root, weapon, mut transform, mut visibility) in &mut roots {
        match wanted
            .iter()
            .find(|(owner, want)| *owner == weapon.owner && want.source == weapon.source)
        {
            Some((_, want)) => {
                *transform = want.transform;
                *visibility = if want.hidden {
                    Visibility::Hidden
                } else {
                    Visibility::Inherited
                };
            }
            None => commands.entity(root).despawn(),
        }
    }
    for (owner, want) in wanted {
        if roots
            .iter()
            .any(|(_, root, _, _)| root.owner == owner && root.source == want.source)
        {
            continue;
        }
        commands.spawn((
            WeaponRoot {
                owner,
                source: want.source.clone(),
            },
            WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(want.source))),
            want.transform,
            Visibility::Hidden,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script_transform::scene_transform_at;
    use cyrus_assets::{animation::Animations, script::Scripts};
    use cyrus_script::script::{AnimationData, ScriptInputs, ScriptStage, ScriptWorld};
    use serde_json::json;

    fn world() -> (ScriptWorld, Animations) {
        let mut value: serde_json::Value = serde_json::from_str(r##"{
            "version":30,"weapons":{},"combat_clank_effects":[null,null],"combat_defenses":[],"combat_attacks":[],"dialogue":{"menu_start":[35,25],"max_menu_items":300,"max_dialog_lines":20,"max_text_width":500,"menu_traverse_delay":4,"print_text":true,"use_speech":true,"max_distance":1600,"rtx_filename":"ENGLISH.RTX"},"player_distance_override":false,"updates":{"fall_death_ticks":144,"task_system":true,"animation_system":true,"disable_slaves":false,"disable_master_slaves":false,"dummy_model_manager":true},
            "clock":{"normal_frame_rate":12,"min_frame_rate":6,"max_frame_rate":300,"use_smooth_fps":true,"use_smooth_divisor":true},
            "actor_pool_capacity":10,"content_id":"","world":17,
            "definition":{"functions":[{"name":"NullFunction","param_count":0,"kind":"function"},{"name":"DisplayHandModel","param_count":4,"kind":"function"}],"flags":[],"references":[],"attributes":[]},
            "actors":{"STATIC":{"header_index":0,"code_origin":0,"instances":1,"instance_variables":[[]],"location_offsets":[],"attributes":[],
                "program":{"version":4,"entry":0,"strings":["system\\hand.3d"],"steps":{
                    "0":{"next":1,"operation":{"type":"call","call":{"function_id":1,"kind":"function","target":{"type":"me"},"function":"DisplayHandModel","arguments":[
                        {"type":"string","index":0},{"type":"integer","value":256},{"type":"integer","value":512},{"type":"integer","value":768}]}}},
                    "1":{"next":2,"operation":{"type":"end","target":0}}
                }}}},
            "instances":[{"object_id":0,"actor":"STATIC","object_type":1,"active":0,"model_name":"test","model_mode":0,"light_intensity":0,"light_radius":0,"light_rgb":[0,0,0],"map_position":[40,-60,80],"angles":[0,0,0]}],"unsupported":{}
        }"##)
        .unwrap();
        value["actors"]["STATIC"]["attributes"] = json!(vec![0u8; 256]);
        let mut scripts: Scripts = serde_json::from_value(value).unwrap();
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        world.prepare_spatial().unwrap();
        let mut data = Animations {
            version: 2,
            world: scripts.world,
            metadata: json!({
                "script_content_id": scripts.content_id,
                "actors": [{"script_name":"STATIC", "model_load_path":"static",
                    "raex":{"grip0":0,"grip1":1,"scabbard0":1,"scabbard1":0,"v_vertex":0,"v_size":0,"unknown_12":0}}]
            }),
            sections: Default::default(),
            models: Default::default(),
            unsupported: Default::default(),
        };
        data.models.insert(
            "HAND".into(),
            cyrus_assets::animation::AnimationModel {
                radius: Some(1.0),
                base_positions: vec![],
                base_normals: vec![],
                faces: vec![],
                corner_normals: vec![],
                encoding: "animated_i16".into(),
                positions: vec![vec![]],
                normals: vec![vec![]],
            },
        );
        (world, data)
    }

    fn inputs(data: &Animations) -> ScriptInputs<'_> {
        ScriptInputs {
            frame_scale: Some(256),
            nominal_ticks: Some(1),
            animations: Some(AnimationData(data)),
            ..Default::default()
        }
    }

    fn roots(app: &mut App) -> Vec<Transform> {
        app.world_mut()
            .query::<(&WeaponRoot, &Transform)>()
            .iter(app.world())
            .map(|(_, transform)| *transform)
            .collect()
    }

    #[test]
    fn display_attachments_are_drawn_at_their_pose_until_the_frame_ends() {
        let (mut world, data) = world();
        let id = world.id(0).unwrap();
        world
            .advance(
                id,
                ScriptStage::OrdinaryMain,
                inputs(&data),
                Some(&mut |_, _| Ok(())),
                None,
                100,
            )
            .unwrap();
        let shown = world.display_attachments()[0].clone();
        let expected = Transform::from_matrix(Mat4::from(scene_transform_at(
            Pose {
                position: [0; 3],
                angles: [0; 3],
                matrix: shown.geometry.matrix,
            },
            shown.geometry.position,
        )));
        let mut actors = ScriptActors::default();
        actors.current = Some(world);
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default(), WeaponModelPlugin))
            .init_asset::<WorldAsset>()
            .init_asset::<Animations>()
            .init_asset::<Scripts>();
        let prepared =
            PreparedWorld::from_cache("ISLAND".into(), app.world().resource::<AssetServer>());
        app.insert_resource(prepared);
        app.insert_resource(actors);
        app.world_mut().spawn((
            ScriptBinding {
                actor: id,
                source: Handle::<Animations>::default(),
            },
            AnimationPose {
                model: "STATIC".into(),
                frame: 0,
            },
        ));
        app.update();
        assert_eq!(roots(&mut app), [expected]);
        // The next ordinary update empties the frame's pool, so the model goes away.
        let mut random = cyrus_game::random::EngineRandom::default();
        app.world_mut()
            .resource_mut::<ScriptActors>()
            .current
            .as_mut()
            .unwrap()
            .update_ordinary(
                &data,
                [10, 30],
                cyrus_script::script::OrdinaryInputs {
                    scripts: inputs(&data),
                    scene: None,
                },
                &mut random,
                &mut |_, _| Ok(()),
                100,
            )
            .unwrap();
        app.update();
        assert!(roots(&mut app).is_empty());
    }
}
