//! Scripted choice menu: the items `menuAddItem` collects, the highlight `menuProc` moves with the
//! up and down controls, and the choice `menuSelection` reports. Layout and drawing are host-owned.
use serde::{Deserialize, Serialize};

/// One item record. Clearing a menu empties the text but leaves the other fields, so
/// `menuSelection` keeps reporting the old value until a new item overwrites the slot.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MenuItem {
    pub text: String,
    /// The `menuAddItem` label argument.
    pub label: i32,
    /// The `menuAddItem` second argument's low word; one selects the alternate item font.
    pub flag: i16,
    /// The `menuAddItem` third argument, reported by `menuSelection`.
    pub value: i32,
}

/// Controls and clock sampled for one `menuProc` step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuInput {
    pub up: bool,
    pub down: bool,
    /// The held select control (the fourth action binding).
    pub select: bool,
    /// Raw BIOS timer sample.
    pub tick: u32,
    /// Whether elapsed BIOS ticks shorten the repeat delay: the update advanced a nominal tick.
    pub elapsed_delay: bool,
    /// Installed `menu_traverse_delay` in ticks.
    pub traverse_delay: i32,
}

/// Menu state shared by all scripts; retained across world replacement.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MenuState {
    slots: Vec<MenuItem>,
    count: usize,
    /// Highlighted item.
    current: usize,
    /// Item committed by the last accepted selection.
    committed: usize,
    /// Last repeated control value (zero while neither control is held).
    repeat: u32,
    /// Remaining repeat delay in ticks.
    delay: i32,
    last_tick: u32,
    /// Select was pressed and awaits its release.
    select_pending: bool,
    /// Set when a selection is accepted.
    accepted: bool,
}

impl MenuState {
    /// Items currently in the menu, in addition order.
    pub fn items(&self) -> &[MenuItem] {
        &self.slots[..self.count]
    }

    pub fn highlighted(&self) -> usize {
        self.current
    }

    /// `menuNew`: with items present, empty the menu and highlight the first.
    pub fn clear(&mut self) {
        if self.count != 0 {
            for slot in &mut self.slots[..self.count] {
                slot.text.clear();
            }
            self.count = 0;
            self.current = 0;
        }
    }

    /// `menuAddItem` for an installed text. `remembered` is the text last highlighted: an item
    /// with the same text is highlighted again. Items past `limit` are dropped.
    pub fn add(
        &mut self,
        limit: usize,
        text: &str,
        label: i32,
        flag: i32,
        value: i32,
        remembered: &str,
    ) {
        if self.count >= limit {
            return;
        }
        if self.slots.len() <= self.count {
            self.slots.resize_with(self.count + 1, MenuItem::default);
        }
        if text == remembered {
            self.current = self.count;
        }
        self.slots[self.count] = MenuItem {
            text: text.to_owned(),
            label,
            flag: flag as i16,
            value,
        };
        self.count += 1;
    }

    /// `menuSelection`: the committed item's value; zero before any slot exists.
    pub fn selection(&self) -> i32 {
        self.slots.get(self.committed).map_or(0, |slot| slot.value)
    }

    /// The highlighted item's text, which the host remembers as the last highlight.
    pub fn highlighted_text(&self) -> Option<&str> {
        self.items()
            .get(self.current)
            .map(|item| item.text.as_str())
    }

    /// Whether a selection was accepted since the host last cleared the flag.
    pub fn take_accepted(&mut self) -> bool {
        std::mem::take(&mut self.accepted)
    }

    /// One `menuProc` step after the menu is active: true while the task keeps running.
    /// Select commits on its release; up takes priority over down and repeats after the delay.
    pub fn step(&mut self, input: MenuInput) -> bool {
        if !input.up && !input.down {
            self.repeat = 0;
            self.delay = 0;
        } else if self.delay != 0 && self.last_tick != input.tick {
            if input.elapsed_delay {
                self.delay -= input.tick.wrapping_sub(self.last_tick) as i32;
            }
            self.delay = self.delay.max(0);
        } else {
            self.last_tick = input.tick;
        }
        if input.select {
            self.select_pending = true;
            return true;
        }
        if self.select_pending {
            self.select_pending = false;
            self.accepted = true;
            self.committed = self.current;
            return false;
        }
        let (held, step_back) = match (input.up, input.down) {
            (true, _) => (true, true),
            (false, true) => (true, false),
            _ => (false, false),
        };
        if held {
            if self.repeat != 1 || self.delay == 0 {
                self.last_tick = input.tick;
                self.delay = input.traverse_delay;
                if step_back {
                    self.current = self
                        .current
                        .checked_sub(1)
                        .unwrap_or(self.count.saturating_sub(1));
                } else {
                    self.current += 1;
                    if self.current >= self.count {
                        self.current = 0;
                    }
                }
            }
            self.repeat = 1;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn menu_of(items: &[&str]) -> MenuState {
        let mut menu = MenuState::default();
        for (index, text) in items.iter().enumerate() {
            menu.add(300, text, index as i32, 0, 10 + index as i32, "");
        }
        menu
    }

    fn input(up: bool, down: bool, select: bool, tick: u32) -> MenuInput {
        MenuInput {
            up,
            down,
            select,
            tick,
            elapsed_delay: true,
            traverse_delay: 4,
        }
    }

    #[test]
    fn items_keep_their_value_after_the_menu_is_cleared() {
        let mut menu = menu_of(&["a", "b"]);
        menu.committed = 1;
        assert_eq!(menu.selection(), 11);
        menu.clear();
        assert_eq!((menu.items().len(), menu.selection()), (0, 11));
        menu.add(300, "c", 5, 1, 77, "");
        menu.committed = 0;
        assert_eq!(menu.selection(), 77, "the slot is reused");
    }

    #[test]
    fn the_limit_drops_items_and_remembered_text_is_highlighted_again() {
        let mut menu = MenuState::default();
        for text in ["a", "b", "c"] {
            menu.add(2, text, 0, 0, 0, "b");
        }
        assert_eq!(menu.items().len(), 2);
        assert_eq!(menu.highlighted(), 1);
    }

    #[test]
    fn up_and_down_wrap_repeat_after_the_delay_and_select_commits_on_release() {
        let mut menu = menu_of(&["a", "b", "c"]);
        assert!(menu.step(input(false, true, false, 100)));
        assert_eq!(menu.highlighted(), 1);
        // Held on the next tick: still inside the repeat delay.
        assert!(menu.step(input(false, true, false, 101)));
        assert_eq!(menu.highlighted(), 1);
        // The elapsed ticks have used up the delay, so the held control repeats.
        assert!(menu.step(input(false, true, false, 110)));
        assert!(menu.step(input(false, true, false, 111)));
        assert_eq!(menu.highlighted(), 2);
        assert!(menu.step(input(false, false, false, 112)));
        assert!(menu.step(input(true, false, false, 113)));
        assert_eq!(menu.highlighted(), 1);
        assert!(menu.step(input(false, false, false, 114)));
        assert!(menu.step(input(false, false, true, 115)));
        assert!(
            !menu.step(input(false, false, false, 116)),
            "select commits on release"
        );
        assert_eq!(menu.selection(), 11);
        assert!(menu.take_accepted());
        assert!(!menu.take_accepted());
        let mut wrap = menu_of(&["a", "b"]);
        assert!(wrap.step(input(true, false, false, 1)));
        assert_eq!(
            wrap.highlighted(),
            1,
            "up from the first item wraps to the last"
        );
    }

    /// Original `menuProc` steps from an emulator run over random controls and timer values:
    /// `count traverse_delay nominal_tick initial_highlight|` then per step `up,down,select,tick`
    /// followed by the original's result, highlight, committed item, repeat word, delay, last tick,
    /// select-pending, accepted and menu-active words.
    const ORACLE: &str = "\
5 2 0 2|0,0,0,1495,1,2,0,0,0,0,0,0,1;0,0,0,1500,1,2,0,0,0,0,0,0,1;0,0,0,1505,1,2,0,0,0,0,0,0,1;1,0,0,1505,1,1,0,1,2,1505,0,0,1;0,0,0,1506,1,1,0,0,0,1505,0,0,1;0,0,0,1506,1,1,0,0,0,1505,0,0,1;0,1,0,1506,1,2,0,1,2,1506,0,0,1;0,1,0,1511,1,2,0,1,2,1506,0,0,1;1,1,1,1511,1,2,0,1,2,1506,1,0,1;0,0,0,1512,0,2,2,0,0,1506,0,1,0
2 1 0 1|0,0,1,1893,1,1,0,0,0,0,1,0,1;1,0,1,1894,1,1,0,0,0,1894,1,0,1;1,1,1,1899,1,1,0,0,0,1899,1,0,1;0,0,0,1901,0,1,1,0,0,1899,0,1,0
5 6 1 3|0,0,0,1071,1,3,0,0,0,0,0,0,1;0,1,0,1076,1,4,0,1,6,1076,0,0,1;0,1,0,1081,1,4,0,1,1,1076,0,0,1;0,0,0,1082,1,4,0,0,0,1076,0,0,1;1,1,0,1087,1,3,0,1,6,1087,0,0,1;1,1,0,1088,1,3,0,1,5,1087,0,0,1
3 6 1 1|1,0,1,1021,1,1,0,0,0,1021,1,0,1;0,1,0,1023,0,1,1,0,0,1023,0,1,0
2 4 1 1|0,0,0,1955,1,1,0,0,0,0,0,0,1;0,1,0,1960,1,0,0,1,4,1960,0,0,1;1,0,0,1962,1,0,0,1,2,1960,0,0,1;1,1,0,1962,1,1,0,1,4,1962,0,0,1;0,1,0,1962,1,1,0,1,4,1962,0,0,1;0,0,0,1962,1,1,0,0,0,1962,0,0,1;0,1,1,1962,1,1,0,0,0,1962,1,0,1;0,1,1,1962,1,1,0,0,0,1962,1,0,1;0,1,0,1967,0,1,1,0,0,1967,0,1,0
1 4 1 0|0,1,1,1823,1,0,0,0,0,1823,1,0,1;0,1,1,1824,1,0,0,0,0,1824,1,0,1;0,1,1,1825,1,0,0,0,0,1825,1,0,1;1,0,0,1826,0,0,0,0,0,1826,0,1,0
2 1 0 0|1,0,1,1265,1,0,0,0,0,1265,1,0,1;0,0,0,1265,0,0,0,0,0,1265,0,1,0
1 1 0 0|1,0,1,1154,1,0,0,0,0,1154,1,0,1;0,1,0,1155,0,0,0,0,0,1155,0,1,0
5 4 1 1|1,0,1,1780,1,1,0,0,0,1780,1,0,1;1,0,0,1781,0,1,1,0,0,1781,0,1,0
3 2 1 0|0,1,1,1873,1,0,0,0,0,1873,1,0,1;0,0,0,1874,0,0,0,0,0,1873,0,1,0
2 2 1 1|0,1,0,1290,1,0,0,1,2,1290,0,0,1;0,0,0,1291,1,0,0,0,0,1290,0,0,1;0,0,0,1292,1,0,0,0,0,1290,0,0,1;1,1,1,1292,1,0,0,0,0,1292,1,0,1;1,1,0,1293,0,0,0,0,0,1293,0,1,0
2 2 0 0|0,1,0,1971,1,1,0,1,2,1971,0,0,1;1,0,0,1973,1,1,0,1,2,1971,0,0,1;0,0,0,1974,1,1,0,0,0,1971,0,0,1;0,0,0,1976,1,1,0,0,0,1971,0,0,1;0,0,0,1981,1,1,0,0,0,1971,0,0,1;1,1,0,1982,1,0,0,1,2,1982,0,0,1;1,0,0,1983,1,0,0,1,2,1982,0,0,1;0,1,0,1983,1,0,0,1,2,1982,0,0,1;1,0,0,1984,1,0,0,1,2,1982,0,0,1;0,1,1,1985,1,0,0,1,2,1982,1,0,1;1,0,0,1990,0,0,0,1,2,1982,0,1,0
5 4 1 2|0,1,0,1029,1,3,0,1,4,1029,0,0,1;1,0,1,1030,1,3,0,1,3,1029,1,0,1;0,1,1,1032,1,3,0,1,0,1029,1,0,1;0,0,0,1032,0,3,3,0,0,1029,0,1,0
5 1 1 0|0,1,0,1266,1,1,0,1,1,1266,0,0,1;0,0,1,1267,1,1,0,0,0,1266,1,0,1;1,1,0,1268,0,1,1,0,0,1268,0,1,0
3 4 1 1|0,1,0,1333,1,2,0,1,4,1333,0,0,1;0,0,0,1333,1,2,0,0,0,1333,0,0,1;0,0,0,1334,1,2,0,0,0,1333,0,0,1;1,0,0,1336,1,1,0,1,4,1336,0,0,1;1,0,1,1341,1,1,0,1,0,1336,1,0,1;0,1,0,1343,0,1,1,1,0,1343,0,1,0
2 2 1 1|1,0,0,1989,1,0,0,1,2,1989,0,0,1;1,0,1,1991,1,0,0,1,0,1989,1,0,1;1,1,1,1991,1,0,0,1,0,1991,1,0,1;0,0,0,1996,0,0,0,0,0,1991,0,1,0
4 2 0 3|0,0,0,1389,1,3,0,0,0,0,0,0,1;0,0,0,1390,1,3,0,0,0,0,0,0,1;0,0,0,1391,1,3,0,0,0,0,0,0,1;0,0,0,1391,1,3,0,0,0,0,0,0,1;1,1,1,1393,1,3,0,0,0,1393,1,0,1;0,0,0,1393,0,3,3,0,0,1393,0,1,0
1 1 0 0|0,0,0,1560,1,0,0,0,0,0,0,0,1;1,1,0,1560,1,0,0,1,1,1560,0,0,1;1,0,0,1560,1,0,0,1,1,1560,0,0,1;0,1,1,1561,1,0,0,1,1,1560,1,0,1;0,0,0,1563,0,0,0,0,0,1560,0,1,0
5 1 0 3|1,0,0,1562,1,2,0,1,1,1562,0,0,1;1,1,0,1562,1,2,0,1,1,1562,0,0,1;1,0,0,1563,1,2,0,1,1,1562,0,0,1;1,0,1,1564,1,2,0,1,1,1562,1,0,1;0,0,0,1565,0,2,2,0,0,1562,0,1,0
4 1 0 3|0,0,0,1830,1,3,0,0,0,0,0,0,1;1,0,1,1835,1,3,0,0,0,1835,1,0,1;0,0,0,1836,0,3,3,0,0,1835,0,1,0
3 4 0 1|0,1,1,1320,1,1,0,0,0,1320,1,0,1;0,0,0,1322,0,1,1,0,0,1320,0,1,0
2 2 0 0|0,0,0,1555,1,0,0,0,0,0,0,0,1;0,0,0,1557,1,0,0,0,0,0,0,0,1;0,0,0,1557,1,0,0,0,0,0,0,0,1;0,0,0,1558,1,0,0,0,0,0,0,0,1;0,1,0,1559,1,1,0,1,2,1559,0,0,1;1,1,0,1561,1,1,0,1,2,1559,0,0,1;0,0,1,1561,1,1,0,0,0,1559,1,0,1;0,1,0,1561,0,1,1,0,0,1561,0,1,0
5 1 1 2|0,1,0,1722,1,3,0,1,1,1722,0,0,1;1,0,0,1722,1,3,0,1,1,1722,0,0,1;1,0,1,1727,1,3,0,1,0,1722,1,0,1;0,0,1,1728,1,3,0,0,0,1722,1,0,1;0,0,1,1730,1,3,0,0,0,1722,1,0,1;1,1,1,1735,1,3,0,0,0,1735,1,0,1;0,0,1,1736,1,3,0,0,0,1735,1,0,1;1,1,0,1741,0,3,3,0,0,1741,0,1,0
1 1 0 0|0,0,0,1833,1,0,0,0,0,0,0,0,1;1,1,0,1834,1,0,0,1,1,1834,0,0,1;1,0,0,1839,1,0,0,1,1,1834,0,0,1;0,0,0,1844,1,0,0,0,0,1834,0,0,1;0,0,1,1849,1,0,0,0,0,1834,1,0,1;0,0,0,1851,0,0,0,0,0,1834,0,1,0
4 2 0 2|1,0,0,1560,1,1,0,1,2,1560,0,0,1;1,1,0,1561,1,1,0,1,2,1560,0,0,1;1,0,0,1562,1,1,0,1,2,1560,0,0,1;1,1,0,1563,1,1,0,1,2,1560,0,0,1;0,1,1,1568,1,1,0,1,2,1560,1,0,1;0,0,0,1569,0,1,1,0,0,1560,0,1,0
3 1 1 2|1,0,0,1405,1,1,0,1,1,1405,0,0,1;0,0,1,1410,1,1,0,0,0,1405,1,0,1;0,0,0,1410,0,1,1,0,0,1405,0,1,0
4 2 0 3|0,0,0,1968,1,3,0,0,0,0,0,0,1;0,1,0,1968,1,0,0,1,2,1968,0,0,1;0,0,0,1969,1,0,0,0,0,1968,0,0,1;0,1,0,1971,1,1,0,1,2,1971,0,0,1;0,0,1,1972,1,1,0,0,0,1971,1,0,1;1,0,1,1972,1,1,0,0,0,1972,1,0,1;1,1,0,1972,0,1,1,0,0,1972,0,1,0
4 1 1 0|0,0,0,1191,1,0,0,0,0,0,0,0,1;0,0,0,1191,1,0,0,0,0,0,0,0,1;0,0,0,1192,1,0,0,0,0,0,0,0,1;1,1,0,1193,1,3,0,1,1,1193,0,0,1;0,1,1,1193,1,3,0,1,1,1193,1,0,1;0,0,0,1193,0,3,3,0,0,1193,0,1,0
4 4 0 0|0,1,0,1879,1,1,0,1,4,1879,0,0,1;0,0,1,1880,1,1,0,0,0,1879,1,0,1;0,0,0,1881,0,1,1,0,0,1879,0,1,0
3 1 0 0|1,0,0,1996,1,2,0,1,1,1996,0,0,1;1,0,0,1996,1,2,0,1,1,1996,0,0,1;0,1,0,2001,1,2,0,1,1,1996,0,0,1;0,1,0,2003,1,2,0,1,1,1996,0,0,1;0,0,1,2004,1,2,0,0,0,1996,1,0,1;1,0,0,2005,0,2,2,0,0,2005,0,1,0
1 1 1 0|0,0,0,1736,1,0,0,0,0,0,0,0,1;0,0,1,1737,1,0,0,0,0,0,1,0,1;1,1,1,1737,1,0,0,0,0,1737,1,0,1;0,0,0,1737,0,0,0,0,0,1737,0,1,0
3 6 1 0|0,1,1,1217,1,0,0,0,0,1217,1,0,1;0,0,0,1217,0,0,0,0,0,1217,0,1,0
2 4 1 0|0,0,0,1104,1,0,0,0,0,0,0,0,1;0,1,1,1109,1,0,0,0,0,1109,1,0,1;0,0,0,1114,0,0,0,0,0,1109,0,1,0
3 6 1 2|0,0,0,1995,1,2,0,0,0,0,0,0,1;0,1,0,2000,1,0,0,1,6,2000,0,0,1;1,0,1,2002,1,0,0,1,4,2000,1,0,1;1,0,0,2003,0,0,0,1,1,2000,0,1,0
5 1 1 4|0,0,1,1947,1,4,0,0,0,0,1,0,1;0,0,0,1947,0,4,4,0,0,0,0,1,0
2 4 1 1|0,0,1,1475,1,1,0,0,0,0,1,0,1;0,1,1,1480,1,1,0,0,0,1480,1,0,1;1,1,0,1481,0,1,1,0,0,1481,0,1,0
5 4 0 2|1,1,0,1263,1,1,0,1,4,1263,0,0,1;0,0,0,1268,1,1,0,0,0,1263,0,0,1;0,1,0,1273,1,2,0,1,4,1273,0,0,1;0,0,0,1274,1,2,0,0,0,1273,0,0,1;0,0,1,1274,1,2,0,0,0,1273,1,0,1;0,1,0,1276,0,2,2,0,0,1276,0,1,0
2 4 0 0|1,1,0,1353,1,1,0,1,4,1353,0,0,1;0,1,0,1353,1,1,0,1,4,1353,0,0,1;0,0,0,1354,1,1,0,0,0,1353,0,0,1;1,1,0,1355,1,0,0,1,4,1355,0,0,1;0,1,0,1360,1,0,0,1,4,1355,0,0,1;0,1,0,1361,1,0,0,1,4,1355,0,0,1;1,0,0,1363,1,0,0,1,4,1355,0,0,1;1,1,0,1365,1,0,0,1,4,1355,0,0,1;0,0,0,1366,1,0,0,0,0,1355,0,0,1;0,0,1,1366,1,0,0,0,0,1355,1,0,1;1,1,0,1367,0,0,0,0,0,1367,0,1,0
5 2 1 2|0,0,1,1265,1,2,0,0,0,0,1,0,1;1,0,0,1270,0,2,2,0,0,1270,0,1,0
2 1 1 1|0,1,0,1512,1,0,0,1,1,1512,0,0,1;0,1,0,1512,1,0,0,1,1,1512,0,0,1;0,1,0,1517,1,1,0,1,1,1517,0,0,1;1,1,0,1518,1,0,0,1,1,1518,0,0,1;0,0,0,1518,1,0,0,0,0,1518,0,0,1;0,0,0,1519,1,0,0,0,0,1518,0,0,1;1,1,1,1521,1,0,0,0,0,1521,1,0,1;0,0,1,1523,1,0,0,0,0,1521,1,0,1;1,1,1,1523,1,0,0,0,0,1523,1,0,1;0,0,0,1523,0,0,0,0,0,1523,0,1,0
1 2 0 0|1,0,0,1968,1,0,0,1,2,1968,0,0,1;1,0,0,1968,1,0,0,1,2,1968,0,0,1;0,1,1,1970,1,0,0,1,2,1968,1,0,1;1,1,0,1970,0,0,0,1,2,1968,0,1,0
1 4 1 0|1,0,0,1077,1,0,0,1,4,1077,0,0,1;0,1,1,1077,1,0,0,1,4,1077,1,0,1;1,1,0,1077,0,0,0,1,4,1077,0,1,0
3 2 1 1|1,0,1,1536,1,1,0,0,0,1536,1,0,1;1,0,0,1537,0,1,1,0,0,1537,0,1,0
4 1 0 1|0,0,0,1026,1,1,0,0,0,0,0,0,1;1,1,0,1027,1,0,0,1,1,1027,0,0,1;1,0,0,1032,1,0,0,1,1,1027,0,0,1;0,0,0,1033,1,0,0,0,0,1027,0,0,1;0,0,0,1033,1,0,0,0,0,1027,0,0,1;1,0,1,1038,1,0,0,0,0,1038,1,0,1;0,0,0,1039,0,0,0,0,0,1038,0,1,0
3 6 1 0|0,0,1,1337,1,0,0,0,0,0,1,0,1;1,0,1,1337,1,0,0,0,0,1337,1,0,1;1,0,0,1337,0,0,0,0,0,1337,0,1,0";

    #[test]
    fn menu_steps_match_the_original() {
        let mut steps = 0;
        for case in ORACLE.lines() {
            let (head, rest) = case.split_once('|').unwrap();
            let head: Vec<i64> = head.split(' ').map(|v| v.parse().unwrap()).collect();
            let mut menu = menu_of(&vec!["x"; head[0] as usize]);
            menu.current = head[3] as usize;
            let mut accepted = false;
            for step in rest.split(';') {
                let v: Vec<i64> = step.split(',').map(|v| v.parse().unwrap()).collect();
                let running = menu.step(MenuInput {
                    up: v[0] != 0,
                    down: v[1] != 0,
                    select: v[2] != 0,
                    tick: v[3] as u32,
                    elapsed_delay: head[2] != 0,
                    traverse_delay: head[1] as i32,
                });
                accepted |= menu.accepted;
                menu.accepted = false;
                let actual = [
                    i64::from(running),
                    menu.current as i64,
                    menu.committed as i64,
                    i64::from(menu.repeat),
                    i64::from(menu.delay),
                    i64::from(menu.last_tick),
                    i64::from(menu.select_pending),
                    i64::from(accepted),
                    // The menu-active word is set while the task runs and cleared by a commit.
                    i64::from(running),
                ];
                assert_eq!(actual, v[4..], "{case}: {step}");
                steps += 1;
            }
        }
        assert!(steps > 150, "{steps} steps");
    }
}
