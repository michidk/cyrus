//! Optional visual death physics. The guest owns anatomy, constraints and impact policy.
mod rig;
use cyrus_mod_sdk::{ActorId, ActorState, ClaimError, Cx, Mod, Persist, Vec3};

struct Ragdolls;
impl Mod for Ragdolls {
    const ID: &'static str = "ragdolls";
    fn frame(cx: &Cx) {
        let mut pending = cx.memory.load::<Pending>("pending").unwrap_or_default();
        for event in cx.actors.deaths() {
            if !pending.0.iter().any(|(id, _)| *id == event.actor) {
                pending.0.push((event.actor, event.source));
            }
        }
        pending.0.retain(|&(actor, source)| {
            let Some(snapshot) = cx.actors.snapshot(actor) else {
                return false;
            };
            if snapshot.state != ActorState::Dead {
                return false;
            }
            let (Some(model), Some(pose)) = (cx.physics.model(actor), cx.physics.origin(actor))
            else {
                return true;
            };
            let Some(profile) = rig::profile(&model) else {
                eprintln!("ragdolls: unsupported anatomy {model}");
                cx.hud
                    .show(&format!("Ragdolls: unsupported anatomy {model}"));
                return false;
            };
            let vertices = cx.physics.vertices(actor);
            let reference = cx.physics.reference(actor);
            if vertices.is_empty() || reference.len() != vertices.len() {
                return true;
            }
            let Some(assembly) = rig::fit(
                profile,
                &reference,
                &vertices,
                pose,
                source,
                cx.settings.number("impulse"),
                &cx.physics.attachments(actor),
            ) else {
                eprintln!("ragdolls: cannot fit anatomy {model}");
                cx.hud
                    .show(&format!("Ragdolls: cannot fit anatomy {model}"));
                return false;
            };
            match cx.physics.claim(actor, &assembly) {
                Ok(()) | Err(ClaimError::AlreadyClaimed) => false,
                Err(ClaimError::Unavailable | ClaimError::Budget) => true,
                Err(error) => {
                    cx.hud.show(&format!("Ragdolls: {model} {error:?}"));
                    false
                }
            }
        });
        // Events are bounded by the host. Keep retry state bounded while geometry loads.
        pending.0.truncate(128);
        cx.memory.save("pending", Some(&pending));
    }
}
cyrus_mod_sdk::export_mod!(Ragdolls);

#[derive(Default)]
struct Pending(Vec<(ActorId, Option<Vec3>)>);
impl Persist for Pending {
    fn pack(&self) -> Vec<f32> {
        self.0
            .iter()
            .flat_map(|(id, source)| {
                let mut values = id.pack();
                values.push(f32::from(source.is_some()));
                let p = source.unwrap_or(Vec3::ZERO);
                values.extend([p.x, p.y, p.z]);
                values
            })
            .collect()
    }
    fn unpack(values: &[f32]) -> Option<Self> {
        if !values.len().is_multiple_of(8) || values.len() > 128 * 8 {
            return None;
        }
        let mut pending = Vec::new();
        for row in values.chunks_exact(8) {
            pending.push((
                ActorId::unpack(&row[..4])?,
                (row[4] == 1.).then(|| Vec3::new(row[5], row[6], row[7])),
            ));
        }
        Some(Self(pending))
    }
}
