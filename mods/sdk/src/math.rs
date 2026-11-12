//! Vectors, rotations and poses in the host's scene units (right-handed, Y up, the camera looks
//! down -Z when its rotation is the identity).
use std::ops::{Add, AddAssign, Mul, Neg, Sub};

/// A point or direction.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    pub const ZERO: Vec3 = Vec3::new(0.0, 0.0, 0.0);
    pub const UP: Vec3 = Vec3::new(0.0, 1.0, 0.0);
    /// Where an unrotated camera looks.
    pub const FORWARD: Vec3 = Vec3::new(0.0, 0.0, -1.0);

    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Vec3 { x, y, z }
    }

    pub fn dot(self, other: Vec3) -> f32 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    pub fn cross(self, other: Vec3) -> Vec3 {
        Vec3::new(
            self.y * other.z - self.z * other.y,
            self.z * other.x - self.x * other.z,
            self.x * other.y - self.y * other.x,
        )
    }

    pub fn length(self) -> f32 {
        self.dot(self).sqrt()
    }

    /// The vector scaled to length 1; `None` for a zero (or non-finite) vector.
    pub fn normalized(self) -> Option<Vec3> {
        let length = self.length();
        (length > 0.0 && length.is_finite()).then(|| self * (1.0 / length))
    }
}

impl Add for Vec3 {
    type Output = Vec3;
    fn add(self, other: Vec3) -> Vec3 {
        Vec3::new(self.x + other.x, self.y + other.y, self.z + other.z)
    }
}

impl AddAssign for Vec3 {
    fn add_assign(&mut self, other: Vec3) {
        *self = *self + other;
    }
}

impl Sub for Vec3 {
    type Output = Vec3;
    fn sub(self, other: Vec3) -> Vec3 {
        Vec3::new(self.x - other.x, self.y - other.y, self.z - other.z)
    }
}

impl Mul<f32> for Vec3 {
    type Output = Vec3;
    fn mul(self, scale: f32) -> Vec3 {
        Vec3::new(self.x * scale, self.y * scale, self.z * scale)
    }
}

impl Neg for Vec3 {
    type Output = Vec3;
    fn neg(self) -> Vec3 {
        Vec3::new(-self.x, -self.y, -self.z)
    }
}

impl From<[f32; 3]> for Vec3 {
    fn from([x, y, z]: [f32; 3]) -> Self {
        Vec3 { x, y, z }
    }
}

impl From<Vec3> for [f32; 3] {
    fn from(v: Vec3) -> Self {
        [v.x, v.y, v.z]
    }
}

/// A unit quaternion.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quat {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

impl Quat {
    pub const IDENTITY: Quat = Quat {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        w: 1.0,
    };

    /// Yaw about Y (positive turns left), then pitch about X (positive looks up), in radians.
    pub fn from_yaw_pitch(yaw: f32, pitch: f32) -> Self {
        let (sy, cy) = (yaw * 0.5).sin_cos();
        let (sp, cp) = (pitch * 0.5).sin_cos();
        Quat {
            x: cy * sp,
            y: sy * cp,
            z: -sy * sp,
            w: cy * cp,
        }
    }

    /// Rotate `v`: v + 2w(u×v) + 2u×(u×v) with u the vector part.
    pub fn rotate(self, v: Vec3) -> Vec3 {
        let u = Vec3::new(self.x, self.y, self.z);
        let uv = u.cross(v);
        let uuv = u.cross(uv);
        v + (uv * self.w + uuv) * 2.0
    }

    /// The yaw and pitch, in radians, that `from_yaw_pitch` turns into this view direction.
    pub fn yaw_pitch(self) -> (f32, f32) {
        let f = self.rotate(Vec3::FORWARD);
        ((-f.x).atan2(-f.z), f.y.clamp(-1.0, 1.0).asin())
    }
}

/// Where something is and which way it faces.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub position: Vec3,
    pub rotation: Quat,
}

impl Pose {
    /// The host's layout: x, y, z, then the rotation's x, y, z, w.
    pub fn from_list(values: &[f32]) -> Option<Self> {
        let &[x, y, z, qx, qy, qz, qw] = values else {
            return None;
        };
        Some(Pose {
            position: Vec3::new(x, y, z),
            rotation: Quat {
                x: qx,
                y: qy,
                z: qz,
                w: qw,
            },
        })
    }

    pub fn to_list(self) -> [f32; 7] {
        let (p, q) = (self.position, self.rotation);
        [p.x, p.y, p.z, q.x, q.y, q.z, q.w]
    }

    /// The direction it looks.
    pub fn forward(self) -> Vec3 {
        self.rotation.rotate(Vec3::FORWARD)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-5
    }

    #[test]
    fn an_unrotated_pose_looks_down_negative_z() {
        let pose = Pose {
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
        };
        assert!(close(pose.forward(), Vec3::FORWARD));
    }

    #[test]
    fn yaw_and_pitch_survive_a_round_trip() {
        for (yaw, pitch) in [(0.0, 0.0), (1.2, 0.4), (-2.5, -1.0), (3.0, 1.4)] {
            let (y, p) = Quat::from_yaw_pitch(yaw, pitch).yaw_pitch();
            let again = Quat::from_yaw_pitch(y, p);
            let original = Quat::from_yaw_pitch(yaw, pitch);
            assert!(close(
                again.rotate(Vec3::FORWARD),
                original.rotate(Vec3::FORWARD)
            ));
        }
    }

    #[test]
    fn yaw_turns_left_and_pitch_looks_up() {
        let left = Quat::from_yaw_pitch(std::f32::consts::FRAC_PI_2, 0.0).rotate(Vec3::FORWARD);
        assert!(close(left, Vec3::new(-1.0, 0.0, 0.0)));
        let up = Quat::from_yaw_pitch(0.0, std::f32::consts::FRAC_PI_2).rotate(Vec3::FORWARD);
        assert!(close(up, Vec3::UP));
    }

    #[test]
    fn a_pose_list_needs_exactly_seven_values() {
        assert!(Pose::from_list(&[0.0; 6]).is_none());
        let pose = Pose::from_list(&[1.0, 2.0, 3.0, 0.0, 0.0, 0.0, 1.0]).unwrap();
        assert_eq!(pose.to_list(), [1.0, 2.0, 3.0, 0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn zero_has_no_direction() {
        assert!(Vec3::ZERO.normalized().is_none());
        assert!((Vec3::new(3.0, 0.0, 4.0).normalized().unwrap().length() - 1.0).abs() < 1e-6);
    }
}
