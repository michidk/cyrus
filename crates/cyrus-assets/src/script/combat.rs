//! Prepared combat attacks with authored omissions retained explicitly.
use serde::{Deserialize, Serialize};

/// One attack's ordered motions and constructor range accumulators.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attack {
    /// Distinguishes an authored combo even when its motion list has zero or one entry.
    pub is_combo: bool,
    pub motions: Vec<Motion>,
    pub min_range: i32,
    pub max_range: i32,
}

/// One COMBAT.INI motion. Optional fields have no inferred runtime default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Motion {
    pub section: u32,
    pub kind: i32,
    pub animation_group: i32,
    pub min_range: i32,
    pub max_range: i32,
    pub first_collide_frame: i32,
    pub collide_duration: i32,
    pub arc: Option<i32>,
    pub arc_center: Option<i32>,
    pub elevation: Option<i32>,
    pub damage: Option<i32>,
    pub force: Option<i32>,
    pub defense: Option<i32>,
    pub first_defend_frame: Option<i32>,
    pub defend_duration: Option<i32>,
    pub col_vertex: Option<i32>,
    pub col_sphere_size: Option<i32>,
}

/// Prepare the contiguous attack table described in preservation's COMBAT.INI rules.
/// Combo references select motion sections directly, in authored order.
#[cfg(feature = "parsers")]
pub fn prepare(ini: &rgpre::import::combat_ini::CombatIni) -> anyhow::Result<Vec<Attack>> {
    use anyhow::{Context, ensure};
    use rgpre::import::combat_ini::AttackType;
    let mut attacks = Vec::new();
    for index in 0.. {
        let Some(record) = ini.attacks.get(&index) else {
            break;
        };
        if record.attack_type.is_none() && record.damage.is_none() {
            break;
        }
        let context = format!("COMBAT.INI attack{index:02}");
        let combo = record
            .attack_type
            .context(format!("{context}: type absent"))?
            == AttackType::Combo;
        let sections = if combo {
            let count = record
                .num_attacks
                .context(format!("{context}: num_attacks absent"))?;
            let children = [record.attack00, record.attack01, record.attack02];
            ensure!(
                count <= children.len() as u32,
                "{context}: num_attacks {count} exceeds supported reference fields"
            );
            children[..count as usize]
                .iter()
                .enumerate()
                .map(|(slot, child)| child.context(format!("{context}: attack{slot:02} absent")))
                .collect::<anyhow::Result<Vec<_>>>()?
        } else {
            vec![index]
        };
        let motions = sections
            .into_iter()
            .map(|section| {
                let source = ini
                    .attacks
                    .get(&section)
                    .with_context(|| format!("{context}: referenced attack{section:02} absent"))?;
                Motion::prepare(section, source).with_context(|| context.clone())
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        let (min_range, max_range) = if combo {
            motions.iter().fold((65535, -1), |(min, max), motion| {
                (min.min(motion.min_range), max.max(motion.max_range))
            })
        } else {
            (motions[0].min_range, motions[0].max_range)
        };
        attacks.push(Attack {
            is_combo: combo,
            motions,
            min_range,
            max_range,
        });
    }
    Ok(attacks)
}

#[cfg(feature = "parsers")]
impl Motion {
    fn prepare(section: u32, source: &rgpre::import::combat_ini::Attack) -> anyhow::Result<Self> {
        use anyhow::Context;
        use rgpre::import::combat_ini::{AttackType, Elevation};
        let required = |name: &str, value: Option<i32>| {
            value.with_context(|| format!("attack{section:02}: {name} absent"))
        };
        let kind = match source
            .attack_type
            .with_context(|| format!("attack{section:02}: type absent"))?
        {
            AttackType::Melee => 0,
            AttackType::Missile => 1,
            AttackType::Combo => 2,
            AttackType::Stab => 3,
            AttackType::Finishing => 4,
            AttackType::Unknown(value) => value,
        };
        Ok(Self {
            section,
            kind,
            animation_group: required("animation", source.animation)?
                .checked_add(97)
                .context("combat animation group overflow")?,
            min_range: required("min_range", source.min_range)?,
            max_range: required("max_range", source.max_range)?,
            first_collide_frame: required("first_collide_frame", source.first_collide_frame)?,
            collide_duration: required("collide_duration", source.collide_duration)?,
            arc: source.arc,
            arc_center: source.arc_center,
            elevation: source.elevation.map(|value| match value {
                Elevation::Regular => 0,
                Elevation::Above => 1,
                Elevation::Below => 2,
                Elevation::Unknown(value) => value,
            }),
            damage: source.damage,
            force: source.force,
            defense: source.defense,
            first_defend_frame: source.first_defend_frame,
            defend_duration: source.defend_duration,
            col_vertex: source.col_vertex,
            col_sphere_size: source.col_sphere_size,
        })
    }
}

#[cfg(all(test, feature = "parsers"))]
mod tests {
    use super::*;
    use rgpre::import::combat_ini::CombatIni;

    fn motion(section: u32, minimum: i32, maximum: i32) -> String {
        format!(
            "[attack{section:02}]\ntype=0\nmin_range={minimum}\nmax_range={maximum}\nanimation=5\nfirst_collide_frame=3\ncollide_duration=2\n"
        )
    }

    #[test]
    fn combo_keeps_order_and_original_range_accumulator_bounds() {
        let text = format!(
            "[misc]\n[attack00]\ntype=2\nnum_attacks=2\nattack00=8\nattack01=7\n{}{}",
            motion(7, 80000, -4),
            motion(8, 90000, -2)
        );
        let attacks = prepare(&CombatIni::parse(&text).unwrap()).unwrap();
        assert_eq!(attacks.len(), 1);
        assert_eq!(
            attacks[0]
                .motions
                .iter()
                .map(|motion| motion.section)
                .collect::<Vec<_>>(),
            [8, 7]
        );
        assert_eq!((attacks[0].min_range, attacks[0].max_range), (65535, -1));
        assert_eq!(attacks[0].motions[0].animation_group, 102);
        assert_eq!(attacks[0].motions[0].arc_center, None);
    }

    #[test]
    fn single_preserves_ranges_and_empty_record_ends_table() {
        let text = format!(
            "[misc]\n{}[attack01]\narc=4\n{}",
            motion(0, 80000, -4),
            motion(2, 1, 2)
        );
        let attacks = prepare(&CombatIni::parse(&text).unwrap()).unwrap();
        assert_eq!(attacks.len(), 1);
        assert_eq!((attacks[0].min_range, attacks[0].max_range), (80000, -4));
    }

    #[test]
    fn absent_required_field_is_not_assigned_zero() {
        let text = format!("[misc]\n{}", motion(0, 1, 2)).replace("animation=5\n", "");
        let error = prepare(&CombatIni::parse(&text).unwrap()).unwrap_err();
        assert!(format!("{error:#}").contains("attack00: animation absent"));
    }
    #[test]
    fn invalid_combo_references_report_the_owning_section() {
        for (fields, expected) in [
            ("num_attacks=4", "num_attacks 4 exceeds"),
            ("num_attacks=1", "attack00: attack00 absent"),
            (
                "num_attacks=1\nattack00=9",
                "attack00: referenced attack09 absent",
            ),
        ] {
            let text = format!("[misc]\n[attack00]\ntype=2\n{fields}\n");
            let error = prepare(&CombatIni::parse(&text).unwrap()).unwrap_err();
            assert!(format!("{error:#}").contains(expected), "{error:#}");
        }
    }
}
