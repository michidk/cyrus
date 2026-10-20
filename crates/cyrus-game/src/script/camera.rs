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

/// Marks the camera natives and setup leave for the render step (`CameraState::present`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CameraMarks {
    /// A cut: the render camera takes the published pose instead of gliding to it.
    pub snap: bool,
    /// Overrides every other glide condition and forces a cut.
    pub hold: bool,
    /// Forces a glide, and skips the pose update's obstacle segment check for this frame.
    pub forced_glide: bool,
}

/// The render camera's retained pose: position in whole map units as floats, angles in
/// 2048 units per turn. Glide and cuts move it; the published pose stays the logical one.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RenderPose {
    pub position: [f32; 3],
    pub angles: [i32; 3],
}

/// The installed `[camera]` glide factors: `glide_x/y/z` for the position axes and
/// `glide_angle_x/y/z` for the angles.
#[derive(Clone, Copy, Debug, Default)]
pub struct GlideSettings {
    pub position: [f32; 3],
    pub angles: [f32; 3],
}

impl From<cyrus_assets::script::CameraGlide> for GlideSettings {
    fn from(glide: cyrus_assets::script::CameraGlide) -> Self {
        Self {
            position: glide.position,
            angles: glide.angles,
        }
    }
}

/// What the render step reads from the selected player to decide between gliding and cutting.
#[derive(Clone, Copy, Debug)]
pub struct GlidePlayer {
    pub holding_weapon: bool,
    pub rope_attached: bool,
}

impl RenderPose {
    /// The render pose of a cut to `pose`: its position shifted right eight bits, truncating
    /// toward zero.
    pub fn cut_to(pose: CameraPose) -> Self {
        Self {
            position: pose.position.map(|axis| (axis / 256) as f32),
            angles: pose.angles,
        }
    }
}

/// Validated index into the nine retained camera records.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CameraSlot(u8);

impl CameraSlot {
    /// Zero-based record index.
    pub fn index(self) -> usize {
        usize::from(self.0)
    }

    pub fn new(slot: u8) -> Result<Self> {
        anyhow::ensure!(slot < 9, "camera slot {slot}: outside the nine records");
        Ok(Self(slot))
    }
}

pub use cyrus_assets::script::{CameraLimits, CameraModeSettings, CameraSettings};

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

    /// Write the request an actor's `attr_camera_move` selects: zero an absolute position
    /// request (flag `0x08`), one a relative request (flag `0x04`), two an orbit from `orbit`'s
    /// length and angles plus the bound angles, which also sets flag `0x01`; any other value
    /// writes nothing.
    fn request_move(
        &mut self,
        camera_move: u8,
        absolute: [i32; 3],
        relative: [i32; 3],
        orbit: [i32; 3],
        bound_angles: [u16; 3],
    ) -> Result<()> {
        match camera_move {
            0 => {
                self.request = absolute;
                self.flags = (self.flags | 0x08) & !0x04;
            }
            1 => {
                self.request = relative;
                self.flags = (self.flags | 0x04) & !0x08;
            }
            2 => {
                self.distance = super::rounded_distance(orbit)? >> 8;
                let angles = direction_angles(orbit)?;
                self.angle_offset = std::array::from_fn(|axis| {
                    angles[axis].wrapping_add(i32::from(bound_angles[axis])) & 2047
                });
                self.flags = (self.flags & !0x0c) | 0x01;
            }
            _ => {}
        }
        Ok(())
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
            match slot {
                // The rope record adds the angle-override bit to the copied collision bit.
                5 => record.flags |= 0xc0,
                6 | 7 => record.flags = 0x40,
                _ => {}
            }
        }
        records[3].target_offset = settings.target_offset;
        Ok(records)
    }

    /// Session state as camera setup leaves it, before the installed initial pose is
    /// published: slot zero selected, no transition, no scripted ownership. Both pose
    /// vectors are zero placeholders until that stage is connected to live geometry.
    /// `camera` is the CAMERA instance; setup stores it as both published-position owners.
    /// `player_type` is the selected player's `attr_player_type`.
    pub fn new(
        settings: &CameraSettings,
        player: super::ScriptActorId,
        camera: super::ScriptActorId,
        player_type: u8,
    ) -> Result<Self> {
        let pose = CameraPose {
            position: [0; 3],
            angles: [0; 3],
        };
        let mut state = Self {
            records: Self::initial_records(settings, player)?,
            selected: CameraSlot(0),
            index: CameraSlot(0),
            transition: 0,
            scripted_active: 0,
            scripted_nesting: 0,
            reselect_pending: false,
            current: pose,
            previous: pose,
            transformed_basis: [[0; 3]; 2],
            camera_actor: camera,
            alternate_actor: camera,
            marks: CameraMarks {
                snap: true,
                ..CameraMarks::default()
            },
            render: RenderPose::cut_to(pose),
            shake: 0,
        };
        state.reload_player_defaults(settings, player, player_type)?;
        Ok(state)
    }

    /// Raise the screen shake by one impulse (32), capped at 32.
    pub fn add_shake(&mut self) {
        self.shake = 32;
    }

    /// The angles the frame draws with, from the published angles. The original's roll tilt is
    /// selected by a mode word nothing ever writes (zero: no tilt), so none is applied. A
    /// shaking camera moves the first angle down by the shake level on even frames, halving the
    /// level, and up by it on odd frames. `frame` is the main loop's frame counter.
    pub fn view_angles(&mut self, frame: u32) -> [u16; 3] {
        let mut angles = self.current.angles;
        if self.shake != 0 {
            let shake = i32::from(self.shake);
            if frame & 1 == 0 {
                angles[0] -= shake;
                self.shake >>= 1;
            } else {
                angles[0] += shake;
            }
        }
        angles.map(|angle| (angle & 0x7ff) as u16)
    }

    /// Cut the render camera to the published pose, as setup does once the first pose exists.
    pub fn reset_render(&mut self) {
        self.render = RenderPose::cut_to(self.current);
        self.marks = CameraMarks {
            snap: true,
            ..CameraMarks::default()
        };
    }

    /// Whether the render camera glides to the published pose this frame instead of cutting:
    /// a hold marks a cut and a forced glide a glide, then a held weapon or a rope association
    /// glides, an active scripted camera cuts, a pending snap cuts, a record that is marked
    /// scripted cuts, and otherwise only the three player slots (zero, three and four) glide.
    fn glides(&self, player: &GlidePlayer) -> bool {
        if self.marks.hold {
            return false;
        }
        if self.marks.forced_glide || player.holding_weapon || player.rope_attached {
            return true;
        }
        if self.scripted_active != 0 {
            return false;
        }
        if !self.marks.snap {
            return true;
        }
        self.records[self.selected.index()].flags & 0x20 == 0 && matches!(self.index.0, 0 | 3 | 4)
    }

    /// The render step, once per frame after the pose update: glide the render camera towards
    /// the published pose or cut to it, as the marks decide. A glide moves each position axis
    /// by its factor of the way (single-precision, then the terrain stage on the whole-unit
    /// result) and rewrites the published angles the same way; a cut copies the published
    /// position. The factors for the two angles are `angles[1]` for pitch and `angles[0]` for
    /// yaw, as the original reads them. Clears the forced-glide mark, retains the angles as the
    /// next frame's start and consumes `in_frame` (the main loop's frame mark).
    /// `terrain` corrects a position in Q8 units in place. Edits commit only on success.
    pub fn present(
        &mut self,
        settings: &GlideSettings,
        player: &GlidePlayer,
        in_frame: bool,
        terrain: &mut dyn FnMut(&mut [i32; 3]) -> Result<()>,
    ) -> Result<()> {
        let mut next = self.clone();
        let clamp = |value: f32| {
            let value = value.max(0.0);
            if value.to_bits() as i32 > 0x477f_ff00 {
                65535.0
            } else {
                value
            }
        };
        next.render.position[0] = clamp(next.render.position[0]);
        next.render.position[2] = clamp(next.render.position[2]);
        if next.glides(player) && !next.marks.snap && in_frame {
            // x87 evaluates each expression in extended precision and stores one single.
            let mut whole = [0i32; 3];
            for (axis, whole) in whole.iter_mut().enumerate() {
                let published = (f64::from(next.current.position[axis]) / 256.0) as f32;
                let previous = f64::from(next.render.position[axis]);
                let moved = ((f64::from(published) - previous) * f64::from(settings.position[axis])
                    + previous) as f32;
                *whole = (f64::from(moved) * 256.0).trunc() as i32;
            }
            terrain(&mut whole)?;
            next.render.position = whole.map(|axis| (axis / 256) as f32);
            for (axis, factor) in [(0, 1), (1, 0), (2, 2)] {
                let previous = next.render.angles[axis];
                let mut gap = (previous & 0x7ff) - (next.current.angles[axis] & 0x7ff);
                if gap > 0x400 {
                    gap -= 0x800;
                } else if gap < -0x400 {
                    gap += 0x800;
                }
                let step = (-f64::from(gap as f32) * f64::from(settings.angles[factor])) as f32;
                next.current.angles[axis] = (f64::from(step).trunc() as i32 + previous) & 0x7ff;
            }
        } else {
            next.render.position = next.current.position.map(|axis| (axis / 256) as f32);
            next.marks.snap = false;
            next.marks.hold = false;
        }
        next.marks.forced_glide = false;
        next.render.angles = next.current.angles;
        *self = next;
        Ok(())
    }

    /// Reload record zero from the installed values for the selected player and select
    /// it, as setup's final step and the defaults reload do. Player type one halves the
    /// distance, minimum distance and horizontal offsets (arithmetic shift) and fixes both
    /// vertical offsets at 6400, per preservation's player-type settings. Other records,
    /// the transition and the published poses are untouched; the caller recomputes the
    /// first pose.
    pub fn reload_player_defaults(
        &mut self,
        settings: &CameraSettings,
        player: super::ScriptActorId,
        player_type: u8,
    ) -> Result<()> {
        let mut record = Self::initial_records(settings, player)?[0].clone();
        if player_type == 1 {
            let reduce = |[x, _, z]: [i32; 3]| [x >> 1, 6400, z >> 1];
            record.distance >>= 1;
            record.minimum_distance >>= 1;
            record.position_offset = reduce(record.position_offset);
            record.target_offset = reduce(record.target_offset);
        }
        let live = &mut self.records[0];
        live.actor = player;
        live.position_offset = record.position_offset;
        live.angle_offset = record.angle_offset;
        live.target_offset = record.target_offset;
        live.distance = record.distance;
        live.minimum_distance = record.minimum_distance;
        live.maximum_movement = record.maximum_movement;
        live.flags = 0x40;
        self.selected = CameraSlot(0);
        self.index = CameraSlot(0);
        Ok(())
    }
}

/// Slot requested for the selected player: a held weapon, then the shared debug override,
/// then the hanging living state, then a rope association; `None` when none applies.
/// The override word has no writer in the executable image and starts zero, so the
/// session passes `false`.
pub fn player_slot(
    holding_weapon: bool,
    debug_override: bool,
    living_state: u8,
    rope_attached: bool,
) -> Option<u8> {
    if holding_weapon {
        Some(3)
    } else if debug_override {
        Some(8)
    } else if living_state == 15 {
        Some(4)
    } else if rope_attached {
        Some(5)
    } else {
        None
    }
}

/// Persistable camera session; actor identities are saved as generation/index pairs.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub(super) struct CameraSnapshot {
    records: Vec<RecordSnapshot>,
    selected: u8,
    index: u8,
    transition: u16,
    scripted_active: u16,
    scripted_nesting: u32,
    reselect_pending: bool,
    current: ([i32; 3], [i32; 3]),
    previous: ([i32; 3], [i32; 3]),
    transformed_basis: [[i32; 3]; 2],
    camera_actor: (u64, usize),
    alternate_actor: (u64, usize),
    /// Absent in snapshots that predate the render camera: restore as a cut to the published pose.
    #[serde(default)]
    marks: Option<CameraMarks>,
    #[serde(default)]
    render: Option<RenderPose>,
    #[serde(default)]
    shake: u8,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct RecordSnapshot {
    actor: (u64, usize),
    position_offset: [i32; 3],
    angle_offset: [i32; 3],
    request: [i32; 3],
    target_offset: [i32; 3],
    bound_position_history: [i32; 3],
    basis: [[i32; 3]; 2],
    distance: i32,
    minimum_distance: i32,
    maximum_movement: u32,
    flags: u8,
}

impl CameraState {
    pub(super) fn snapshot(&self) -> CameraSnapshot {
        let id = |actor: super::ScriptActorId| (actor.generation, actor.index);
        let pose = |pose: CameraPose| (pose.position, pose.angles);
        CameraSnapshot {
            records: self
                .records
                .iter()
                .map(|record| RecordSnapshot {
                    actor: id(record.actor),
                    position_offset: record.position_offset,
                    angle_offset: record.angle_offset,
                    request: record.request,
                    target_offset: record.target_offset,
                    bound_position_history: record.bound_position_history,
                    basis: record.basis,
                    distance: record.distance,
                    minimum_distance: record.minimum_distance,
                    maximum_movement: record.maximum_movement,
                    flags: record.flags,
                })
                .collect(),
            selected: self.selected.0,
            index: self.index.0,
            transition: self.transition,
            scripted_active: self.scripted_active,
            scripted_nesting: self.scripted_nesting,
            reselect_pending: self.reselect_pending,
            current: pose(self.current),
            previous: pose(self.previous),
            transformed_basis: self.transformed_basis,
            camera_actor: id(self.camera_actor),
            alternate_actor: id(self.alternate_actor),
            marks: Some(self.marks),
            render: Some(self.render),
            shake: self.shake,
        }
    }

    pub(super) fn from_snapshot(
        saved: CameraSnapshot,
        rebind: &impl Fn(u64, usize) -> Result<super::ScriptActorId>,
    ) -> Result<Self> {
        let records: [CameraRecord; 9] = saved
            .records
            .into_iter()
            .map(|record| {
                Ok(CameraRecord {
                    actor: rebind(record.actor.0, record.actor.1)?,
                    position_offset: record.position_offset,
                    angle_offset: record.angle_offset,
                    request: record.request,
                    target_offset: record.target_offset,
                    bound_position_history: record.bound_position_history,
                    basis: record.basis,
                    distance: record.distance,
                    minimum_distance: record.minimum_distance,
                    maximum_movement: record.maximum_movement,
                    flags: record.flags,
                })
            })
            .collect::<Result<Vec<_>>>()?
            .try_into()
            .map_err(|records: Vec<_>| {
                anyhow::anyhow!("camera snapshot: {} records, expected nine", records.len())
            })?;
        let pose = |(position, angles)| CameraPose { position, angles };
        Ok(Self {
            records,
            selected: CameraSlot::new(saved.selected)?,
            index: CameraSlot::new(saved.index)?,
            transition: saved.transition,
            scripted_active: saved.scripted_active,
            scripted_nesting: saved.scripted_nesting,
            reselect_pending: saved.reselect_pending,
            current: pose(saved.current),
            previous: pose(saved.previous),
            transformed_basis: saved.transformed_basis,
            camera_actor: rebind(saved.camera_actor.0, saved.camera_actor.1)?,
            alternate_actor: rebind(saved.alternate_actor.0, saved.alternate_actor.1)?,
            marks: saved.marks.unwrap_or(CameraMarks {
                snap: true,
                ..CameraMarks::default()
            }),
            render: saved
                .render
                .unwrap_or(RenderPose::cut_to(pose(saved.current))),
            shake: saved.shake,
        })
    }
}

/// Inputs of one `showPlayerPan` invocation.
#[derive(Clone, Copy, Debug)]
pub struct PanInputs {
    pub duration: i32,
    /// The slot the player camera selector requests; none selects slot zero.
    pub requested_slot: Option<u8>,
    pub bound_position: [i32; 3],
    pub bound_angles: [u16; 3],
}

/// The axes a location task's vector argument is given in: `showObj` and `showObjPan` use
/// world axes, `showObjLoc` and `showObjPanLoc` rotate it by the bound actor's angles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VectorFrame {
    World,
    Actor,
}

/// Inputs of one location-camera invocation: the bound actor, its camera attributes
/// (`attr_camera_move`, then the six target/offset bytes), its pose and the task arguments.
#[derive(Clone, Copy, Debug)]
pub struct LocationInputs {
    pub frame: VectorFrame,
    pub actor: super::ScriptActorId,
    pub camera_move: u8,
    pub attributes: [u8; 6],
    pub arguments: [i32; 3],
    pub bound_position: [i32; 3],
    pub bound_angles: [u16; 3],
}

/// Start values a pan keeps between invocations (the original keeps them in shared scratch).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PanState {
    /// Angles of the camera's direction from the player at the start.
    start: [i32; 3],
    /// Wrapped difference from those angles to the slot's own offset.
    delta: [i32; 3],
    start_distance: i32,
    final_distance: i32,
    /// The slot's angle offset minus the bound angles, as the pan's last step writes it.
    final_offset: [i32; 3],
}

/// Inputs of one object-pan invocation. `location.arguments` are the task's vector; the
/// world-frame pan is `showObjPan` and the actor-frame pan is `showObjPanLoc`.
#[derive(Clone, Copy, Debug)]
pub struct ObjectPanInputs {
    pub location: LocationInputs,
    /// The actor's `attr_camera_pan_type`: one interpolates the position request, any other
    /// value interpolates the angle offset and distance.
    pub pan_type: u8,
    pub duration: i32,
}

/// Start values an object pan keeps between invocations (the original keeps them in shared scratch).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ObjectPanState {
    /// The task vector, rotated by the bound angles for `showObjPanLoc`.
    vector: [i32; 3],
    /// The location offset rotated by the bound angles.
    offset: [i32; 3],
    /// The published camera position relative to the bound actor at the start.
    start_position: [i32; 3],
    /// Angles and length of the start position minus the offset.
    start_angles: [i32; 3],
    start_distance: i32,
    /// Wrapped difference from the start angles to the vector's angles.
    delta: [i32; 3],
    final_distance: i32,
}

/// Inputs of one `showObjWorld` invocation: the actor, its camera attributes, a world position
/// as the task arguments, and the poses the request modes read.
#[derive(Clone, Copy, Debug)]
pub struct WorldLocationInputs {
    pub actor: super::ScriptActorId,
    pub camera_move: u8,
    pub attributes: [u8; 6],
    pub position: [i32; 3],
    /// The position the record binds to (the actor's effective position).
    pub bound_position: [i32; 3],
    /// The actor's own local position, which `attr_camera_move` two reads instead.
    pub actor_position: [i32; 3],
    pub bound_angles: [u16; 3],
}

/// The camera natives that run an object pan, with the four task arguments (x, y, z, duration).
/// The `Cyrus` natives pan towards the selected player and return zero, without touching the
/// camera, while that player holds a weapon.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ObjectPanCommand {
    Object,
    ObjectLoc,
    ObjectWorld,
    Cyrus,
    CyrusLoc,
}

impl ObjectPanCommand {
    pub fn name(self) -> &'static str {
        match self {
            Self::Object => "showObjPan",
            Self::ObjectLoc => "showObjPanLoc",
            Self::ObjectWorld => "showObjWorldPan",
            Self::Cyrus => "showCyrusPan",
            Self::CyrusLoc => "showCyrusPanLoc",
        }
    }

    pub fn named(function: &str) -> Option<Self> {
        [
            Self::Object,
            Self::ObjectLoc,
            Self::ObjectWorld,
            Self::Cyrus,
            Self::CyrusLoc,
        ]
        .into_iter()
        .find(|command| command.name().eq_ignore_ascii_case(function))
    }

    /// The axes the pan's vector is given in once any world-position conversion is done.
    pub fn frame(self) -> VectorFrame {
        match self {
            Self::ObjectLoc | Self::CyrusLoc => VectorFrame::Actor,
            Self::Object | Self::ObjectWorld | Self::Cyrus => VectorFrame::World,
        }
    }

    /// Whether the arguments are a world position (`showObjWorldPan`).
    pub fn world(self) -> bool {
        self == Self::ObjectWorld
    }

    /// Whether the target is the selected player, guarded by the holding-weapon test.
    pub fn targets_selected_player(self) -> bool {
        matches!(self, Self::Cyrus | Self::CyrusLoc)
    }
}

/// The location camera natives. `Cyrus` natives target the selected player and return zero,
/// without touching the camera, while that player holds a weapon.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum LocationCommand {
    Object,
    ObjectLoc,
    ObjectWorld,
    Cyrus,
    CyrusLoc,
}

impl LocationCommand {
    pub fn name(self) -> &'static str {
        match self {
            Self::Object => "showObj",
            Self::ObjectLoc => "showObjLoc",
            Self::ObjectWorld => "showObjWorld",
            Self::Cyrus => "showCyrus",
            Self::CyrusLoc => "showCyrusLoc",
        }
    }

    pub fn named(function: &str) -> Option<Self> {
        [
            Self::Object,
            Self::ObjectLoc,
            Self::ObjectWorld,
            Self::Cyrus,
            Self::CyrusLoc,
        ]
        .into_iter()
        .find(|command| command.name().eq_ignore_ascii_case(function))
    }

    /// The axes the vector argument is given in; a world position is not a vector.
    pub fn frame(self) -> VectorFrame {
        match self {
            Self::ObjectLoc | Self::CyrusLoc => VectorFrame::Actor,
            Self::Object | Self::ObjectWorld | Self::Cyrus => VectorFrame::World,
        }
    }

    /// Whether the arguments are a world position (`showObjWorld`).
    pub fn world(self) -> bool {
        self == Self::ObjectWorld
    }

    pub fn targets_selected_player(self) -> bool {
        matches!(self, Self::Cyrus | Self::CyrusLoc)
    }
}

/// The camera pose update a location native runs inside its first invocation. The owner supplies
/// the geometry, hit history and settings and keeps them behind interior mutability (the handle is
/// shared); the world applies the logical camera actor positions it publishes only after the
/// native succeeded.
pub trait CameraUpdateService: std::fmt::Debug {
    /// Replace `state` with its updated pose. A failure must leave `state` untouched.
    fn update(&self, state: &mut CameraState) -> Result<()>;
    /// The camera actors' new positions from the updates since the last call.
    fn take_published(&self) -> Vec<(super::ScriptActorId, [i32; 3])>;
}

/// Shared handle to the owner's update service for one scheduler pass.
pub type CameraUpdateHandle<'a> = &'a dyn CameraUpdateService;

/// Inputs of one `lookObj` invocation: the actor's camera attributes and pose, as for a
/// location camera, and the task's duration.
#[derive(Clone, Copy, Debug)]
pub struct LookInputs {
    pub actor: super::ScriptActorId,
    pub camera_move: u8,
    pub attributes: [u8; 6],
    pub bound_position: [i32; 3],
    pub bound_angles: [u16; 3],
    pub duration: i32,
}

/// Start values a look keeps between invocations (the original keeps them in shared scratch).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LookState {
    /// The published angles when the look began.
    start: [i32; 3],
    /// Wrapped difference from them to the angles facing the actor's target.
    delta: [i32; 3],
}

/// A single-step wrap into half a turn either side of zero (2048 units per turn).
fn wrap_half_turn(value: i32) -> i32 {
    if value < -1024 {
        value.wrapping_add(2048)
    } else if value > 1024 {
        value.wrapping_sub(2048)
    } else {
        value
    }
}

/// Session-owned camera state. Construction requires established initial values.
#[derive(Clone, Debug)]
pub struct CameraState {
    pub records: [CameraRecord; 9],
    /// The selected record. Record fields are read and written through it.
    pub selected: CameraSlot,
    /// The selected-index word, separate from the record: the limiter, orientation and
    /// distance-height stages test it, and a pan leaves it on the slot being panned
    /// while the record is its scratch copy.
    pub index: CameraSlot,
    pub transition: u16,
    pub scripted_active: u16,
    /// Nesting count of begun scripted cameras, wrapping.
    pub scripted_nesting: u32,
    /// Raised when the scripted-camera release step changes the selected record.
    pub reselect_pending: bool,
    pub current: CameraPose,
    pub previous: CameraPose,
    pub transformed_basis: [[i32; 3]; 2],
    pub camera_actor: super::ScriptActorId,
    pub alternate_actor: super::ScriptActorId,
    pub marks: CameraMarks,
    pub render: RenderPose,
    /// Screen shake level, zero to 32: the frame's pitch wobble amplitude.
    pub shake: u8,
}

impl CameraState {
    /// Request a slot change, retaining the active transition's selection guard.
    /// A change selects both the record and the selected index.
    pub fn select(&mut self, slot: u8) -> Result<()> {
        let slot = CameraSlot::new(slot)?;
        if slot != self.selected
            && (self.transition == 0 || self.records[usize::from(self.selected.0)].flags & 1 != 0)
        {
            self.selected = slot;
            self.index = slot;
        }
        Ok(())
    }

    /// One showPlayer invocation. The native task owns the wrapping private counter.
    /// Returns the handler's raw result: `true` keeps the task active (the initial
    /// invocation), `false` completes it.
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
            self.marks.snap = true;
            Ok(true)
        } else {
            *flags &= !0x21;
            self.marks.hold = true;
            Ok(false)
        }
    }

    /// Begin a scripted camera: set the active word, request slot two (the selection guard
    /// applies), clear the selected record's position offset, count the nesting and reset
    /// the record's flags to `0x60`.
    pub fn begin_scripted(&mut self) -> Result<()> {
        self.scripted_active = 1;
        self.select(2)?;
        let record = &mut self.records[self.selected.index()];
        record.position_offset = [0; 3];
        self.scripted_nesting = self.scripted_nesting.wrapping_add(1);
        record.flags = 0x60;
        Ok(())
    }

    /// The scripted-camera release step. It selects record two, raising the reselect mark when
    /// that changed the record. While `duration - 1` exceeds `elapsed` it returns `true`;
    /// otherwise it ends one nesting level, clears the active word at zero and returns `false`.
    pub fn release_scripted(&mut self, duration: i32, elapsed: i32) -> bool {
        if self.selected.0 != 2 {
            self.selected = CameraSlot(2);
            self.reselect_pending = true;
        }
        if duration.wrapping_sub(1) > elapsed {
            return true;
        }
        self.scripted_nesting = self.scripted_nesting.wrapping_sub(1);
        if self.scripted_nesting == 0 {
            self.scripted_active = 0;
        }
        false
    }

    /// One `showPlayerPan(duration)` invocation. The first invocation copies the requested
    /// slot's record to record two, which the pan then edits, and records where the camera
    /// currently is; each invocation interpolates record two's angle offset and distance
    /// from there to the slot's own values. The last restores the record and selects the
    /// slot through `showPlayer`. Returns `true` while the task stays active.
    /// `bound_position`/`bound_angles` are the selected player's pose.
    pub fn show_player_pan(
        &mut self,
        pan: &PanInputs,
        state: &mut Option<PanState>,
        counter: &mut u32,
        elapsed: i32,
    ) -> Result<bool> {
        if pan.duration < 0 {
            // The original writes a diagnostic line and completes before touching any state.
            return Ok(false);
        }
        *counter = counter.wrapping_add(1);
        if *counter == 1 && elapsed == 0 {
            let slot = CameraSlot::new(pan.requested_slot.unwrap_or(0))?;
            self.selected = slot;
            self.begin_scripted()?;
            self.records[2] = self.records[slot.index()].clone();
            self.index = slot;
            let record = &self.records[self.selected.index()];
            let from_player: [i32; 3] = std::array::from_fn(|axis| {
                self.current.position[axis]
                    .wrapping_sub(record.position_offset[axis])
                    .wrapping_sub(pan.bound_position[axis])
            });
            let start = direction_angles(from_player)?;
            let target: [i32; 3] = std::array::from_fn(|axis| {
                record.angle_offset[axis].wrapping_sub(i32::from(pan.bound_angles[axis]))
            });
            *state = Some(PanState {
                start_distance: super::rounded_distance(from_player)? >> 8,
                final_distance: record.distance,
                delta: std::array::from_fn(|axis| {
                    wrap_half_turn(target[axis].wrapping_sub(start[axis]))
                }),
                start,
                final_offset: target,
            });
        }
        let saved = (*state).ok_or_else(|| {
            anyhow::anyhow!("showPlayerPan start state unavailable (task restored mid-pan)")
        })?;
        let running = self.release_scripted(pan.duration, elapsed);
        let record = &mut self.records[self.selected.index()];
        if running && pan.duration > 1 {
            let span = pan.duration - 1;
            record.flags &= !0x0c;
            record.angle_offset = std::array::from_fn(|axis| {
                i32::from(pan.bound_angles[axis])
                    .wrapping_add(saved.start[axis])
                    .wrapping_add(saved.delta[axis].wrapping_mul(elapsed) / span)
            });
            record.distance = saved.start_distance.wrapping_add(
                saved
                    .final_distance
                    .wrapping_sub(saved.start_distance)
                    .wrapping_mul(elapsed)
                    / span,
            );
            return Ok(true);
        }
        record.angle_offset = saved.final_offset;
        record.distance = saved.final_distance;
        record.flags &= !0x01;
        record.flags &= !0x20;
        self.show_player(pan.requested_slot, counter, elapsed)?;
        Ok(false)
    }

    /// One invocation of the location camera (`showObj` and `showObjLoc`, and their `showCyrus`
    /// wrappers for the selected player). The first invocation begins a scripted camera bound to the actor, converts the
    /// actor's camera attributes to offsets, writes the request its `attr_camera_move` selects
    /// from the task arguments, then runs one camera update through `update` and stays active.
    /// Later invocations run the release step with duration zero and complete.
    ///
    /// All edits happen on a copy that replaces the state only after `update` succeeds, so a
    /// failed update leaves the camera and the private counter untouched.
    /// The original also sets marks (two for `showObjLoc`, one for `showObj`) whose readers
    /// are not ported; they are not modelled.
    pub fn show_location(
        &mut self,
        inputs: &LocationInputs,
        counter: &mut u32,
        elapsed: i32,
        update: &mut dyn FnMut(&mut CameraState) -> Result<()>,
    ) -> Result<bool> {
        let mut next = self.clone();
        let next_counter = counter.wrapping_add(1);
        if next_counter != 1 || elapsed != 0 {
            next.release_scripted(0, elapsed);
            *self = next;
            *counter = next_counter;
            return Ok(false);
        }
        next.records[next.selected.index()].flags |= 0x01;
        next.marks.snap = true;
        next.marks.hold |= inputs.frame == VectorFrame::Actor;
        next.begin_scripted()?;
        let slot = next.selected.index();
        next.records[slot].actor = inputs.actor;
        next.set_location_offsets(inputs.attributes);
        let offset = rotate(next.records[slot].position_offset, inputs.bound_angles)?;
        let mut vector = inputs.arguments.map(|argument| argument.wrapping_shl(8));
        if inputs.frame == VectorFrame::Actor {
            vector = rotate(vector, inputs.bound_angles)?;
        }
        let record = &mut next.records[slot];
        record.flags |= 0x01;
        let relative = add(vector, offset);
        record.request_move(
            inputs.camera_move,
            add(relative, inputs.bound_position),
            relative,
            vector,
            inputs.bound_angles,
        )?;
        update(&mut next)?;
        let slot = next.selected.index();
        next.records[slot].flags &= !0x01;
        *self = next;
        *counter = next_counter;
        Ok(true)
    }

    /// One invocation of `showObjPan` or `showObjPanLoc`. The first invocation begins a scripted
    /// camera bound to the actor, converts its camera attributes to offsets and records where the
    /// camera is relative to it. Every invocation then runs the scripted release step; while it
    /// keeps running (and the duration exceeds one) the selected record is interpolated from the
    /// start towards the task's vector, otherwise the final request for `attr_camera_move` is
    /// written, the retained flag cleared and the task completes. Unlike the location camera no
    /// camera update runs here. Returns `true` while the task stays active.
    ///
    /// A negative duration completes before touching the counter or the camera; `showObjPan` has
    /// already marked the selected record with flag `0x40` by then, and marks it again at the end.
    /// All edits happen on a copy committed only on success. The original also sets a mark whose
    /// reader is not ported; it is not modelled.
    pub fn show_object_pan(
        &mut self,
        pan: &ObjectPanInputs,
        state: &mut Option<ObjectPanState>,
        counter: &mut u32,
        elapsed: i32,
    ) -> Result<bool> {
        let plain = pan.location.frame == VectorFrame::World;
        let inputs = &pan.location;
        let mut next = self.clone();
        next.marks.forced_glide = true;
        if plain {
            next.records[next.selected.index()].flags |= 0x40;
        }
        if pan.duration < 0 {
            *self = next;
            return Ok(false);
        }
        let next_counter = counter.wrapping_add(1);
        let mut next_state = *state;
        if next_counter == 1 && elapsed == 0 {
            next.begin_scripted()?;
            let slot = next.selected.index();
            next.records[slot].actor = inputs.actor;
            next.set_location_offsets(inputs.attributes);
            let offset = rotate(next.records[slot].position_offset, inputs.bound_angles)?;
            let mut vector = inputs.arguments.map(|argument| argument.wrapping_shl(8));
            if !plain {
                vector = rotate(vector, inputs.bound_angles)?;
            }
            let start_position: [i32; 3] = std::array::from_fn(|axis| {
                next.current.position[axis].wrapping_sub(inputs.bound_position[axis])
            });
            let from_offset: [i32; 3] =
                std::array::from_fn(|axis| start_position[axis].wrapping_sub(offset[axis]));
            let start_angles = direction_angles(from_offset)?;
            let end_angles = direction_angles(vector)?;
            next_state = Some(ObjectPanState {
                vector,
                offset,
                start_position,
                start_distance: super::rounded_distance(from_offset)? >> 8,
                final_distance: super::rounded_distance(vector)? >> 8,
                delta: std::array::from_fn(|axis| {
                    wrap_half_turn(end_angles[axis].wrapping_sub(start_angles[axis]))
                }),
                start_angles,
            });
        }
        let saved = next_state.ok_or_else(|| {
            anyhow::anyhow!("object pan start state unavailable (task restored mid-pan)")
        })?;
        let running = next.release_scripted(pan.duration, elapsed);
        let relative = add(saved.vector, saved.offset);
        let record = &mut next.records[next.selected.index()];
        if running && pan.duration > 1 {
            let span = pan.duration - 1;
            let step = |from: i32, to: i32| {
                from.wrapping_add(to.wrapping_sub(from).wrapping_mul(elapsed) / span)
            };
            if pan.pan_type == 1 {
                record.flags = (record.flags | 0x04) & !0x08;
                record.request =
                    std::array::from_fn(|axis| step(saved.start_position[axis], relative[axis]));
            } else {
                record.flags &= !0x0c;
                record.angle_offset = std::array::from_fn(|axis| {
                    i32::from(inputs.bound_angles[axis]).wrapping_add(step(
                        saved.start_angles[axis],
                        saved.start_angles[axis].wrapping_add(saved.delta[axis]),
                    ))
                });
                record.distance = step(saved.start_distance, saved.final_distance);
            }
            *self = next;
            *state = next_state;
            *counter = next_counter;
            return Ok(true);
        }
        let absolute = if plain {
            add(saved.vector, inputs.bound_position)
        } else {
            add(relative, inputs.bound_position)
        };
        record.request_move(
            inputs.camera_move,
            absolute,
            relative,
            saved.vector,
            inputs.bound_angles,
        )?;
        record.flags &= !0x01;
        if plain {
            record.flags |= 0x40;
        }
        *self = next;
        *state = next_state;
        *counter = next_counter;
        Ok(false)
    }

    /// One invocation of `lookObj(duration)`: turn the camera in place to face the actor's
    /// target. The first invocation begins a scripted camera bound to the actor, converts its
    /// attributes to offsets, computes the angles the orientation stage gives at the published
    /// position, fixes the orientation (flag `0x02`) and requests the published position, then
    /// stays active without running the release step. Each later invocation runs the release
    /// step; while it keeps running (and the duration exceeds one) the published angles move from
    /// the start towards those angles. Otherwise the fixed
    /// orientation is released and the request `attr_camera_move` selects is written from the
    /// published position, relative to the bound position and the record's offset where the mode
    /// asks for it. Returns `true` while the task stays active.
    ///
    /// A negative duration completes before touching the counter or the camera. All edits happen
    /// on a copy committed only on success.
    pub fn look_at(
        &mut self,
        look: &LookInputs,
        state: &mut Option<LookState>,
        counter: &mut u32,
        elapsed: i32,
    ) -> Result<bool> {
        if look.duration < 0 {
            return Ok(false);
        }
        let mut next = self.clone();
        let next_counter = counter.wrapping_add(1);
        let mut next_state = *state;
        if next_counter == 1 && elapsed == 0 {
            next.begin_scripted()?;
            let slot = next.selected.index();
            next.records[slot].actor = look.actor;
            next.set_location_offsets(look.attributes);
            let start = next.current.angles;
            let position = next.current.position;
            let target = next
                .orient(position, look.bound_position, look.bound_angles)?
                .angles;
            let record = &mut next.records[slot];
            record.flags |= 0x0a;
            record.request = position;
            next_state = Some(LookState {
                start,
                delta: std::array::from_fn(|axis| {
                    wrap_half_turn(target[axis].wrapping_sub(start[axis]))
                }),
            });
            *self = next;
            *state = next_state;
            *counter = next_counter;
            return Ok(true);
        }
        let saved = next_state.ok_or_else(|| {
            anyhow::anyhow!("lookObj start state unavailable (task restored mid-look)")
        })?;
        let running = next.release_scripted(look.duration, elapsed);
        if running && look.duration > 1 {
            let span = look.duration - 1;
            next.current.angles = std::array::from_fn(|axis| {
                saved.start[axis].wrapping_add(saved.delta[axis].wrapping_mul(elapsed) / span)
            });
        } else {
            let position = next.current.position;
            let record = &mut next.records[next.selected.index()];
            record.flags &= !0x02;
            let relative = std::array::from_fn(|axis| {
                position[axis]
                    .wrapping_sub(look.bound_position[axis])
                    .wrapping_sub(record.position_offset[axis])
            });
            record.request_move(
                look.camera_move,
                position,
                relative,
                relative,
                look.bound_angles,
            )?;
        }
        *self = next;
        *state = next_state;
        *counter = next_counter;
        Ok(running && look.duration > 1)
    }

    /// One invocation of `showObjWorld`: a location camera whose three arguments are a world
    /// position. The vector is `(position - selected record's position offset) << 8`, read from the
    /// record selected on entry. The first invocation marks that record with flags `0x20` and
    /// `0x01`, begins a scripted camera bound to the actor, converts its attributes to offsets and
    /// writes the request `attr_camera_move` selects: the vector itself (zero, absolute), the
    /// vector minus the bound position (one, relative) or the orbit of the vector minus the
    /// actor's own position (two). Nothing is rotated and the record's offsets are not added. It then
    /// runs one camera update through `update`, clears flag `0x01` and stays active. Later
    /// invocations run the release step with duration zero and complete. As for `show_location`,
    /// edits are committed only if `update` succeeds.
    pub fn show_world_location(
        &mut self,
        inputs: &WorldLocationInputs,
        counter: &mut u32,
        elapsed: i32,
        update: &mut dyn FnMut(&mut CameraState) -> Result<()>,
    ) -> Result<bool> {
        let mut next = self.clone();
        let next_counter = counter.wrapping_add(1);
        if next_counter != 1 || elapsed != 0 {
            next.release_scripted(0, elapsed);
            *self = next;
            *counter = next_counter;
            return Ok(false);
        }
        let entry = &mut next.records[next.selected.index()];
        let vector: [i32; 3] = std::array::from_fn(|axis| {
            inputs.position[axis]
                .wrapping_sub(entry.position_offset[axis])
                .wrapping_shl(8)
        });
        entry.flags |= 0x21;
        next.marks.snap = true;
        next.begin_scripted()?;
        let slot = next.selected.index();
        next.records[slot].actor = inputs.actor;
        next.set_location_offsets(inputs.attributes);
        let record = &mut next.records[slot];
        record.flags |= 0x01;
        record.request_move(
            inputs.camera_move,
            vector,
            std::array::from_fn(|axis| vector[axis].wrapping_sub(inputs.bound_position[axis])),
            std::array::from_fn(|axis| vector[axis].wrapping_sub(inputs.actor_position[axis])),
            inputs.bound_angles,
        )?;
        update(&mut next)?;
        let slot = next.selected.index();
        next.records[slot].flags &= !0x01;
        *self = next;
        *counter = next_counter;
        Ok(true)
    }

    /// One invocation of `showObjWorldPan`: `showObjPan` with the task's world position converted
    /// to `(position << 8) - selected_bound_position`, using the position the record selected on
    /// entry is bound to, before `showObjPan` shifts it again. A negative duration completes
    /// without calling `showObjPan`, so it leaves the camera untouched.
    /// `pan.location.arguments` are the raw position.
    pub fn show_object_world_pan(
        &mut self,
        pan: &ObjectPanInputs,
        selected_bound_position: [i32; 3],
        state: &mut Option<ObjectPanState>,
        counter: &mut u32,
        elapsed: i32,
    ) -> Result<bool> {
        if pan.duration < 0 {
            return Ok(false);
        }
        let mut converted = *pan;
        converted.location.arguments = std::array::from_fn(|axis| {
            pan.location.arguments[axis]
                .wrapping_shl(8)
                .wrapping_sub(selected_bound_position[axis])
        });
        self.show_object_pan(&converted, state, counter, elapsed)
    }

    /// Location-camera setup: the actor's six attribute bytes (target x/y/z, then camera
    /// x/y/z) replace the selected record's target and position offsets.
    /// Each zero byte stays zero; any other becomes `(byte - 127) * 500`.
    pub fn set_location_offsets(&mut self, attributes: [u8; 6]) {
        let convert = |byte: u8| {
            if byte == 0 {
                0
            } else {
                (i32::from(byte) - 127) * 500
            }
        };
        let record = &mut self.records[usize::from(self.selected.0)];
        record.target_offset = std::array::from_fn(|axis| convert(attributes[axis]));
        record.position_offset = std::array::from_fn(|axis| convert(attributes[axis + 3]));
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
#[derive(Clone, Copy, Debug)]
pub struct MovementLimits {
    pub budget_increment: i32,
    pub vertical: i32,
    pub frame_scale: i32,
    pub selected_player_special_state: bool,
}

impl MovementLimits {
    /// The limiter's shared parameters from the installed values: the budget increment is
    /// `max_acc` and the vertical parameter is `max_y_vel` negated, both unscaled.
    /// `frame_scale` is the current signed frame scale; the special-state flag comes
    /// from the selected player.
    pub fn from_installed(
        limits: &CameraLimits,
        frame_scale: i32,
        selected_player_special_state: bool,
    ) -> Self {
        Self {
            budget_increment: limits.max_acc,
            vertical: limits.max_y_vel.wrapping_neg(),
            frame_scale,
            selected_player_special_state,
        }
    }
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
        if self.index.0 != 1 && !limits.selected_player_special_state {
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
        let offset = if self.index.0 < 2 {
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

/// Correction settings for one live update; the bound actor's pose comes from the world.
#[derive(Clone, Copy, Debug)]
pub struct CameraUpdateSettings {
    pub movement: MovementLimits,
    pub body_radius: f32,
    pub body_clearance: f32,
    pub terrain_enabled: bool,
}

/// Settled actor pose and explicitly initialized correction settings for one update.
pub struct CameraUpdateInputs {
    pub bound_position: [i32; 3],
    pub bound_angles: [u16; 3],
    pub movement: MovementLimits,
    pub body_radius: f32,
    pub body_clearance: f32,
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
        let slot = state.index.0;
        let record_slot = state.selected.0;
        let record = &mut state.records[usize::from(record_slot)];
        let mut position = record.candidate(inputs.bound_position, inputs.bound_angles)?;
        // A pan marks the frame to glide, which also skips the obstacle segment check.
        let segment_enabled = record.flags & 0x40 != 0 && !state.marks.forced_glide;
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
