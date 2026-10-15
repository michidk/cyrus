//! Session item ownership and selection from installed inventory definitions.
use anyhow::{Context, Result};
use cyrus_assets::hud::{HudAsset, InventoryItem};
use std::collections::BTreeMap;

/// Current item presentation. Ownership includes hidden items; cycling excludes them.
pub struct InventoryView {
    pub selected: Option<u16>,
    pub counts: BTreeMap<u16, u32>,
}

pub(super) struct Inventory {
    items: BTreeMap<u16, InventoryItem>,
    selected: Option<u16>,
}
impl Inventory {
    pub fn new(hud: &HudAsset) -> Self {
        Self {
            items: hud.inventory.clone(),
            selected: hud.selected.or_else(|| {
                hud.items
                    .iter()
                    .find(|item| item.count > 0)
                    .map(|item| item.id)
            }),
        }
    }
    pub fn inspect(&self) -> serde_json::Value {
        serde_json::json!({
            "selected": self.selected,
            "counts": self.items.iter().map(|(&id, item)| (id, item.count)).collect::<BTreeMap<_, _>>(),
        })
    }
    pub fn count(&self, id: u16) -> Result<u32> {
        Ok(self
            .items
            .get(&id)
            .with_context(|| format!("ITEM.INI item {id}: definition absent"))?
            .count)
    }
    pub fn add(&mut self, id: u16, amount: i32) -> Result<()> {
        let item = self
            .items
            .get_mut(&id)
            .with_context(|| format!("AddItem({id}): ITEM.INI definition absent"))?;
        let amount = u32::try_from(amount)
            .unwrap_or(0)
            .min(item.total.saturating_sub(item.count))
            .min(item.player_max.saturating_sub(item.count));
        if amount == 0 {
            return Ok(());
        }
        anyhow::ensure!(
            !item.acquisition_effects,
            "AddItem({id}): ITEM.INI acquisition lists require unsupported item services"
        );
        item.count += amount;
        Ok(())
    }
    pub fn select(&mut self, id: u16) -> Result<()> {
        self.count(id)?;
        self.selected = Some(id);
        Ok(())
    }
    pub fn view(&mut self, cycle: Option<bool>) -> InventoryView {
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
