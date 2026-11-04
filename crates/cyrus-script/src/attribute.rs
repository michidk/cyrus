//! Actor attribute slots: the 256 bytes every SOUP actor carries.
//!
//! Slot positions are fixed by the engine. Variant names are the installed `SOUP386.DEF`
//! attribute equates without their `attr_` prefix (preservation `docs/engine/SOUP.md`).
//! Rust code addresses slots only through [`Attribute`]; numeric access is limited to
//! script calls such as `GetAttribute`, whose index is script data.
use serde::{Deserialize, Serialize};
use std::ops::{Index, IndexMut};

/// A named actor attribute slot; the discriminant is the slot index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Attribute {
    Active = 0,
    Collide = 1,
    Strength = 2,
    Health = 3,
    Armor = 4,
    Movement = 5,
    Weight = 6,
    Ymove = 7,
    ForwardSpeed = 8,
    BackwardSpeed = 9,
    SideSpeed = 10,
    StepType = 11,
    ForwardWalk = 12,
    BackwardWalk = 13,
    Combat = 14,
    TurnSpeed = 15,
    FightLungeSpeed = 16,
    FightForwardSpeed = 17,
    FightBackwardSpeed = 18,
    FightForwardWalk = 19,
    FightBackwardWalk = 20,
    FightTurnSpeed = 21,
    FightTurnAccel = 22,
    Weapon = 23,
    PlayerAlign = 24,
    Master = 25,
    Slave = 26,
    SlaveType = 27,
    Rotation = 28,
    Group = 29,
    User = 30,
    JumpTime = 31,
    JumpHeight = 32,
    NoSlaveRot = 33,
    SheathFrame = 34,
    DrawFrame = 35,
    IgnoreOwnMatrix = 36,
    StopYRotate = 37,
    StopXRotate = 38,
    StopZRotate = 39,
    StopXMove = 40,
    StopYMove = 41,
    StopZMove = 42,
    AlwaysSync = 43,
    PanicGroup = 44,
    CameraMove = 45,
    CameraPanType = 46,
    CameraTargetOffsetX = 47,
    CameraTargetOffsetY = 48,
    CameraTargetOffsetZ = 49,
    CameraOffsetX = 50,
    CameraOffsetY = 51,
    CameraOffsetZ = 52,
    CameraDistance = 53,
    DialogY = 54,
    NoCollide = 55,
    NoSphereObject = 56,
    Mass = 57,
    Aggression = 58,
    CombatDefense = 59,
    Attack = 60,
    Defend = 61,
    AttackSkill = 62,
    DefenseSkill = 63,
    CombatAllies = 64,
    CombatAlignment = 65,
    CombatGroup = 66,
    WeaponType = 67,
    ArmorType = 68,
    DeathType = 69,
    AttackSingle = 70,
    AttackDouble = 71,
    AttackTriple = 72,
    AttackThrust = 73,
    AttackLunge = 74,
    AttackLow = 75,
    AttackJumpUp = 76,
    AttackJumpForward = 77,
    AttackDisarm = 78,
    DefendLow = 79,
    DefendRight = 80,
    DefendLeft = 81,
    DefendHigh = 82,
    FightCircleSpeed = 83,
    FightCircleDeltaAngle = 84,
    NumTaunts = 85,
    HitSound = 86,
    Chatty = 87,
    Twitch = 88,
    StickToSurface = 89,
    NodeIdle = 90,
    NodeClimbs = 91,
    NodeJumps = 92,
    NodeRun = 93,
    NodeInUse = 94,
    NodeIgnoreInSight = 95,
    Damage = 96,
    Npc = 97,
    BouncePower = 98,
    BounceMove = 99,
    BounceTurn = 100,
    BoatRockAlign = 101,
    VWeapon = 102,
    LightFrame = 103,
    UnlightFrame = 104,
    PlayerType = 105,
    SpecialActivate = 106,
    FallHold = 107,
    FallDamage = 108,
    ZapAnim = 109,
    HoldingWeapon = 110,
    ReloadType = 111,
    NoContinue = 112,
    ShortMapping = 113,
    NoViewManage = 114,
    ReservedWeight = 115,
    NoNodeCombat = 116,
    LockPos = 117,
    Woman = 118,
    ReservedCollide = 119,
    Invulnerable = 120,
    CombatIgnoreInSight = 121,
    NonMoving = 122,
    CombatLongSight = 123,
    CombatRadiusWeapon = 124,
    MessGroup = 125,
    CombatRadiusVhit = 126,
}

impl Attribute {
    /// The slot index in [`Attributes`].
    pub const fn index(self) -> usize {
        self as usize
    }
}

/// One actor's attribute bytes, indexed by [`Attribute`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<u8>", into = "Vec<u8>")]
pub struct Attributes([u8; Self::LEN]);

impl Attributes {
    /// Slot count, including the unnamed slots after the last named attribute.
    pub const LEN: usize = 256;

    /// Raw slots for script calls that address attributes by number.
    pub fn bytes(&self) -> &[u8; Self::LEN] {
        &self.0
    }

    /// Raw slots for script calls that address attributes by number.
    pub fn bytes_mut(&mut self) -> &mut [u8; Self::LEN] {
        &mut self.0
    }
}

impl Default for Attributes {
    fn default() -> Self {
        Self([0; Self::LEN])
    }
}

impl From<[u8; Attributes::LEN]> for Attributes {
    fn from(bytes: [u8; Attributes::LEN]) -> Self {
        Self(bytes)
    }
}

impl TryFrom<Vec<u8>> for Attributes {
    type Error = String;

    fn try_from(bytes: Vec<u8>) -> Result<Self, Self::Error> {
        let len = bytes.len();
        bytes.try_into().map(Self).map_err(|_| {
            format!(
                "actor attributes: expected {} bytes, found {len}",
                Self::LEN
            )
        })
    }
}

impl From<Attributes> for Vec<u8> {
    fn from(attributes: Attributes) -> Self {
        attributes.0.to_vec()
    }
}

impl Index<Attribute> for Attributes {
    type Output = u8;

    fn index(&self, attribute: Attribute) -> &u8 {
        &self.0[attribute.index()]
    }
}

impl IndexMut<Attribute> for Attributes {
    fn index_mut(&mut self, attribute: Attribute) -> &mut u8 {
        &mut self.0[attribute.index()]
    }
}

impl<const N: usize> From<[(Attribute, u8); N]> for Attributes {
    fn from(values: [(Attribute, u8); N]) -> Self {
        let mut attributes = Self::default();
        attributes.extend(values);
        attributes
    }
}

impl Extend<(Attribute, u8)> for Attributes {
    fn extend<I: IntoIterator<Item = (Attribute, u8)>>(&mut self, values: I) {
        for (attribute, value) in values {
            self[attribute] = value;
        }
    }
}
