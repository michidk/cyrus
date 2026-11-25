//! Logical actors execute independently of geometry through explicit public stages.

use cyrus_script::script::test_support::{GameplayGlobalsTestExt, ScriptWorldTestExt};
mod support;
use cyrus_assets::script::PlacementType;
use cyrus_assets::script::TauntType;
use cyrus_assets::script::*;
use cyrus_assets::surface::Surface;
use cyrus_collision::ground::SurfaceClass;
use cyrus_script::script::AiType;
use cyrus_script::script::CollisionFlags;
use cyrus_script::script::CombatFlags;
use cyrus_script::script::CombatMessageCode;
use cyrus_script::script::JumpMotion;
use cyrus_script::script::LivingState;
use cyrus_script::script::MotionDirection;
use cyrus_script::script::PointPhase;
use cyrus_script::script::StepDirection;
use cyrus_script::script::{ExecutorState, ScriptStage, ScriptWorld};
use cyrus_soup::program::*;
use cyrus_soup::{AnimationGroup, Argument, CallResult, Host, Target, Thread};
use cyrus_soup::{Attribute, Attributes};
use cyrus_soup::{Native, Property};
use cyrus_world::heading::wrap;
use std::collections::BTreeMap;
use support::ordinary_collision_scene;

#[path = "script_registry/effects.rs"]
mod environmental_effect_tests;

#[path = "script_registry/water_model.rs"]
mod water_model_tests;

#[path = "script_registry/player_tilt.rs"]
mod player_tilt_tests;

#[path = "script_registry/delete_broadcast.rs"]
mod delete_broadcast_tests;

#[path = "script_registry/player_force_talk.rs"]
mod player_force_talk_tests;

#[path = "script_registry/turn_selected_player.rs"]
mod turn_selected_player_tests;

#[path = "script_registry/move_above_me.rs"]
mod move_above_me_tests;

#[path = "script_registry/point_at.rs"]
mod point_at_tests;

#[path = "script_registry/native_source_order.rs"]
mod native_source_order_tests;

#[path = "script_registry/parameter_diagnostics.rs"]
mod parameter_diagnostics_tests;

#[path = "script_registry/header_hook.rs"]
mod header_hook_tests;

#[path = "script_registry/collision_extents.rs"]
mod collision_extents_tests;

#[path = "script_registry/forward_surface_transition.rs"]
mod forward_surface_transition_tests;

#[path = "script_registry/pose_properties.rs"]
mod pose_properties_tests;

#[path = "script_registry/task_properties.rs"]
mod task_properties_tests;

#[path = "script_registry/anim_properties.rs"]
mod anim_properties_tests;

#[path = "script_registry/zap_health.rs"]
mod zap_health_tests;

#[path = "script_registry/movement_tasks.rs"]
mod movement_tasks_tests;

#[path = "script_registry/player_queries.rs"]
mod player_queries_tests;

#[path = "script_registry/flat_texture.rs"]
mod flat_texture_tests;

#[path = "script_registry/jump_fall_guards.rs"]
mod jump_fall_guards_tests;

#[path = "script_registry/rope_terminal.rs"]
mod rope_terminal_tests;

#[path = "script_registry/camera_contacts.rs"]
mod camera_contacts_tests;

#[path = "script_registry/rope_attachment.rs"]
mod rope_attachment_tests;

#[path = "script_registry/carrying_player.rs"]
mod carrying_player_tests;

#[path = "script_registry/actor_update_skip.rs"]
mod actor_update_skip_tests;

#[path = "script_registry/jump_movement.rs"]
mod jump_movement_tests;

#[path = "script_registry/attack_interruption.rs"]
mod attack_interruption_tests;

#[path = "script_registry/contact_update.rs"]
mod contact_update_tests;

#[cfg(feature = "original-save-import")]
#[path = "script_registry/original_pose_binding.rs"]
mod original_pose_binding_tests;

#[cfg(feature = "original-save-import")]
#[path = "script_registry/original_actor_state.rs"]
mod original_actor_state_tests;

#[cfg(feature = "original-save-import")]
#[path = "script_registry/original_globals.rs"]
mod original_globals;

#[cfg(feature = "original-save-import")]
#[path = "script_registry/original_world.rs"]
mod original_world;

#[path = "script_registry/activation.rs"]
mod activation_tests;

#[path = "script_registry/ai_progress.rs"]
mod ai_progress_tests;
use ai_progress_tests::wander_world;

#[path = "script_registry/attack.rs"]
mod attack_tests;

#[path = "script_registry/attributes.rs"]
mod attributes_tests;
use attributes_tests::{attribute_cache, attribute_call};

#[path = "script_registry/body.rs"]
mod body_tests;

#[path = "script_registry/bounce.rs"]
mod bounce_tests;
use bounce_tests::animated_bounce_pair_at;

#[path = "script_registry/bounds.rs"]
mod bounds_tests;

#[path = "script_registry/combat_decision.rs"]
mod combat_decision_tests;

#[path = "script_registry/contact.rs"]
mod contact_tests;

#[path = "script_registry/coordination.rs"]
mod coordination_tests;
use coordination_tests::coordination_message_fixture;

#[path = "script_registry/counter.rs"]
mod counter_tests;
use counter_tests::{counter_fixture, counter_fixture_state, counter_fixture_timing};

#[path = "script_registry/damage.rs"]
mod damage_tests;

#[path = "script_registry/death_exit.rs"]
mod death_exit_tests;

#[path = "script_registry/defence.rs"]
mod defence_tests;

#[path = "script_registry/diagnostics.rs"]
mod diagnostics_tests;

#[path = "script_registry/dialogue.rs"]
mod dialogue_tests;

#[path = "script_registry/direct_point.rs"]
mod direct_point_tests;
use direct_point_tests::retained_point_fixture;

#[path = "script_registry/dragon.rs"]
mod dragon_tests;

#[path = "script_registry/effect_dispatch.rs"]
mod effect_dispatch_tests;

#[path = "script_registry/facing.rs"]
mod facing_tests;
use facing_tests::facing_world;

#[path = "script_registry/falling.rs"]
mod falling_tests;

#[path = "script_registry/family.rs"]
mod family_tests;

#[path = "script_registry/fixtures.rs"]
mod fixtures;
use fixtures::{
    NoServices, assign_call, bind_scheduler_environment, cache, run_native_calls, run_native_world,
    run_native_world_with,
};

#[path = "script_registry/flat.rs"]
mod flat_tests;

#[path = "script_registry/global_flag.rs"]
mod global_flag_tests;

#[path = "script_registry/ground.rs"]
mod ground_tests;

#[path = "script_registry/hazard.rs"]
mod hazard_tests;

#[path = "script_registry/health.rs"]
mod health_tests;
use health_tests::{
    player_health_fixture, player_health_fixture_with, replace_health_test_animation,
};

#[path = "script_registry/inventory.rs"]
mod inventory_tests;
use inventory_tests::{inventory_asset, item_count_world, run_item_count_native};

#[path = "script_registry/item_use.rs"]
mod item_use_tests;

#[path = "script_registry/jump.rs"]
mod jump_tests;
use jump_tests::script_jump_fixture;

#[path = "script_registry/ledge.rs"]
mod ledge_tests;

#[path = "script_registry/ledge_states.rs"]
mod ledge_states_tests;
use ledge_states_tests::{
    advance_selected_hang_script, advance_selected_hang_stage,
    selected_hang_classification_fixture, selected_hang_fixture_with_point_task,
};

#[path = "script_registry/lifecycle.rs"]
mod lifecycle_tests;
use lifecycle_tests::revisit_test_app;

#[path = "script_registry/light.rs"]
mod light_tests;
use light_tests::{light_maintenance_scripts, light_maintenance_update, light_maintenance_world};

#[path = "script_registry/lineup.rs"]
mod lineup_tests;
use lineup_tests::scheduler_round_with;

#[path = "script_registry/linked.rs"]
mod linked_tests;

#[path = "script_registry/loading.rs"]
mod loading_tests;

#[path = "script_registry/map_marker.rs"]
mod map_marker_tests;

#[path = "script_registry/messages.rs"]
mod messages_tests;
use messages_tests::{message_call_scripts, message_query_scripts};

#[path = "script_registry/model.rs"]
mod model_tests;
use model_tests::{static_model_fixture, static_model_fixture_at, static_model_fixture_session};

#[path = "script_registry/navigation.rs"]
mod navigation_tests;
use navigation_tests::{
    free_node_fixture, navigation_owner_fixture, restore_navigation_owner, walk_node,
};

#[path = "script_registry/obstacle.rs"]
mod obstacle_tests;
use obstacle_tests::ordinary_forward_wall_model;

#[path = "script_registry/ordinary.rs"]
mod ordinary_tests;

#[cfg(feature = "original-save-import")]
#[path = "script_registry/original_import.rs"]
mod original_import_tests;

#[cfg(feature = "original-save-import")]
use original_import_tests::synthetic_original_player;

#[path = "script_registry/overlap.rs"]
mod overlap_tests;
use overlap_tests::{
    overlap_response_fixture, overlap_response_fixture_tweaked, overlap_response_fixture_with,
    static_overlap_scene,
};

#[path = "script_registry/placement.rs"]
mod placement_tests;

#[path = "script_registry/player.rs"]
mod player_tests;

#[path = "script_registry/player_queries_dispatch.rs"]
mod player_queries_dispatch_tests;

#[path = "script_registry/player_walk.rs"]
mod player_walk_tests;
use player_walk_tests::{
    player_walking_fixture, player_walking_fixture_with, player_walking_fixture_with_animation,
};

#[path = "script_registry/point.rs"]
mod point_tests;

#[path = "script_registry/prelight.rs"]
mod prelight_tests;

#[path = "script_registry/prevalidation.rs"]
mod prevalidation_tests;

#[path = "script_registry/projectile.rs"]
mod projectile_tests;

#[path = "script_registry/property.rs"]
mod property_tests;
use property_tests::heading_property_scripts;

#[path = "script_registry/queued.rs"]
mod queued_tests;

#[path = "script_registry/quick_keys.rs"]
mod quick_keys_tests;

#[path = "script_registry/random.rs"]
mod random_tests;
use random_tests::random_scripts;

#[path = "script_registry/recovery.rs"]
mod recovery_tests;

#[path = "script_registry/rope.rs"]
mod rope_tests;
use rope_tests::{
    rope_classification_fixture, rope_classification_fixture_with, tick_rope_classification,
    tick_rope_classification_with_timing,
};

#[path = "script_registry/routing.rs"]
mod routing_tests;
use routing_tests::{
    actor_reference_scripts, actor_reference_step, add_standalone_receivers, routed_cache,
};

#[path = "script_registry/script_control.rs"]
mod script_control_tests;

#[path = "script_registry/separate.rs"]
mod separate_tests;

#[path = "script_registry/session.rs"]
mod session_tests;

#[path = "script_registry/sight.rs"]
mod sight_tests;

#[path = "script_registry/slope.rs"]
mod slope_tests;

#[path = "script_registry/sound.rs"]
mod sound_tests;

#[path = "script_registry/spacing.rs"]
mod spacing_tests;
use spacing_tests::{movement_snapshot, spacing_fixture, spacing_fixture_group};

#[path = "script_registry/spatial.rs"]
mod spatial_tests;

#[path = "script_registry/terrain_sink.rs"]
mod terrain_sink_tests;

#[path = "script_registry/timing.rs"]
mod timing_tests;

#[path = "script_registry/torch.rs"]
mod torch_tests;

#[path = "script_registry/transition.rs"]
mod transition_tests;

#[path = "script_registry/vertical.rs"]
mod vertical_tests;

#[path = "script_registry/wait.rs"]
mod wait_tests;
use wait_tests::{set_flag, wait, wait_cache};

#[path = "script_registry/walk.rs"]
mod walk_tests;
use walk_tests::movement_fixture;

#[path = "script_registry/weapon.rs"]
mod weapon_tests;
use weapon_tests::display_model_fixture;
