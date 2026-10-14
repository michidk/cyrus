//! PVO selection for collision-facing model admission, independent of rendering culling.
use anyhow::Result;
use cyrus_assets::collision::ModelVisibility;

/// Validated original-coordinate PVO data. Missing preparations must be rejected by
/// the asset host; an empty tree explicitly means unrestricted model admission.
pub struct ModelAdmission(ModelVisibility);

impl ModelAdmission {
    pub fn new(data: ModelVisibility) -> Result<Self> {
        data.validate()?;
        Ok(Self(data))
    }

    /// Select original object IDs using already-truncated integer camera coordinates.
    /// Outside-root and no-list nodes return an empty, unrestricted selection.
    /// Entry order and duplicates follow preservation's runtime PVO rules.
    pub fn select(&self, camera: [i32; 3]) -> &[u16] {
        if self.0.nodes.is_empty() {
            return &[];
        }
        let mut relative = std::array::from_fn::<_, 3, _>(|axis| {
            i64::from(camera[axis]) - i64::from(self.0.center[axis])
        });
        let mut extent = i64::from(self.0.half_extent);
        if relative.iter().any(|value| value.abs() > extent) {
            return &[];
        }
        let mut index = 0;
        loop {
            let octant = usize::from(relative[2] > 0)
                | (usize::from(relative[1] > 0) << 1)
                | (usize::from(relative[0] > 0) << 2);
            let node = &self.0.nodes[index];
            let Some(child) = node.children[octant] else {
                return &node.objects;
            };
            extent /= 2;
            for (axis, value) in relative.iter_mut().enumerate() {
                let bit = [4, 2, 1][axis];
                *value -= if octant & bit != 0 { extent } else { -extent };
            }
            index = child;
        }
    }
}
