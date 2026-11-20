use anyhow::{Context, Result, ensure};
use cyrus_collision::{
    candidates::{CandidateOwner, CollisionCandidates, FaceCandidate, ModelInstance},
    collision_mesh::BodyProbe,
    collision_world::{CurrentSlope, RuntimeForwardQuery, RuntimeForwardSupport, SlopeCheck},
    ground::{FallbackProbe, GroundProbe},
};
use cyrus_script::player::data::PlayerScene;

#[test]
fn installed_belltower_proposed_slope_miss_continues_to_support() -> Result<()> {
    let root = std::env::var_os("REDGUARD_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard"));
    let Ok(install) = cyrus_assets::install::GameInstall::locate(Some(&root)) else {
        eprintln!("skipped: no install (installed belltower edge)");
        return Ok(());
    };
    let files = cyrus_assets::native::Files::open(&install)?;
    let prepared = cyrus_assets::game::prepare(&files, "BELLTOWR.player")?;
    let scene = PlayerScene::decode(&prepared["BELLTOWR.player"])?;
    let position = [532_736, -116_736, 571_927];
    let rotation = cyrus_script::player::motion::upright_rotation(25);
    let proposed = [532_362, -116_736, 576_777];
    let candidates = cyrus_collision::collision_world::StaticCandidates {
        faces: vec![2, 0],
        actors: vec![],
    };
    let ground = GroundProbe::new(proposed, 62, 50)?;
    let terrain = |position| {
        scene
            .terrain
            .as_ref()
            .map(|terrain| {
                terrain
                    .sample(position)
                    .context("BELLTOWR edge terrain unavailable")
            })
            .transpose()
    };
    let current = CurrentSlope::new(position, proposed, 62, 50, || terrain(position))?
        .context("edge requires a displaced probe")?;
    let current_contacts =
        scene
            .static_collision
            .ground_contacts(&candidates, &current.probe, false)?;
    ensure!(
        current
            .probe
            .slope_surface(current.terrain, &current_contacts)
            .is_some(),
        "BELLTOWR edge lost its current support"
    );
    let contacts = scene
        .static_collision
        .ground_contacts(&candidates, &ground, false)?;
    ensure!(
        ground
            .slope_surface(terrain(proposed)?, &contacts)
            .is_none(),
        "BELLTOWR edge acquired a proposed slope"
    );
    let fallback = FallbackProbe::new(proposed, [0; 3], 62, false)?;
    ensure!(
        scene
            .static_collision
            .fallback_support(&candidates, &fallback, false)?
            .is_some(),
        "BELLTOWR fallback support absent"
    );
    let runtime = CollisionCandidates {
        faces: candidates
            .faces
            .iter()
            .map(|&index| FaceCandidate {
                model: ModelInstance::StaticPlacement(index),
                owner: CandidateOwner::Static(index),
                collision_flags: scene.collision.models[&scene.collision.placements[index].model]
                    .flags,
                candidate_type: 0x20,
            })
            .collect(),
        actors: vec![],
    };
    let outcome = scene.static_collision.runtime_forward_support(
        &runtime,
        scene
            .actor_collision
            .as_ref()
            .context("BELLTOWR actor collision missing")?,
        &Default::default(),
        RuntimeForwardQuery {
            body: BodyProbe {
                position: proposed,
                rotation,
                spheres: &[
                    (bevy::math::Vec3::new(0.0, -42.0, 0.0), 24.0),
                    (bevy::math::Vec3::new(0.0, -12.0, 0.0), 24.0),
                ],
                sidedness: false,
            },
            ground: &ground,
            terrain: terrain(proposed)?,
            fallback: &fallback,
            check_slope: Some(SlopeCheck(Some(&current))),
            excluded_candidate_type: None,
        },
    )?;
    ensure!(
        matches!(outcome, RuntimeForwardSupport::Queried(Some(_))),
        "BELLTOWR proposed slope miss must continue to support"
    );
    Ok(())
}
