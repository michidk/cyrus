//! Authored anatomy templates fitted to installed frame geometry. No game mesh is bundled.
use cyrus_mod_sdk::{
    Assembly, AttachmentBinding, BodyDesc, JointDesc, Pose, Quat, Shape, Vec3, VertexWeight,
};

#[derive(Clone, Copy, Debug)]
pub enum Profile {
    Biped,
    ArmsDown,
    Crouched,
    Gremlin,
    Robed,
    Mechanical,
    Serpent,
    Dragon,
    Bird,
}

pub fn profile(model: &str) -> Option<Profile> {
    let (family, variant) = model.split_at_checked(5)?;
    let number = variant.parse::<u8>().ok()?;
    match (family, number) {
        ("CYRSA", 1..=16 | 18..=21 | 23)
        | ("VANDA", 1..=3)
        | ("GARDA", 1..=4)
        | ("DRAMA", 1..=3)
        | ("RICHA", 1..=2)
        | ("GBLNA", 1..=3)
        | ("SWAPA", 1)
        | ("SKELA", 1..=4)
        | ("TROLA", 1..=2)
        | ("VERMA", 1..=2)
        | ("YAELA", 1) => Some(Profile::Biped),
        ("THUGA", 2 | 4) => Some(Profile::Biped),
        ("THUGA", 1 | 3 | 5) | ("ZOMBA", 1..=2) | ("CYRSA", 17) => Some(Profile::ArmsDown),
        ("GOLMA", 1..=2) | ("OGREA", 1..=2) => Some(Profile::Crouched),
        ("DGOLA", 1) => Some(Profile::Biped),
        ("GREMA", 1) => Some(Profile::Gremlin),
        ("NGASA", 1..=2) => Some(Profile::Robed),
        ("ROBOA", 1..=3) => Some(Profile::Mechanical),
        ("SERPA", 1) => Some(Profile::Serpent),
        ("NFARA", 1..=3) => Some(Profile::Dragon),
        _ if model == "KANA001" => Some(Profile::Bird),
        _ => None,
    }
}

#[derive(Clone)]
struct Bone {
    a: Vec3,
    b: Vec3,
    parent: Option<usize>,
    radius: f32,
    mass: f32,
    limit: f32,
}
fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}
fn bone(
    bones: &mut Vec<Bone>,
    a: Vec3,
    b: Vec3,
    parent: Option<usize>,
    radius: f32,
    mass: f32,
    limit: f32,
) -> usize {
    let id = bones.len();
    bones.push(Bone {
        a,
        b,
        parent,
        radius,
        mass,
        limit,
    });
    id
}
fn anatomy(profile: Profile) -> Vec<Bone> {
    let mut b = Vec::new();
    if matches!(profile, Profile::Serpent) {
        for i in 0..9 {
            bone(
                &mut b,
                v(0., i as f32 / 9., 0.),
                v(0., (i + 1) as f32 / 9., 0.),
                (i as usize).checked_sub(1),
                0.025,
                3.,
                0.65,
            );
        }
        return b;
    }
    if matches!(profile, Profile::Dragon | Profile::Bird) {
        let bird = matches!(profile, Profile::Bird);
        let core = bone(
            &mut b,
            v(0., 0.35, -0.12),
            v(0., 0.43, 0.12),
            None,
            0.07,
            18.,
            0.4,
        );
        let neck = bone(
            &mut b,
            v(0., 0.43, 0.1),
            v(0., 0.68, 0.3),
            Some(core),
            0.035,
            3.,
            0.6,
        );
        bone(
            &mut b,
            v(0., 0.68, 0.3),
            v(0., 0.85, 0.42),
            Some(neck),
            0.04,
            3.,
            0.6,
        );
        let mut tail = core;
        for i in 0..3 {
            tail = bone(
                &mut b,
                v(0., 0.38, -0.12 - i as f32 * 0.12),
                v(0., 0.36, -0.24 - i as f32 * 0.12),
                Some(tail),
                0.02,
                1.,
                0.7,
            );
        }
        for side in [-1., 1.] {
            let wing = bone(
                &mut b,
                v(side * 0.04, 0.43, 0.),
                v(side * 0.25, 0.45, 0.),
                Some(core),
                0.018,
                2.,
                1.0,
            );
            bone(
                &mut b,
                v(side * 0.25, 0.45, 0.),
                v(side * 0.48, 0.45, -0.08),
                Some(wing),
                0.012,
                1.,
                1.2,
            );
            for z in if bird { vec![-0.02] } else { vec![-0.12, 0.12] } {
                let leg = bone(
                    &mut b,
                    v(side * 0.04, 0.35, z),
                    v(side * 0.07, 0.17, z - 0.04),
                    Some(core),
                    0.025,
                    3.,
                    0.8,
                );
                bone(
                    &mut b,
                    v(side * 0.07, 0.17, z - 0.04),
                    v(side * 0.09, 0.03, z + 0.04),
                    Some(leg),
                    0.02,
                    2.,
                    0.7,
                );
            }
        }
        return b;
    }
    let mechanical = matches!(profile, Profile::Mechanical);
    let crouch = matches!(profile, Profile::Crouched | Profile::Gremlin);
    let hips = if mechanical {
        0.25
    } else if crouch {
        0.42
    } else {
        0.52
    };
    let shoulder = if mechanical { 0.65 } else { 0.79 };
    let root = bone(
        &mut b,
        v(0., hips, 0.),
        v(0., hips + 0.12, 0.),
        None,
        0.09,
        12.,
        0.4,
    );
    let chest = bone(
        &mut b,
        v(0., hips + 0.12, 0.),
        v(0., shoulder, 0.),
        Some(root),
        0.1,
        18.,
        0.45,
    );
    if !mechanical {
        bone(
            &mut b,
            v(0., shoulder, 0.),
            v(0., 0.97, 0.),
            Some(chest),
            0.065,
            5.,
            0.65,
        );
    }
    for side in [-1., 1.] {
        let down = matches!(
            profile,
            Profile::ArmsDown | Profile::Crouched | Profile::Gremlin
        );
        let upper = bone(
            &mut b,
            v(side * 0.12, shoulder, 0.),
            v(
                side * if down { 0.19 } else { 0.3 },
                if down { 0.6 } else { shoulder },
                0.,
            ),
            Some(chest),
            0.04,
            3.,
            1.4,
        );
        let upper_end = b[upper].b;
        let lower = bone(
            &mut b,
            upper_end,
            v(
                side * if down { 0.21 } else { 0.45 },
                if down { 0.43 } else { shoulder },
                0.,
            ),
            Some(upper),
            0.035,
            2.,
            1.3,
        );
        let lower_end = b[lower].b;
        bone(
            &mut b,
            lower_end,
            v(
                side * if down { 0.22 } else { 0.5 },
                if down { 0.38 } else { shoulder },
                0.,
            ),
            Some(lower),
            0.035,
            0.7,
            0.5,
        );
        if !mechanical {
            let thigh = bone(
                &mut b,
                v(side * 0.075, hips, 0.),
                v(
                    side * if crouch { 0.17 } else { 0.08 },
                    0.27,
                    if crouch { 0.12 } else { 0. },
                ),
                Some(root),
                0.055,
                7.,
                0.9,
            );
            let thigh_end = b[thigh].b;
            let shin = bone(
                &mut b,
                thigh_end,
                v(side * if crouch { 0.2 } else { 0.08 }, 0.06, 0.),
                Some(thigh),
                0.04,
                4.,
                1.3,
            );
            let shin_end = b[shin].b;
            bone(
                &mut b,
                shin_end,
                v(side * if crouch { 0.2 } else { 0.08 }, 0.025, 0.16),
                Some(shin),
                0.04,
                1.,
                0.45,
            );
        }
    }
    b
}

fn distance(p: Vec3, a: Vec3, b: Vec3) -> f32 {
    let d = b - a;
    let t = ((p - a).dot(d) / d.dot(d).max(1e-8)).clamp(0., 1.);
    (p - (a + d * t)).length()
}
fn rotation_y(direction: Vec3) -> Option<Quat> {
    let d = direction.normalized()?;
    if d.y < -0.9999 {
        return Some(Quat {
            x: 1.,
            y: 0.,
            z: 0.,
            w: 0.,
        });
    }
    let q = Quat {
        x: d.z,
        y: 0.,
        z: -d.x,
        w: 1. + d.y,
    };
    let scale = 1. / (q.x * q.x + q.y * q.y + q.z * q.z + q.w * q.w).sqrt();
    Some(Quat {
        x: q.x * scale,
        y: q.y * scale,
        z: q.z * scale,
        w: q.w * scale,
    })
}
fn multiply(a: Quat, b: Quat) -> Quat {
    Quat {
        x: a.w * b.x + a.x * b.w + a.y * b.z - a.z * b.y,
        y: a.w * b.y - a.x * b.z + a.y * b.w + a.z * b.x,
        z: a.w * b.z + a.x * b.y - a.y * b.x + a.z * b.w,
        w: a.w * b.w - a.x * b.x - a.y * b.y - a.z * b.z,
    }
}
fn endpoint(point: Vec3, reference: &[Vec3], current: &[Vec3]) -> Vec3 {
    let mut nearest = (0..reference.len())
        .map(|i| (i, (reference[i] - point).length()))
        .collect::<Vec<_>>();
    nearest.sort_by(|a, b| a.1.total_cmp(&b.1));
    let mut position = Vec3::ZERO;
    let mut total = 0.;
    for &(i, d) in nearest.iter().take(6) {
        let weight = 1. / (d * d + 0.0001);
        position += current[i] * weight;
        total += weight;
    }
    position * (1. / total)
}
/// Templates describe mod preferences in normalized reference bounds. Fitting transfers each
/// landmark through nearby source vertices, so the current animated frame starts undeformed.
/// The simulation then deforms that frozen frame with the resulting body bindings.
pub fn fit(
    profile: Profile,
    reference: &[Vec3],
    current: &[Vec3],
    pose: Pose,
    source: Option<Vec3>,
    strength: f32,
    attachments: &[Pose],
) -> Option<Assembly> {
    if reference.len() < 12 || reference.len() != current.len() {
        return None;
    }
    let mut lo = reference[0];
    let mut hi = lo;
    for p in reference {
        lo = v(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z));
        hi = v(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z));
    }
    let extent = hi - lo;
    if extent.x <= 0. || extent.y <= 0. || extent.z <= 0. {
        return None;
    }
    let template = anatomy(profile);
    let map = |p: Vec3| {
        v(
            lo.x + (p.x + 0.5) * extent.x,
            lo.y + p.y * extent.y,
            lo.z + (p.z + 0.5) * extent.z,
        )
    };
    let world = |p: Vec3| pose.position + pose.rotation.rotate(p);
    let impulse = source
        .and_then(|p| (pose.position - p).normalized())
        .unwrap_or(Vec3::ZERO)
        * strength.clamp(0., 3.)
        * 18.;
    let mut assembly = Assembly::default();
    let mut bodies = Vec::new();
    let mut centers = Vec::new();
    for (i, bone) in template.iter().enumerate() {
        let a = endpoint(map(bone.a), reference, current);
        let b = endpoint(map(bone.b), reference, current);
        let delta = b - a;
        let length = delta.length().max(0.02);
        let center = world((a + b) * 0.5);
        let radius = (extent.y * bone.radius).max(0.015).min(length * 0.45);
        let body = assembly.body(BodyDesc {
            pose: Pose {
                position: center,
                rotation: multiply(pose.rotation, rotation_y(delta)?),
            },
            shape: Shape::Capsule {
                half_height: (length * 0.5 - radius).max(0.005),
                radius,
            },
            mass: bone.mass,
            linear_damping: 0.25,
            angular_damping: 0.65,
            velocity: Vec3::ZERO,
            acceleration: v(0., -9.81, 0.),
            impulse: if i == 0 { impulse } else { Vec3::ZERO },
        });
        bodies.push(body);
        centers.push(center);
        if let Some(parent) = bone.parent {
            assembly.joints.push(JointDesc {
                bodies: [bodies[parent], body],
                frame: Pose {
                    position: world(a),
                    rotation: pose.rotation,
                },
                limits: [[-bone.limit, bone.limit]; 3],
            });
        }
    }
    for &p in reference {
        let (body, _) = template
            .iter()
            .enumerate()
            .map(|(i, b)| (i, distance(p, map(b.a), map(b.b))))
            .min_by(|a, b| a.1.total_cmp(&b.1))?;
        assembly.weights.push(vec![VertexWeight {
            body: bodies[body],
            weight: 1.,
        }]);
    }
    for (index, attachment) in attachments.iter().enumerate() {
        let (body, _) = centers
            .iter()
            .enumerate()
            .map(|(i, p)| (i, (*p - attachment.position).length()))
            .min_by(|a, b| a.1.total_cmp(&b.1))?;
        assembly.attachments.push(AttachmentBinding {
            index,
            body: bodies[body],
        });
    }
    Some(assembly)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profiles_are_articulated_and_unknown_models_remain_unsupported() {
        for model in [
            "CYRSA001", "THUGA001", "GOLMA001", "GREMA001", "NGASA001", "ROBOA001", "SERPA001",
            "NFARA001", "KANA001",
        ] {
            let bones = anatomy(profile(model).unwrap());
            assert!(bones.len() >= 8, "{model}");
            assert_eq!(
                bones.iter().filter(|b| b.parent.is_some()).count(),
                bones.len() - 1
            );
        }
        assert!(profile("UNKNOWN").is_none());
        assert!(profile("GARDA099").is_none());
    }
}

#[cfg(test)]
mod installed {
    use super::*;
    #[test]
    fn installed_model_variants_fit_in_reference_moving_and_final_frames() {
        let install = std::env::var_os("REDGUARD_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard")
            });
        if !install.is_dir() {
            eprintln!("skipped: no Redguard install at {}", install.display());
            return;
        }
        let families = [
            ("CYRSA", 23),
            ("VANDA", 3),
            ("GARDA", 4),
            ("THUGA", 5),
            ("DRAMA", 3),
            ("RICHA", 2),
            ("ROBOA", 3),
            ("GBLNA", 3),
            ("GOLMA", 2),
            ("DGOLA", 1),
            ("GREMA", 1),
            ("SWAPA", 1),
            ("NGASA", 2),
            ("OGREA", 2),
            ("SKELA", 4),
            ("TROLA", 2),
            ("ZOMBA", 2),
            ("NFARA", 3),
            ("SERPA", 1),
            ("VERMA", 2),
            ("KANA", 1),
            ("YAELA", 1),
        ];
        let pose = Pose {
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
        };
        let mut tested = 0;
        for (family, count) in families {
            for variant in 1..=count {
                if family == "CYRSA" && variant == 22 {
                    continue;
                }
                let name = format!("{family}{variant:03}");
                let path = install.join("fxart").join(format!("{name}.3DC"));
                let bytes =
                    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                let model = rgpre::import::model3d::parse_3d_file(&bytes).unwrap();
                let convert = |frame: usize| {
                    model.frame_vertex_data[frame]
                        .coords
                        .iter()
                        .map(|p| v(-p.x / 20., -p.y / 20., p.z / 20.))
                        .collect::<Vec<_>>()
                };
                let reference = convert(0);
                for frame in [0, 1, model.frame_vertex_data.len() - 1] {
                    let current = convert(frame);
                    let rig = fit(
                        profile(&name).unwrap(),
                        &reference,
                        &current,
                        pose,
                        None,
                        1.,
                        &[],
                    )
                    .unwrap_or_else(|| panic!("{name} frame {frame}: cannot fit"));
                    assert_eq!(rig.weights.len(), current.len());
                    assert!(rig.bodies().len() > 1);
                    for body in rig.bodies() {
                        assert!(
                            body.pose.to_list().iter().all(|p| p.is_finite()),
                            "{name} frame {frame}"
                        );
                        assert!((body.pose.rotation.rotate(Vec3::UP).length() - 1.).abs() < 0.001);
                        assert_eq!(
                            body.impulse,
                            Vec3::ZERO,
                            "unknown sources must have no directed impulse"
                        );
                    }
                }
                tested += 1;
            }
        }
        eprintln!("fitted {tested} installed model variants in three frames each");
    }
    #[test]
    fn source_direction_uses_world_coordinates_and_unknown_has_no_impulse() {
        let vertices = (0..5)
            .flat_map(|x| {
                (0..5).flat_map(move |y| (0..3).map(move |z| v(x as f32, y as f32, z as f32)))
            })
            .collect::<Vec<_>>();
        let pose = Pose {
            position: v(10., 5., 2.),
            rotation: Quat::from_yaw_pitch(1.2, 0.),
        };
        let left = fit(
            Profile::Biped,
            &vertices,
            &vertices,
            pose,
            Some(v(0., 5., 2.)),
            1.,
            &[],
        )
        .unwrap();
        let right = fit(
            Profile::Biped,
            &vertices,
            &vertices,
            pose,
            Some(v(20., 5., 2.)),
            1.,
            &[],
        )
        .unwrap();
        assert!(left.bodies()[0].impulse.x > 0.);
        assert_eq!(left.bodies()[0].impulse, -right.bodies()[0].impulse);
        let unknown = fit(Profile::Biped, &vertices, &vertices, pose, None, 1., &[]).unwrap();
        assert_eq!(unknown.bodies()[0].impulse, Vec3::ZERO);
    }
}
