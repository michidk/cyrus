//! Retained actor flat records follow the original's per-actor flat sprite controls.
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Installed frame counts by `TEXBSI` bank then image index.
pub(super) type TextureFrames = BTreeMap<u16, BTreeMap<u16, u16>>;

/// Flat sprite control record, created zeroed by the first flat native on an actor.
/// Word fields keep the original's sixteen-bit stores.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct FlatState {
    pub enabled: bool,
    /// Ticks the record has been disabled (byte-wrapping); cleared by every flat native.
    pub pending: u8,
    pub offset: [u16; 3],
    /// Texture identifier: bank in the upper nine bits, image in the low seven.
    pub texture: u16,
    /// Bank and image whose frame count sized the playback; kept when the texture changes.
    pub bitmap: Option<(u16, u16)>,
    pub frame: u16,
    pub last_frame: u16,
    pub counter: u16,
    /// `FlatAnimate` second and third arguments, low bytes (record bytes 18 and 19).
    pub animate: [u8; 2],
}

impl FlatState {
    fn show(&mut self, texture: u16, frame: u16) {
        self.enabled = true;
        self.pending = 0;
        self.texture = texture;
        self.animate[1] = 0;
        self.last_frame = 0;
        self.counter = 0;
        self.frame = frame;
    }

    fn frames(&self, table: &TextureFrames) -> Result<u16> {
        let (bank, image) = self.bitmap.context("flat bitmap unavailable")?;
        table
            .get(&bank)
            .and_then(|images| images.get(&image))
            .copied()
            .with_context(|| format!("texture bank {bank} image {image} not installed"))
    }

    /// One actor update of the retained record, in `ticks` original ticks. Returns false when the
    /// record has been disabled for more than 32 ticks and is released.
    fn maintain(&mut self, ticks: u32) -> bool {
        if self.texture == 0 || self.texture == 0xffff {
            self.enabled = false;
        }
        if !self.enabled {
            self.pending = self.pending.wrapping_add(ticks as u8);
            return self.pending <= 0x20;
        }
        if self.animate[1] & 0x80 != 0 {
            self.animate[1] &= 0x7f;
            self.frame = self.counter;
        }
        // A frame word of 0xffff shows the static image; equal bounds do not animate.
        if self.frame == 0xffff || self.counter == self.last_frame {
            return true;
        }
        let step = (ticks as u16).wrapping_mul(u16::from(self.animate[0]));
        let forward = (self.counter as i16) < (self.last_frame as i16);
        self.frame = if forward {
            self.frame.wrapping_add(step)
        } else {
            self.frame.wrapping_sub(step)
        };
        let (frame, last) = (self.frame as i16, self.last_frame as i16);
        if if forward { last < frame } else { frame < last } {
            if self.animate[1] == 0 {
                self.enabled = false;
                self.pending = 0;
            } else {
                self.animate[1] |= 0x80;
                self.frame = self
                    .counter
                    .wrapping_add(self.frame.wrapping_sub(self.last_frame));
                let frame = self.frame as i16;
                if if forward { last < frame } else { frame < last } {
                    self.frame = self.counter;
                }
            }
        }
        true
    }

    /// Select the bitmap for a texture identifier unless the record already shows it.
    fn select(&mut self, key: i32, bank: u16, image: u16, table: &TextureFrames) -> Result<()> {
        if i32::from(self.texture as i16) != key || self.bitmap.is_none() {
            ensure!(
                table
                    .get(&bank)
                    .is_some_and(|images| images.contains_key(&image)),
                "texture bank {bank} image {image} not installed"
            );
            self.bitmap = Some((bank, image));
        }
        Ok(())
    }
}

/// Run a flat native on the receiver's record. `FlatOff` leaves an actor without a record alone.
pub(super) fn call(
    flat: &mut Option<FlatState>,
    function: &str,
    values: &[i32],
    table: &TextureFrames,
) -> Result<()> {
    let name = function.to_ascii_lowercase();
    let expected = match name.as_str() {
        "flatoff" | "flatstop" => 0,
        "flat" | "flatlikestatic" => 1,
        "flatsettexture" => 2,
        _ => 3,
    };
    ensure!(values.len() == expected, "{function}: wrong argument count");
    if name == "flatoff" {
        if let Some(flat) = flat {
            flat.enabled = false;
            flat.pending = 0;
        }
        return Ok(());
    }
    let flat = flat.get_or_insert_with(FlatState::default);
    match name.as_str() {
        "flat" => flat.show(values[0] as u16, 0),
        "flatlikestatic" => flat.show(values[0] as u16, 0xffff),
        "flatoffset" => {
            flat.pending = 0;
            flat.offset = [values[0] as u16, values[1] as u16, values[2] as u16];
        }
        "flatstop" => {
            flat.last_frame = flat.frame;
            flat.counter = flat.last_frame;
            flat.enabled = true;
            flat.pending = 0;
            flat.animate[1] = 0;
        }
        "flatanimate" => {
            let id = values[0];
            flat.select(id, ((id >> 7) & 0x1ff) as u16, (id & 0x7f) as u16, table)
                .with_context(|| format!("{function}: texture {id}"))?;
            flat.last_frame = flat.frames(table)?.wrapping_sub(1);
            flat.texture = id as u16;
            flat.counter = 0;
            flat.frame = 0;
            flat.enabled = true;
            flat.pending = 0;
            flat.animate = [values[1] as u8, values[2] as u8];
        }
        "flatsettexture" => {
            // The original loads image `n & 7` on a cache miss but looks up `n`; which one
            // applies then depends on its global bitmap cache, so wider images stay unsupported.
            ensure!(
                (0..8).contains(&values[1]),
                "{function}: image index above seven unsupported"
            );
            let key = ((values[0] << 7) & 0xff80) | (values[1] & 7);
            flat.select(key, values[0] as u16, values[1] as u16, table)
                .with_context(|| format!("{function}: texture {}.{}", values[0], values[1]))?;
            flat.texture = key as u16;
            flat.last_frame = (flat.frames(table)?.wrapping_sub(1) as i16).max(0) as u16;
            flat.counter = 0;
            flat.frame = 0;
            flat.enabled = true;
            flat.pending = 0;
            flat.animate[1] = 0;
        }
        _ => bail!("{function}: not a flat native"),
    }
    Ok(())
}

impl super::ScriptActor {
    /// The per-update flat step, run beside light maintenance for an actor that is not hidden.
    pub(super) fn maintain_flat(&mut self, ticks: u32) {
        if self.flat.as_mut().is_some_and(|flat| !flat.maintain(ticks)) {
            self.flat = None;
        }
    }

    /// The actor's retained flat record as inspection JSON, when it has one.
    pub fn flat_state(&self) -> Option<serde_json::Value> {
        self.flat
            .as_ref()
            .map(|flat| serde_json::to_value(flat).expect("flat state serializes"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Installed-image frame counts of banks 1 to 5, images 0 to 7, used for the rows below.
    const FRAMES: &str = "1.0:6 1.1:7 1.2:3 1.3:1 1.4:3 1.5:3 1.6:3 1.7:5 2.0:2 2.1:3 2.2:6 2.3:6 2.4:6 2.5:2 2.6:7 2.7:4 3.0:6 3.1:2 3.2:8 3.3:4 3.4:5 3.5:4 3.6:3 3.7:2 4.0:8 4.1:7 4.2:2 4.3:2 4.4:8 4.5:1 4.6:2 4.7:6 5.0:5 5.1:8 5.2:3 5.3:8 5.4:5 5.5:4 5.6:1 5.7:3";

    /// Call sequences on one fresh actor, run through the original natives in an emulator with the
    /// bitmap lookup, loader and allocator stubbed: `Name args=record` after each call, `-` while
    /// the actor has no record. The record is the original's 20 bytes; bytes 10 and 11 hold the
    /// bitmap handle, compared here only for being set.
    const ORACLE: &str = "\
FlatOffset 281,-258,272=00001901fefe1001000000000000000000000000;FlatSetTexture 5,2=01001901fefe1001820201000000020000000000
FlatStop =0100000000000000000000000000000000000000;Flat 390=0100000000000000860100000000000000000000;FlatSetTexture 2,3=0100000000000000030101000000050000000000;FlatAnimate 134,208,226=010000000000000086000200000002000000d0e2;Flat 15179=01000000000000004b3b0200000000000000d000
FlatLikeStatic 2562=0100000000000000020a0000ffff000000000000
FlatStop =0100000000000000000000000000000000000000;FlatOffset 662,-624,262=0100960290fd0601000000000000000000000000
Flat 259=0100000000000000030100000000000000000000;FlatStop =0100000000000000030100000000000000000000
FlatOff =-;FlatOff =-
FlatOffset 91,-156,-646=00005b0064ff7afd000000000000000000000000;FlatSetTexture 1,2=01005b0064ff7afd820001000000020000000000;FlatAnimate 512,251,35=01005b0064ff7afd00020200000007000000fb23;FlatStop =01005b0064ff7afd00020200000000000000fb00;FlatStop =01005b0064ff7afd00020200000000000000fb00
FlatStop =0100000000000000000000000000000000000000;FlatAnimate 261,170,177=010000000000000005010100000001000000aab1;FlatSetTexture 5,4=010000000000000084020200000004000000aa00;FlatLikeStatic 130=010000000000000082000200ffff00000000aa00;FlatSetTexture 4,5=010000000000000005020300000000000000aa00
FlatAnimate 131,208,83=010000000000000083000100000000000000d053;FlatAnimate 640,177,251=010000000000000080020200000004000000b1fb
FlatOffset -368,450,-559=000090fec201d1fd000000000000000000000000;FlatOffset -583,-111,-594=0000b9fd91ffaefd000000000000000000000000;FlatSetTexture 3,4=0100b9fd91ffaefd840101000000040000000000;Flat 22364=0100b9fd91ffaefd5c5701000000000000000000;Flat 19817=0100b9fd91ffaefd694d01000000000000000000
FlatOff =-
FlatLikeStatic 48333=0100000000000000cdbc0000ffff000000000000
FlatStop =0100000000000000000000000000000000000000;FlatLikeStatic 644=010000000000000084020000ffff000000000000;FlatOffset -159,515,-33=010061ff0302dfff84020000ffff000000000000;FlatAnimate 262,131,207=010061ff0302dfff0601010000000600000083cf
FlatLikeStatic 257=010000000000000001010000ffff000000000000;FlatLikeStatic 135=010000000000000087000000ffff000000000000
FlatAnimate 386,155,275=0100000000000000820101000000070000009b13
FlatOffset 572,-360,-329=00003c0298feb7fe000000000000000000000000;FlatLikeStatic 515=01003c0298feb7fe03020000ffff000000000000;FlatSetTexture 1,6=01003c0298feb7fe860001000000020000000000
FlatSetTexture 1,6=0100000000000000860001000000020000000000;FlatSetTexture 5,2=0100000000000000820202000000020000000000;FlatSetTexture 2,4=0100000000000000040103000000050000000000;FlatAnimate 645,21,125=010000000000000085020400000003000000157d
FlatLikeStatic 65945=010000000000000099010000ffff000000000000;FlatStop =010000000000000099010000ffffffffffff0000;FlatStop =010000000000000099010000ffffffffffff0000;FlatStop =010000000000000099010000ffffffffffff0000
FlatStop =0100000000000000000000000000000000000000;Flat 26387=0100000000000000136700000000000000000000;Flat 390=0100000000000000860100000000000000000000;FlatAnimate 135,258,10=010000000000000087000100000004000000020a;FlatAnimate 519,264,262=0100000000000000070202000000050000000806
FlatOffset -649,-601,654=000077fda7fd8e02000000000000000000000000;FlatOffset -136,507,128=000078fffb018000000000000000000000000000;FlatAnimate 519,121,204=010078fffb0180000702010000000500000079cc;FlatOffset -234,593,640=010016ff510280020702010000000500000079cc
Flat 64472=0100000000000000d8fb00000000000000000000
FlatLikeStatic 32152=0100000000000000987d0000ffff000000000000;FlatOffset 558,82,373=01002e0252007501987d0000ffff000000000000;FlatAnimate 130,160,103=01002e025200750182000100000002000000a067;FlatOff =00002e025200750182000100000002000000a067;Flat 10422=01002e0252007501b6280100000000000000a000
FlatOffset -360,187,-279=000098febb00e9fe000000000000000000000000;FlatAnimate 389,297,96=010098febb00e9fe850101000000030000002960;FlatOff =000098febb00e9fe850101000000030000002960;FlatOff =000098febb00e9fe850101000000030000002960;FlatSetTexture 2,3=010098febb00e9fe030102000000050000002900
FlatStop =0100000000000000000000000000000000000000;FlatLikeStatic 52649=0100000000000000a9cd0000ffff000000000000
FlatStop =0100000000000000000000000000000000000000;FlatAnimate 516,50,96=0100000000000000040201000000070000003260;FlatLikeStatic 133=010000000000000085000100ffff000000003200;Flat 52186=0100000000000000dacb01000000000000003200
FlatLikeStatic 12388=010000000000000064300000ffff000000000000;FlatLikeStatic 392=010000000000000088010000ffff000000000000;FlatLikeStatic 262=010000000000000006010000ffff000000000000;FlatOff =000000000000000006010000ffff000000000000
FlatStop =0100000000000000000000000000000000000000;FlatStop =0100000000000000000000000000000000000000;FlatSetTexture 1,1=0100000000000000810001000000060000000000;Flat 515=0100000000000000030201000000000000000000
FlatSetTexture 5,7=0100000000000000870201000000020000000000;FlatOffset -148,-386,23=01006cff7efe1700870201000000020000000000;FlatOff =00006cff7efe1700870201000000020000000000;FlatSetTexture 1,1=01006cff7efe1700810002000000060000000000
FlatStop =0100000000000000000000000000000000000000;FlatOff =0000000000000000000000000000000000000000
FlatLikeStatic 36077=0100000000000000ed8c0000ffff000000000000
Flat 47001=010000000000000099b700000000000000000000;FlatOffset 374,167,-540=01007601a700e4fd99b700000000000000000000
FlatSetTexture 2,3=0100000000000000030101000000050000000000;FlatStop =0100000000000000030101000000000000000000;FlatStop =0100000000000000030101000000000000000000
FlatAnimate 391,244,82=010000000000000087010100000001000000f452
FlatSetTexture 1,4=0100000000000000840001000000020000000000;FlatAnimate 131,273,57=0100000000000000830002000000000000001139;FlatOff =0000000000000000830002000000000000001139;FlatOffset 20,-345,556=00001400a7fe2c02830002000000000000001139
FlatStop =0100000000000000000000000000000000000000;FlatAnimate 258,274,256=0100000000000000020101000000050000001200;FlatOffset 354,436,-115=01006201b4018dff020101000000050000001200
FlatAnimate 385,122,207=0100000000000000810101000000010000007acf;FlatStop =0100000000000000810101000000000000007a00;FlatStop =0100000000000000810101000000000000007a00;Flat 512=0100000000000000000201000000000000007a00
FlatStop =0100000000000000000000000000000000000000;FlatStop =0100000000000000000000000000000000000000
FlatLikeStatic 519=010000000000000007020000ffff000000000000;FlatAnimate 260,94,220=0100000000000000040101000000050000005edc
FlatLikeStatic 519=010000000000000007020000ffff000000000000;FlatStop =010000000000000007020000ffffffffffff0000;Flat 391=0100000000000000870100000000000000000000;FlatAnimate 258,192,72=010000000000000002010100000005000000c048
Flat 132=0100000000000000840000000000000000000000;FlatOffset 677,-588,-645=0100a502b4fd7bfd840000000000000000000000;FlatOff =0000a502b4fd7bfd840000000000000000000000;FlatOff =0000a502b4fd7bfd840000000000000000000000";

    fn table() -> TextureFrames {
        let mut table = TextureFrames::new();
        for entry in FRAMES.split(' ') {
            let (key, frames) = entry.split_once(':').unwrap();
            let (bank, image) = key.split_once('.').unwrap();
            table
                .entry(bank.parse().unwrap())
                .or_default()
                .insert(image.parse().unwrap(), frames.parse().unwrap());
        }
        table
    }

    fn record(flat: &FlatState) -> Vec<u8> {
        let mut bytes = vec![u8::from(flat.enabled), flat.pending];
        bytes.extend(flat.offset.iter().flat_map(|word| word.to_le_bytes()));
        bytes.extend(flat.texture.to_le_bytes());
        bytes.extend(u16::from(flat.bitmap.is_some()).to_le_bytes());
        bytes.extend(flat.frame.to_le_bytes());
        bytes.extend(flat.last_frame.to_le_bytes());
        bytes.extend(flat.counter.to_le_bytes());
        bytes.extend(flat.animate);
        bytes
    }

    #[test]
    fn flat_natives_match_the_original_record_updates() {
        let table = table();
        let mut steps = 0;
        for case in ORACLE.lines() {
            let mut flat = None;
            for step in case.split(';') {
                let (invocation, expected) = step.split_once('=').unwrap();
                let (name, args) = invocation.split_once(' ').unwrap();
                let values: Vec<i32> = args
                    .split(',')
                    .filter(|value| !value.is_empty())
                    .map(|value| value.parse().unwrap())
                    .collect();
                call(&mut flat, name, &values, &table).unwrap();
                let mut golden: Vec<u8> = if expected == "-" {
                    vec![]
                } else {
                    (0..40)
                        .step_by(2)
                        .map(|at| u8::from_str_radix(&expected[at..at + 2], 16).unwrap())
                        .collect()
                };
                if !golden.is_empty() {
                    golden[10] = u8::from(golden[10] != 0);
                    golden[11] = 0;
                }
                let actual = flat.as_ref().map(record).unwrap_or_default();
                assert_eq!(actual, golden, "{case}: {step}");
                steps += 1;
            }
        }
        assert!(steps > 100, "{steps} steps");
    }

    fn animating(speed: u8, looping: u8) -> FlatState {
        FlatState {
            enabled: true,
            texture: 0x0a02,
            last_frame: 2,
            animate: [speed, looping],
            ..Default::default()
        }
    }

    #[test]
    fn a_looping_flat_wraps_to_its_start_and_a_one_shot_flat_disables() {
        let mut looping = animating(1, 1);
        looping.maintain(2);
        assert_eq!(looping.frame, 2);
        looping.maintain(2);
        // Past the last frame: the wrap marker is set and the overshoot (4 - 2) restarts from the counter.
        assert_eq!((looping.frame, looping.animate[1]), (2, 0x81));
        looping.maintain(1);
        assert_eq!((looping.frame, looping.animate[1]), (1, 0x01));
        let mut once = animating(1, 0);
        once.maintain(2);
        assert!(once.enabled);
        once.maintain(2);
        assert!(!once.enabled);
        assert_eq!(once.pending, 0);
    }

    #[test]
    fn static_and_disabled_flats_do_not_advance_and_a_stale_record_is_released() {
        let mut still = animating(5, 1);
        still.frame = 0xffff;
        still.maintain(9);
        assert_eq!(still.frame, 0xffff);
        let mut stopped = animating(5, 1);
        stopped.counter = stopped.last_frame;
        stopped.maintain(9);
        assert_eq!(stopped.frame, 0);
        let mut off = FlatState {
            texture: 7,
            ..Default::default()
        };
        assert!(off.maintain(32));
        assert!(!off.maintain(1), "released once the counter exceeds 32");
        let mut unset = animating(1, 1);
        unset.texture = 0;
        unset.maintain(1);
        assert!(!unset.enabled);
    }

    #[test]
    fn missing_banks_and_wide_images_are_contextual_errors() {
        let mut flat = None;
        let error = call(&mut flat, "FlatAnimate", &[(9 << 7) | 1, 0, 0], &table()).unwrap_err();
        assert!(format!("{error:#}").contains("bank 9 image 1 not installed"));
        let error = call(&mut flat, "FlatSetTexture", &[1, 8], &table()).unwrap_err();
        assert!(format!("{error:#}").contains("image index above seven"));
    }
}
