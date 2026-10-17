//! Retained logical camera records; pose correction and publication share this owner.
use anyhow::Result;
use cyrus_world::transform::Matrix;

/// One initialized camera record. Coordinates have eight fractional bits.
/// Initialization must supply installed values; missing settings have no default here.
#[derive(Clone, Debug)]
pub struct CameraRecord {
    pub actor: super::ScriptActorId,
    pub position_offset: [i32; 3],
    pub angle_offset: [i32; 3],
    pub request: [i32; 3],
    pub target_offset: [i32; 3],
    pub bound_position_history: [i32; 3],
    pub basis: [[i32; 3]; 2],
    /// Stored distance before the candidate stage shifts it by eight.
    pub distance: i32,
    /// Stored minimum, retaining the native stage-specific conversions.
    pub minimum_distance: i32,
    pub maximum_movement: u32,
    pub flags: u8,
}

impl CameraRecord {
    /// Compose the position supplied to collision, limiting and terrain correction.
    /// This does not publish a final camera pose.
    pub fn candidate(
        &mut self,
        bound_position: [i32; 3],
        bound_angles: [u16; 3],
    ) -> Result<[i32; 3]> {
        if self.distance.wrapping_shl(8) < self.minimum_distance {
            self.distance = self.minimum_distance;
        }
        if self.flags & 0x04 != 0 {
            return Ok(add(bound_position, self.request));
        }
        if self.flags & 0x08 != 0 {
            return Ok(self.request);
        }
        let offset = rotate(self.position_offset, bound_angles)?;
        let mut angles = std::array::from_fn(|axis| {
            i32::from(bound_angles[axis]).wrapping_sub(self.angle_offset[axis]) as u16 & 2047
        });
        if self.flags & 0x80 != 0 {
            angles[0] = self.angle_offset[0].wrapping_neg() as u16 & 2047;
        }
        let orbit = rotate([0, 0, self.distance.wrapping_shl(8).wrapping_neg()], angles)?;
        Ok(add(add(bound_position, orbit), offset))
    }
}

fn add(left: [i32; 3], right: [i32; 3]) -> [i32; 3] {
    std::array::from_fn(|axis| left[axis].wrapping_add(right[axis]))
}

fn rotate(position: [i32; 3], [pitch, yaw, roll]: [u16; 3]) -> Result<[i32; 3]> {
    Matrix::orientation([pitch.wrapping_neg(), yaw, roll.wrapping_neg()]).transform(position)
}

/// Apply the documented distance-height stage to slots zero and three.
/// The caller supplies the bound position from the same update as the candidate.
fn correct_distance_height(
    position: &mut [i32; 3],
    bound_position: [i32; 3],
    slot: u8,
    minimum: i32,
) -> Result<()> {
    if !matches!(slot, 0 | 3) {
        return Ok(());
    }
    let delta: [f64; 3] = std::array::from_fn(|axis| {
        f64::from((position[axis].wrapping_sub(bound_position[axis]) / 256) as f32)
    });
    let squared = (delta[1] * delta[1] + delta[0] * delta[0] + delta[2] * delta[2]) as f32;
    let distance = squared.sqrt();
    let distance = if distance > 0.0 { distance } else { 0.1 };
    let minimum = minimum as f32;
    if distance < minimum {
        let vertical = (delta[1] as f32 / distance) * minimum;
        let displacement =
            truncated_i32(f64::from(vertical).abs() * 256.0, "vertical displacement")?;
        position[1] = bound_position[1].wrapping_sub(displacement);
    }
    Ok(())
}

/// Clamp against a height from the retained terrain owner, returning the correction flag.
/// The owner must query at candidate X/Z arithmetic-shifted by eight.
fn correct_terrain_height(position: &mut [i32; 3], height: f32, clearance: f32) -> Result<bool> {
    let height = truncated_i32(f64::from(height), "terrain height")?.wrapping_shl(8);
    let clearance = truncated_i32(f64::from(clearance) * 1024.0, "body clearance")?;
    if position[1].wrapping_add(clearance) > height {
        position[1] = height.wrapping_sub(clearance);
        return Ok(true);
    }
    Ok(false)
}

fn truncated_i32(value: f64, field: &str) -> Result<i32> {
    let value = value.trunc();
    anyhow::ensure!(
        value.is_finite() && value >= f64::from(i32::MIN) && value <= f64::from(i32::MAX),
        "camera {field}: nonfinite or out-of-range value {value}"
    );
    Ok(value as i32)
}

/// Shared published pose, distinct from an uncorrected record request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CameraPose {
    pub position: [i32; 3],
    pub angles: [i32; 3],
}

/// Validated index into the nine retained camera records.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CameraSlot(u8);

impl CameraSlot {
    pub fn new(slot: u8) -> Result<Self> {
        anyhow::ensure!(slot < 9, "camera slot {slot}: outside the nine records");
        Ok(Self(slot))
    }
}

/// Installed `[camera]` values consumed by record initialization, unscaled as stored in
/// SYSTEM.INI. Preservation documents the mapping in its "Camera record installed
/// settings" section (<https://github.com/michidk/redguard-preservation/pull/100>).
#[derive(Clone, Debug, Default)]
pub struct CameraSettings {
    pub offset_pos: [i32; 3],
    pub offset_angle: [i32; 3],
    pub target_offset: [i32; 3],
    pub distance: i32,
    pub min_distance: i32,
    pub max_vel: i32,
    /// Slots three through eight: combat, hang, rope, rope above, rope below, debug.
    pub modes: [CameraModeSettings; 6],
}

/// Slot-specific overrides applied after record zero is copied.
#[derive(Clone, Copy, Debug, Default)]
pub struct CameraModeSettings {
    pub angle_offset: [i32; 3],
    pub distance: i32,
    /// Zero keeps the copied record-zero rate.
    pub max_vel: i32,
}

impl CameraRecord {
    fn cleared(actor: super::ScriptActorId) -> Self {
        Self {
            actor,
            position_offset: [0; 3],
            angle_offset: [0; 3],
            request: [0; 3],
            target_offset: [0; 3],
            bound_position_history: [0; 3],
            basis: [[0; 3]; 2],
            distance: 0,
            minimum_distance: 0,
            maximum_movement: 0,
            flags: 0,
        }
    }
}

impl CameraState {
    /// The nine records as camera setup leaves them. Runtime fields start cleared; the
    /// debug slot's second and third angle offsets keep the record-zero copy.
    /// Pose, selection and actor identities come from the session's bound actors.
    pub fn initial_records(
        settings: &CameraSettings,
        player: super::ScriptActorId,
    ) -> Result<[CameraRecord; 9]> {
        let mut zero = CameraRecord::cleared(player);
        zero.position_offset = settings.offset_pos;
        zero.angle_offset = settings.offset_angle;
        zero.distance = settings.distance;
        zero.minimum_distance = settings.min_distance;
        zero.maximum_movement = u32::try_from(settings.max_vel).map_err(|_| {
            anyhow::anyhow!("camera max_vel {}: negative unsupported", settings.max_vel)
        })?;
        zero.flags = 0x40;
        let mut records: [CameraRecord; 9] = std::array::from_fn(|_| zero.clone());
        records[0].target_offset = settings.target_offset;
        for (index, mode) in settings.modes.iter().enumerate() {
            let slot = index + 3;
            let record = &mut records[slot];
            if slot == 8 {
                record.angle_offset[0] = mode.angle_offset[2];
            } else {
                record.angle_offset = mode.angle_offset;
            }
            record.distance = mode.distance;
            if mode.max_vel != 0 {
                record.maximum_movement = u32::try_from(mode.max_vel).map_err(|_| {
                    anyhow::anyhow!(
                        "camera slot {slot} max_vel {}: negative unsupported",
                        mode.max_vel
                    )
                })?;
            }
            if matches!(slot, 6 | 7) {
                record.flags = 0x40;
            }
        }
        records[3].target_offset = settings.target_offset;
        Ok(records)
    }
}

/// Session-owned camera state. Construction requires established initial values.
#[derive(Clone, Debug)]
pub struct CameraState {
    pub records: [CameraRecord; 9],
    pub selected: CameraSlot,
    pub transition: u16,
    pub scripted_active: u16,
    pub current: CameraPose,
    pub previous: CameraPose,
    pub transformed_basis: [[i32; 3]; 2],
    pub camera_actor: super::ScriptActorId,
    pub alternate_actor: super::ScriptActorId,
}

impl CameraState {
    /// Request a slot change, retaining the active transition's selection guard.
    pub fn select(&mut self, slot: u8) -> Result<()> {
        let slot = CameraSlot::new(slot)?;
        if slot != self.selected
            && (self.transition == 0 || self.records[usize::from(self.selected.0)].flags & 1 != 0)
        {
            self.selected = slot;
        }
        Ok(())
    }

    /// One showPlayer invocation. The native task owns the wrapping private counter.
    pub fn show_player(
        &mut self,
        requested_slot: Option<u8>,
        counter: &mut u32,
        elapsed: i32,
    ) -> Result<bool> {
        self.select(requested_slot.unwrap_or(0))?;
        *counter = counter.wrapping_add(1);
        let flags = &mut self.records[usize::from(self.selected.0)].flags;
        if *counter == 1 && elapsed == 0 {
            *flags |= 1;
            Ok(true)
        } else {
            *flags &= !0x21;
            Ok(false)
        }
    }

    /// Whether scripted ownership or the current record prevents reselection.
    pub fn reselection_inhibited(&self) -> bool {
        self.scripted_active != 0 || self.records[usize::from(self.selected.0)].flags & 0x20 != 0
    }

    /// Publish a fully corrected pose and return the two logical actor position writes.
    /// The registry must validate both identities before committing the update.
    pub fn publish(
        &mut self,
        next: CameraPose,
        movement_limited: bool,
        collision_or_terrain_corrected: bool,
    ) -> [(super::ScriptActorId, [i32; 3]); 2] {
        self.previous = self.current;
        self.current = next;
        if collision_or_terrain_corrected || (self.transition == 1 && !movement_limited) {
            self.transition = 0;
        }
        [
            (self.camera_actor, next.position),
            (self.alternate_actor, next.position),
        ]
    }
}

/// Initialized shared limiter parameters. No missing installed value is defaulted.
pub struct MovementLimits {
    pub budget_increment: i32,
    pub vertical: i32,
    pub frame_scale: i32,
    pub selected_player_special_state: bool,
}

impl CameraState {
    /// Limit a candidate using the two published positions, without publishing it.
    pub fn limit_movement(
        &self,
        candidate: &mut [i32; 3],
        limits: &MovementLimits,
    ) -> Result<bool> {
        let record = &self.records[usize::from(self.selected.0)];
        if record.flags & 1 != 0 {
            return Ok(false);
        }
        let timing = |value: i32| value.wrapping_mul(limits.frame_scale.max(180)) >> 8;
        let origin = self.current.position;
        let mut next = *candidate;
        let mut delta: [i32; 3] = std::array::from_fn(|axis| next[axis].wrapping_sub(origin[axis]));
        let mut limited = false;
        if self.selected.0 != 1 && !limits.selected_player_special_state {
            let lower = timing(limits.vertical.wrapping_shl(8)) >> 8;
            if delta[1] < lower {
                delta[1] = lower;
                limited = true;
            }
            if delta[1] > lower.wrapping_neg() {
                delta[1] = limits.vertical.wrapping_neg();
                limited = true;
            }
            next[1] = origin[1].wrapping_add(delta[1]);
        }
        let distance = super::rounded_distance(delta)? as u32;
        let history = super::rounded_distance(std::array::from_fn(|axis| {
            origin[axis]
                .wrapping_sub(self.previous.position[axis])
                .wrapping_abs()
        }))?;
        let budget =
            (history.wrapping_add(limits.budget_increment) as u32).min(record.maximum_movement);
        let budget = (timing(budget.wrapping_shl(8) as i32) as u32) >> 8;
        if distance > budget {
            let fraction = budget.wrapping_shl(8) / distance;
            for axis in 0..3 {
                if axis == 1 || !limits.selected_player_special_state {
                    next[axis] =
                        origin[axis].wrapping_add(delta[axis].wrapping_mul(fraction as i32) >> 8);
                }
            }
            limited = true;
        }
        *candidate = next;
        Ok(limited)
    }
}

/// Apply ordered contact normals from the camera body's collision query.
/// Normals use eight fractional bits; radius is in unscaled world units.
fn respond_to_contacts(candidate: &mut [i32; 3], normals: &[[i32; 3]], radius: f32) -> Result<()> {
    let Some(first) = normals.first() else {
        return Ok(());
    };
    let radius = truncated_i32(f64::from(radius), "contact radius")?;
    let mut direction = [0.0_f32; 3];
    for normal in normals {
        for axis in 0..3 {
            direction[axis] = (f64::from(direction[axis]) + f64::from(normal[axis]) / 256.0) as f32;
        }
    }
    if direction == [0.0; 3] {
        direction = first.map(|value| (f64::from(value) / 256.0) as f32);
    } else {
        let squared: f64 = direction
            .into_iter()
            .map(|value| f64::from(value).powi(2))
            .sum();
        let reciprocal = (1.0 / squared.sqrt()) as f32;
        direction = direction.map(|value| value * reciprocal);
    }
    let mut next = *candidate;
    for axis in 0..3 {
        let displacement = (f64::from(radius) * f64::from(direction[axis])) as f32;
        let displacement = truncated_i32(f64::from(displacement) * 256.0, "contact displacement")?;
        next[axis] = next[axis].wrapping_add(displacement);
    }
    *candidate = next;
    Ok(())
}

/// Quantize a direction before the camera angle lookup, retaining native single stores.
fn normalize_direction(direction: [i32; 3]) -> [i32; 3] {
    let divisor = cyrus_collision::distance::single_precision_length(direction) / 65536.0;
    if divisor == 0.0 {
        return [0; 3];
    }
    direction.map(|component| ((component as f32) / divisor).round_ties_even() as i32)
}

/// Convert camera-minus-target direction to pitch/yaw/roll in 2048 units per turn.
fn direction_angles(direction: [i32; 3]) -> Result<[i32; 3]> {
    let [x, y, z] = normalize_direction(direction);
    let horizontal = super::rounded_distance([x, 0, z])?;
    let bearing = |x, z| -i32::from(super::spatial::query_bearing(x, z));
    Ok([
        (bearing(horizontal, y) - 512) & 2047,
        (bearing(x, z) - 1024) & 2047,
        0,
    ])
}

impl CameraState {
    /// Orient a corrected position, retaining the fixed-orientation branch's history.
    pub fn orient(
        &mut self,
        position: [i32; 3],
        bound_position: [i32; 3],
        bound_angles: [u16; 3],
    ) -> Result<CameraPose> {
        let record = &mut self.records[usize::from(self.selected.0)];
        if record.flags & 2 != 0 {
            return Ok(CameraPose {
                position,
                angles: self.current.angles,
            });
        }
        let offset = if self.selected.0 < 2 {
            [0, record.target_offset[1], 0]
        } else {
            record.target_offset
        };
        let target = add(bound_position, rotate(offset, bound_angles)?);
        let angles = direction_angles(std::array::from_fn(|axis| {
            position[axis].wrapping_sub(target[axis])
        }))?;
        let matrix =
            Matrix::collision_orientation([angles[0].wrapping_neg() as u16, angles[1] as u16, 0]);
        let basis = [
            matrix.inverse_transform(record.basis[0])?,
            matrix.inverse_transform(record.basis[1])?,
        ];
        record.bound_position_history = bound_position;
        self.transformed_basis = basis;
        Ok(CameraPose { position, angles })
    }
}

/// Retained outputs of the camera's ordered segment query.
#[derive(Clone, Debug)]
pub struct CameraHitSelection {
    pub minimum_position: [i32; 3],
    pub maximum_position: [i32; 3],
    pub minimum_normal: [i32; 3],
    pub maximum_normal: [i32; 3],
}

impl CameraHitSelection {
    fn select(
        &mut self,
        contacts: &[cyrus_collision::collision_mesh::IntegerSegmentContact],
    ) -> bool {
        let mut minimum = 32767;
        let mut maximum = 0;
        for contact in contacts {
            if contact.rank < minimum {
                minimum = contact.rank;
                self.minimum_position = contact.point.map(|value| value.wrapping_shl(8));
                self.minimum_normal = contact.normal;
            }
            if contact.rank > maximum {
                maximum = contact.rank;
                self.maximum_position = contact.point.map(|value| value.wrapping_shl(8));
                self.maximum_normal = contact.normal;
            }
        }
        !contacts.is_empty()
    }
}

/// Geometry owners supply the retained model/terrain state for the same update.
/// Segment endpoints and contact positions use Q8; returned segment points use map units.
pub trait CameraGeometry {
    fn segment(
        &mut self,
        start: [i32; 3],
        end: [i32; 3],
        excluded: super::ScriptActorId,
    ) -> Result<Vec<cyrus_collision::collision_mesh::IntegerSegmentContact>>;
    fn contacts(
        &mut self,
        position: [i32; 3],
        excluded: super::ScriptActorId,
    ) -> Result<Vec<[i32; 3]>>;
    fn terrain_height(&mut self, horizontal: [i32; 2]) -> Result<f32>;
}

/// Settled actor pose and explicitly initialized correction settings for one update.
pub struct CameraUpdateInputs {
    pub bound_position: [i32; 3],
    pub bound_angles: [u16; 3],
    pub movement: MovementLimits,
    pub body_radius: f32,
    pub body_clearance: f32,
    pub collision_inhibited: bool,
    pub terrain_enabled: bool,
}

/// Off-live replacement; publish state, hit history and actor positions together.
pub struct CameraUpdate {
    pub state: CameraState,
    pub hits: CameraHitSelection,
    pub actor_positions: [(super::ScriptActorId, [i32; 3]); 2],
}

impl CameraState {
    /// Prepare a complete ordered pose off-live. The registry validates actor bindings
    /// before publishing the returned owner and both logical actor positions together.
    pub fn prepare_pose(
        &self,
        hits: &CameraHitSelection,
        inputs: &CameraUpdateInputs,
        geometry: &mut impl CameraGeometry,
    ) -> Result<CameraUpdate> {
        let mut state = self.clone();
        let mut hits = hits.clone();
        let slot = state.selected.0;
        let record = &mut state.records[usize::from(slot)];
        let mut position = record.candidate(inputs.bound_position, inputs.bound_angles)?;
        let segment_enabled = record.flags & 0x40 != 0 && !inputs.collision_inhibited;
        let target = if segment_enabled {
            Some(add(
                inputs.bound_position,
                rotate(record.target_offset, inputs.bound_angles)?,
            ))
        } else {
            None
        };
        let segment = |position: &mut [i32; 3],
                       hits: &mut CameraHitSelection,
                       geometry: &mut dyn CameraGeometry|
         -> Result<bool> {
            let Some(target) = target else {
                return Ok(false);
            };
            let contacts = geometry.segment(target, *position, record.actor)?;
            if hits.select(&contacts) {
                *position = hits.minimum_position;
                return Ok(true);
            }
            Ok(false)
        };
        let height_and_contacts =
            |position: &mut [i32; 3], geometry: &mut dyn CameraGeometry| -> Result<()> {
                correct_distance_height(
                    position,
                    inputs.bound_position,
                    slot,
                    record.minimum_distance,
                )?;
                respond_to_contacts(
                    position,
                    &geometry.contacts(*position, record.actor)?,
                    inputs.body_radius,
                )
            };
        let terrain =
            |position: &mut [i32; 3], geometry: &mut dyn CameraGeometry| -> Result<bool> {
                if !inputs.terrain_enabled {
                    return Ok(false);
                }
                let height = geometry.terrain_height([position[0] >> 8, position[2] >> 8])?;
                correct_terrain_height(position, height, inputs.body_clearance)
            };
        let mut corrected = segment(&mut position, &mut hits, geometry)?;
        if corrected {
            height_and_contacts(&mut position, geometry)?;
            terrain(&mut position, geometry)?;
        }
        corrected |= segment(&mut position, &mut hits, geometry)?;
        height_and_contacts(&mut position, geometry)?;
        let limited = state.limit_movement(&mut position, &inputs.movement)?;
        corrected |= terrain(&mut position, geometry)?;
        let pose = state.orient(position, inputs.bound_position, inputs.bound_angles)?;
        let actor_positions = state.publish(pose, limited, corrected);
        Ok(CameraUpdate {
            state,
            hits,
            actor_positions,
        })
    }
}

/// Segment collector origin and added model radius in map units.
/// The admitted range excludes the original signed squared-length overflow path.
fn segment_bounds(start: [i32; 3], end: [i32; 3]) -> Result<([i32; 3], u32)> {
    let delta = std::array::from_fn::<_, 3, _>(|axis| end[axis].wrapping_sub(start[axis]) >> 8);
    let squared: i64 = delta.into_iter().map(|value| i64::from(value).pow(2)).sum();
    anyhow::ensure!(
        squared <= i64::from(i32::MAX),
        "camera segment squared length {squared}: signed overflow path unsupported"
    );
    Ok((start.map(|value| value >> 8), 2 * (squared as u32).isqrt()))
}

/// Test a segment collector's model bound with stored single-precision intermediates.
/// Model identity exclusion and ordered list admission remain caller-owned.
fn segment_model_intersects(
    origin: [i32; 3],
    added_radius: u32,
    model: Option<([f32; 3], f32)>,
) -> Result<bool> {
    let Some((center, radius)) = model else {
        return Ok(false);
    };
    anyhow::ensure!(
        center.iter().all(|value| value.is_finite()) && radius.is_finite() && radius >= 0.0,
        "camera segment model bound is nonfinite or has negative radius"
    );
    let difference = std::array::from_fn::<_, 3, _>(|axis| {
        (f64::from(origin[axis]) - f64::from(center[axis])) as f32
    });
    let squared = difference
        .into_iter()
        .map(|value| f64::from(value).powi(2))
        .sum::<f64>() as f32;
    let combined = (f64::from(added_radius) + f64::from(radius)) as f32;
    Ok(f64::from(squared) <= f64::from(combined).powi(2))
}

/// Filter the caller's already eligible, ordered camera model instances.
/// This does not infer actor eligibility from ordinary-collision candidates.
/// Candidate saturation is explicitly unsupported until its lifecycle is admitted.
pub fn collect_segment_models(
    start: [i32; 3],
    end: [i32; 3],
    obstacle_size: i32,
    excluded: Option<cyrus_collision::candidates::ModelInstance>,
    models: &[cyrus_collision::candidates::ModelBounds],
) -> Result<Vec<cyrus_collision::candidates::ModelInstance>> {
    let (origin, radius) = segment_bounds(start, end)?;
    let mut result = Vec::new();
    for model in models {
        if Some(model.instance) == excluded {
            continue;
        }
        anyhow::ensure!(
            model.radius.is_finite() && model.radius >= 0.0,
            "camera model {:?}: invalid radius",
            model.instance
        );
        if f64::from(model.radius) <= f64::from(obstacle_size >> 8) {
            continue;
        }
        if segment_model_intersects(origin, radius, Some((model.position, model.radius)))? {
            anyhow::ensure!(
                result.len() < 64,
                "camera segment candidate saturation unsupported"
            );
            result.push(model.instance);
        }
    }
    Ok(result)
}

/// Terrain vertices selected for the camera's query, in engine cell coordinates.
/// The retained window center is independent of render frustum admission.
#[derive(Debug, PartialEq, Eq)]
struct CameraTerrainSelection {
    pub retained_grid: bool,
    pub corners: [[i32; 2]; 3],
}

fn terrain_selection(horizontal: [i32; 2], center: [i32; 2]) -> CameraTerrainSelection {
    let [x, z] = horizontal.map(|value| value as f32);
    let i = (x / 256.0 - 0.5).round_ties_even() as i32;
    let j = (z / 256.0 + 0.5).round_ties_even() as i32;
    let retained_grid = [i.wrapping_sub(center[0]), j.wrapping_sub(center[1])]
        .into_iter()
        .all(|offset| offset > -16 && offset < 16);
    let diagonal = [i + 1, j - 1];
    let corners = if f64::from(x) - f64::from(i) * 256.0 > f64::from(j) * 256.0 - f64::from(z) {
        [[i, j], [i + 1, j], diagonal]
    } else {
        [[i, j], diagonal, [i, j - 1]]
    };
    CameraTerrainSelection {
        retained_grid,
        corners,
    }
}

/// Sample the selected triangle from caller-supplied retained or map heights.
/// Heights follow `selection.corners`; the caller owns source identity and window lifetime.
fn terrain_triangle(
    horizontal: [i32; 2],
    selection: &CameraTerrainSelection,
    heights: [f32; 3],
) -> Result<(f32, [i32; 3])> {
    anyhow::ensure!(
        heights.iter().all(|height| height.is_finite()),
        "camera terrain triangle has nonfinite height"
    );
    let vertices: [[f32; 3]; 3] = std::array::from_fn(|index| {
        [
            (f64::from(selection.corners[index][0]) * 256.0) as f32,
            heights[index],
            (f64::from(selection.corners[index][1]) * 256.0) as f32,
        ]
    });
    let u: [f32; 3] = std::array::from_fn(|axis| vertices[2][axis] - vertices[0][axis]);
    let v: [f32; 3] = std::array::from_fn(|axis| vertices[1][axis] - vertices[2][axis]);
    let cross =
        |a, b| (f64::from(u[a]) * f64::from(v[b]) - f64::from(u[b]) * f64::from(v[a])) as f32;
    let plane = [cross(1, 2), cross(2, 0), cross(0, 1)];
    let length = plane
        .into_iter()
        .map(|value| f64::from(value).powi(2))
        .sum::<f64>()
        .sqrt();
    anyhow::ensure!(
        length.is_finite() && length > 0.0 && plane[1] != 0.0,
        "camera terrain triangle is degenerate"
    );
    let normal = plane.map(|value| f64::from(value) / length);
    let stored = normal.map(|value| value as f32);
    let [x, z] = horizontal.map(|value| f64::from(value as f32));
    let height = f64::from(vertices[0][1])
        - ((x - f64::from(vertices[0][0])) * f64::from(stored[0])
            + (z - f64::from(vertices[0][2])) * f64::from(stored[2]))
            / normal[1];
    anyhow::ensure!(
        height.is_finite() && (height as f32).is_finite(),
        "camera terrain triangle height overflow"
    );
    Ok((
        height as f32,
        stored.map(|value| (value * 256.0).round_ties_even() as i32),
    ))
}

/// Read the three admitted engine-coordinate heights without advancing terrain state.
/// The host must bind both sources and the retained center to the same world/epoch.
pub trait CameraTerrainHeights {
    fn retained_center(&self) -> [i32; 2];
    fn retained_height(&self, cell: [i32; 2]) -> Result<f32>;
    fn map_height(&self, cell: [i32; 2]) -> Result<f32>;
}

/// Sample the selected retained or map triangle without changing either source.
pub fn sample_terrain(
    horizontal: [i32; 2],
    source: &impl CameraTerrainHeights,
) -> Result<(f32, [i32; 3])> {
    let selection = terrain_selection(horizontal, source.retained_center());
    let mut heights = [0.0; 3];
    for (height, cell) in heights.iter_mut().zip(selection.corners) {
        *height = if selection.retained_grid {
            source.retained_height(cell)?
        } else {
            source.map_height(cell)?
        };
    }
    terrain_triangle(horizontal, &selection, heights)
}
