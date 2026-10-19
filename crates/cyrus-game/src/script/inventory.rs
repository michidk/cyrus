//! Session item ownership and selection from installed inventory definitions.
use anyhow::{Context, Result};
use cyrus_assets::hud::{HudAsset, InventoryItem};
use std::collections::BTreeMap;

/// Current item presentation. Ownership includes hidden items; cycling excludes them.
pub struct InventoryView {
    pub selected: Option<u16>,
    pub counts: BTreeMap<u16, u32>,
}

/// Complete inventory continuation, separate from the original SVIT file codec.
/// Installed definitions are retained to reject restoration against different content.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct InventorySnapshot {
    version: u32,
    definitions: BTreeMap<u16, InventoryItem>,
    items: BTreeMap<u16, SavedItem>,
    selected: Option<u16>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct SavedItem {
    count: u32,
    total: u32,
    player_max: u32,
    hidden: bool,
}

/// Opaque replacement inventory for whole-session candidate assembly.
/// Construction from a snapshot never runs startup, acquisition or weapon callbacks.
pub struct Inventory {
    definitions: BTreeMap<u16, InventoryItem>,
    items: BTreeMap<u16, InventoryItem>,
    selected: Option<u16>,
}
impl Inventory {
    /// Capture every item, including hidden, empty and final installed records.
    pub fn snapshot(&self) -> InventorySnapshot {
        InventorySnapshot {
            version: 1,
            definitions: self.definitions.clone(),
            items: self
                .items
                .iter()
                .map(|(&id, item)| {
                    (
                        id,
                        SavedItem {
                            count: item.count,
                            total: item.total,
                            player_max: item.player_max,
                            hidden: item.hidden,
                        },
                    )
                })
                .collect(),
            selected: self.selected,
        }
    }

    /// Validate exact installed acquisition definitions and return an off-live replacement.
    /// Mutable limits need not match installed limits or contain the saved count.
    /// Selection may name an empty or hidden item and is not repaired during restoration.
    pub fn from_snapshot(hud: &HudAsset, snapshot: InventorySnapshot) -> Result<Self> {
        anyhow::ensure!(
            snapshot.version == 1,
            "inventory snapshot version {} is unsupported",
            snapshot.version
        );
        anyhow::ensure!(
            snapshot.definitions.keys().eq(hud.inventory.keys())
                && snapshot.items.keys().eq(hud.inventory.keys()),
            "inventory snapshot item census differs from ITEM.INI"
        );
        for (&id, installed) in &hud.inventory {
            let saved = &snapshot.definitions[&id];
            anyhow::ensure!(
                saved.weapon == installed.weapon
                    && saved.count == installed.count
                    && saved.total == installed.total
                    && saved.player_max == installed.player_max
                    && saved.hidden == installed.hidden
                    && saved.flags == installed.flags
                    && saved.required_items == installed.required_items
                    && saved.drop_add_items == installed.drop_add_items
                    && saved.add_items == installed.add_items
                    && saved.remove_drop_items == installed.remove_drop_items,
                "inventory snapshot item {id}: installed definition differs"
            );
        }
        if let Some(id) = snapshot.selected {
            anyhow::ensure!(
                hud.inventory.contains_key(&id),
                "inventory snapshot selected item {id}: definition absent"
            );
        }
        let mut items = hud.inventory.clone();
        for (id, saved) in snapshot.items {
            let item = items.get_mut(&id).expect("validated inventory census");
            item.count = saved.count;
            item.total = saved.total;
            item.player_max = saved.player_max;
            item.hidden = saved.hidden;
        }
        Ok(Self {
            definitions: hud.inventory.clone(),
            items,
            selected: snapshot.selected,
        })
    }

    pub(super) fn new(hud: &HudAsset) -> Result<Self> {
        let mut inventory = Self {
            definitions: hud.inventory.clone(),
            items: hud.inventory.clone(),
            selected: hud.inventory.keys().next().copied(),
        };
        for &id in &hud.start_items {
            inventory
                .add(id, 1, &mut |_| {
                    anyhow::bail!("startup acquisition has no initial item record")
                })
                .with_context(|| format!("ITEM.INI start_item_list item {id}"))?;
        }
        if let Some(id) = hud.selected {
            inventory.select(id).context("ITEM.INI start_item_select")?;
        }
        Ok(inventory)
    }
    pub(super) fn selected(&self) -> Option<u16> {
        self.selected
    }

    pub(super) fn inspect(&self) -> serde_json::Value {
        serde_json::json!({
            "selected": self.selected,
            "counts": self.items.iter().map(|(&id, item)| (id, item.count)).collect::<BTreeMap<_, _>>(),
        })
    }
    pub(super) fn count(&self, id: u16) -> Result<u32> {
        Ok(self
            .items
            .get(&id)
            .with_context(|| format!("ITEM.INI item {id}: definition absent"))?
            .count)
    }
    pub(super) fn add(
        &mut self,
        id: u16,
        amount: i32,
        assign_weapon: &mut dyn FnMut(u8) -> Result<()>,
    ) -> Result<()> {
        self.acquire(id, amount, &mut Vec::new(), assign_weapon)
    }

    pub(super) fn validate_weapon(&self, id: u8) -> Result<()> {
        let item = self
            .items
            .get(&u16::from(id))
            .with_context(|| format!("ITEM.INI item {id}: weapon definition absent"))?;
        anyhow::ensure!(
            item.weapon,
            "ITEM.INI item {id}: assigned item is not a weapon"
        );
        Ok(())
    }

    pub(super) fn owned_weapon(&self, id: u8) -> bool {
        self.items
            .get(&u16::from(id))
            .is_some_and(|item| item.weapon && item.count > 0)
    }

    fn repair_empty_selection(
        &mut self,
        assign_weapon: &mut dyn FnMut(u8) -> Result<()>,
    ) -> Result<()> {
        if self.selected.is_some() {
            return Ok(());
        }
        let candidate = self.items.contains_key(&1).then_some(1).or_else(|| {
            self.items
                .iter()
                .find(|(_, item)| item.count != 0 && item.weapon)
                .or_else(|| self.items.iter().find(|(_, item)| item.count != 0))
                .map(|(&id, _)| id)
        });
        if let Some(id) = candidate {
            assign_weapon(id as u8)?;
            self.selected = Some(id);
        }
        Ok(())
    }

    fn requirements(&self, id: u16, path: &mut Vec<u16>) -> Result<bool> {
        anyhow::ensure!(
            !path.contains(&id),
            "ITEM.INI required_item_list cycle at item {id}: {path:?}"
        );
        path.push(id);
        let item = self
            .items
            .get(&id)
            .with_context(|| format!("ITEM.INI required item {id}: definition absent"))?;
        for &required in &item.required_items {
            if self.count(required)? == 0 || !self.requirements(required, path)? {
                path.pop();
                return Ok(false);
            }
        }
        path.pop();
        Ok(true)
    }

    fn acquire(
        &mut self,
        id: u16,
        amount: i32,
        path: &mut Vec<u16>,
        assign_weapon: &mut dyn FnMut(u8) -> Result<()>,
    ) -> Result<()> {
        let item = self
            .items
            .get(&id)
            .with_context(|| format!("AddItem({id}): ITEM.INI definition absent"))?;
        let admitted = u32::try_from(amount)
            .unwrap_or(0)
            .min(item.total.saturating_sub(item.count))
            .min(item.player_max.saturating_sub(item.count));
        if admitted == 0 || !self.requirements(id, &mut Vec::new())? {
            return Ok(());
        }
        anyhow::ensure!(
            !path.contains(&id),
            "ITEM.INI add_item_list cycle at item {id}: {path:?}"
        );
        let drops = item.drop_add_items.clone();
        let adds = item.add_items.clone();
        path.push(id);
        for other in drops {
            self.remove(other, 1)
                .with_context(|| format!("ITEM.INI drop_add_item_list[{id}] item {other}"))?;
        }
        for other in adds {
            self.acquire(other, 1, path, assign_weapon)
                .with_context(|| format!("ITEM.INI add_item_list[{id}] item {other}"))?;
        }
        path.pop();
        self.repair_empty_selection(assign_weapon)?;
        let item = self.items.get_mut(&id).expect("validated item");
        item.count = item
            .count
            .checked_add(admitted)
            .with_context(|| format!("AddItem({id}): count exceeds u32"))?;
        Ok(())
    }

    pub(super) fn remove(&mut self, id: u16, amount: i32) -> Result<()> {
        if amount <= 0 {
            return Ok(());
        }
        let item = self
            .items
            .get_mut(&id)
            .with_context(|| format!("DropItem({id}): ITEM.INI definition absent"))?;
        item.count = item.count.saturating_sub(amount as u32);
        if item.flags & 1 != 0 {
            item.count = 0;
        }
        self.remove_dependents(id, &mut Vec::new())
    }

    fn remove_dependents(&mut self, id: u16, path: &mut Vec<u16>) -> Result<()> {
        if self.count(id)? == 0 {
            return Ok(());
        }
        anyhow::ensure!(
            !path.contains(&id),
            "ITEM.INI remove_drop_item_list cycle at item {id}: {path:?}"
        );
        path.push(id);
        let dependents = self.items[&id].remove_drop_items.clone();
        for other in dependents {
            self.remove_dependents(other, path)
                .with_context(|| format!("ITEM.INI remove_drop_item_list[{id}] item {other}"))?;
            self.items
                .get_mut(&other)
                .expect("validated dependent")
                .count = 0;
        }
        path.pop();
        Ok(())
    }
    /// Drop every owned item in id order, then clear the selection. The original walks ids
    /// zero up to the item count, so a definition gap is an error rather than skipped.
    pub(super) fn drop_all(&mut self) -> Result<()> {
        let count = u16::try_from(self.items.len()).context("item definitions exceed u16")?;
        for id in 0..count {
            let owned = self.count(id)?;
            if owned != 0 {
                self.remove(id, i32::try_from(owned).context("item count exceeds i32")?)?;
            }
        }
        self.selected = None;
        Ok(())
    }
    pub(super) fn select(&mut self, id: u16) -> Result<()> {
        self.count(id)?;
        self.selected = Some(id);
        Ok(())
    }
    pub(super) fn view(&mut self, cycle: Option<bool>) -> InventoryView {
        let valid: Vec<_> = self
            .items
            .iter()
            .filter_map(|(&id, item)| (item.count > 0 && !item.hidden).then_some(id))
            .collect();
        if self
            .selected
            .is_some_and(|id| self.items.get(&id).is_none_or(|item| item.count == 0))
        {
            self.selected = self
                .selected
                .and_then(|old| valid.iter().copied().find(|id| *id > old))
                .or_else(|| valid.first().copied());
        }
        if let Some(forward) = cycle
            && let Some(old) = self.selected
        {
            self.selected = if forward {
                valid
                    .iter()
                    .copied()
                    .find(|id| *id > old)
                    .or_else(|| valid.first().copied())
            } else {
                valid
                    .iter()
                    .rev()
                    .copied()
                    .find(|id| *id < old)
                    .or_else(|| valid.last().copied())
            };
        }
        InventoryView {
            selected: self.selected,
            counts: self
                .items
                .iter()
                .map(|(&id, item)| (id, item.count))
                .collect(),
        }
    }
}
