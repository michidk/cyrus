//! Player-specific forward admission after world support queries.
use cyrus_world::collision::ground::{GroundSupport, SelectedSupport};
/// Stored support class including the original collision-attribute steep-ground override.
pub fn classification(support: &SelectedSupport, collision_attribute: u8) -> u8 {
    let class = match support {
        SelectedSupport::Primary { support, .. } => support.classification(),
        SelectedSupport::Fallback { normal, .. } => GroundSupport {
            height: 0.0,
            normal: *normal,
        }
        .classification(),
    };
    if class == 2 && collision_attribute & 2 == 0 {
        4
    } else {
        class
    }
}
/// The support updater's retained surface veto is not recomputed from material flags.
pub fn allows_forward(
    support: &SelectedSupport,
    collision_attribute: u8,
    surface_veto: i32,
) -> bool {
    classification(support, collision_attribute) != 2 && surface_veto != 1
}
