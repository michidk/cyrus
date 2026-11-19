//! Installed harbour edge: a support miss rejects movement without stopping scripts.
use anyhow::{Context, Result, ensure};
use cyrus_collision::{
    candidates::{CandidateOwner, CollisionCandidates, FaceCandidate, ModelInstance},
    collision_mesh::BodyProbe,
    collision_world::{CurrentSlope, RuntimeForwardQuery, RuntimeForwardSupport, SlopeCheck},
    ground::{FallbackProbe, GroundProbe, SupportHistory},
};
use cyrus_script::player::{
    data::PlayerScene,
    grounded::{StaticGroundQuery, SurfaceUpdate},
};

#[test]
fn installed_harbour_edge_without_forward_support_is_blocked() -> Result<()> {
    let root = std::env::var_os("REDGUARD_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard"));
    let Ok(install) = cyrus_assets::install::GameInstall::locate(Some(&root)) else {
        eprintln!("skipped: no install (installed harbour edge)");
        return Ok(());
    };
    let files = cyrus_assets::native::Files::open(&install)?;
    let prepared = cyrus_assets::game::prepare(&files, "ISLAND.player")?;
    let scene = PlayerScene::decode(&prepared["ISLAND.player"])?;
    let definition = scene
        .definition
        .as_ref()
        .context("ISLAND player definition missing")?;
    // Settled endpoint of an unlocked, input-only walk after the original arrival.
    let position = [9_344_411, -67_584, 9_882_576];
    let rotation = cyrus_script::player::motion::upright_rotation(947);
    let proposed = definition.forward_probe(definition.positive_extent, position, rotation)?;
    let candidates = scene
        .static_collision
        .candidates([36526, 38708], position, None)?;
    let ground = GroundProbe::new(
        proposed,
        definition.positive_extent[1],
        scene.settings.post_collide_height,
    )?;
    let terrain = |position| {
        scene
            .terrain
            .as_ref()
            .map(|terrain| {
                terrain
                    .sample(position)
                    .context("ISLAND edge terrain unavailable")
            })
            .transpose()
    };
    let current = CurrentSlope::new(
        position,
        proposed,
        definition.positive_extent[1],
        scene.settings.post_collide_height,
        || terrain(position),
    )?
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
        "ISLAND edge lost its current support"
    );
    let contacts = scene
        .static_collision
        .ground_contacts(&candidates, &ground, false)?;
    ensure!(
        ground
            .slope_surface(terrain(proposed)?, &contacts)
            .is_none(),
        "ISLAND edge acquired a proposed slope"
    );
    let fallback = FallbackProbe::new(
        proposed,
        definition.center_offset,
        definition.positive_extent[1],
        false,
    )?;
    ensure!(
        scene
            .static_collision
            .forward_support(
                &candidates,
                &ground,
                terrain(proposed)?,
                Some(SlopeCheck(Some(&current))),
                Some(&fallback),
                false
            )?
            .is_none(),
        "ISLAND edge accepted unsupported movement"
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
    ensure!(
        scene.static_collision.runtime_forward_support(
            &runtime,
            scene
                .actor_collision
                .as_ref()
                .context("ISLAND actor collision missing")?,
            &Default::default(),
            RuntimeForwardQuery {
                body: BodyProbe {
                    position: proposed,
                    rotation,
                    spheres: &[],
                    sidedness: false
                },
                ground: &ground,
                terrain: terrain(proposed)?,
                fallback: &fallback,
                check_slope: Some(SlopeCheck(Some(&current))),
                excluded_candidate_type: None,
            }
        )? == RuntimeForwardSupport::Blocked,
        "ISLAND edge must reject before support-state commits"
    );
    let query = StaticGroundQuery {
        scene: &scene,
        candidates: &candidates,
        rotation,
        collision_attribute: 6,
        alternate_collision: false,
        sidedness: false,
        surface_update: SurfaceUpdate::Assign,
        bounds: None,
    };
    let mut history = SupportHistory::new(0, 3);
    let mut surface = cyrus_assets::surface::Surface::WOOD;
    let mut veto = 0;
    ensure!(
        query
            .forward_support(proposed, position, &mut history, &mut surface, &mut veto)?
            .is_none(),
        "static player accepted edge"
    );
    ensure!(
        history.ticks() == 0
            && history.metadata() == 3
            && surface == cyrus_assets::surface::Surface::WOOD
            && veto == 0,
        "blocked edge changed support state"
    );
    Ok(())
}
