//! Inventory native-call dispatch.
use super::super::{Argument, Attribute, CallKind, CallResult, GameHost, Native, Result};
use anyhow::Context;

impl GameHost<'_, '_, '_, '_> {
    pub(in crate::script::registry) fn call_inventory(
        &mut self,
        kind: CallKind,
        function: Native,
        arguments: &[Argument],
    ) -> Result<Option<CallResult>> {
        if kind == CallKind::Function
            && (function == Native::ShowItem || function == Native::ShowItemNoRtx)
        {
            // ShowItem(item, text, first, second) ignores its text argument here; ShowItemNoRtx
            // has no text argument. Both leave the viewer request pending.
            let rtx = function == Native::ShowItem;
            let values = arguments
                .iter()
                .map(|argument| argument.integer())
                .collect::<Result<Vec<_>>>()?;
            let (item, words) = match (rtx, values.as_slice()) {
                (true, &[item, _, first, second]) => (item, [first, second]),
                (false, &[item, first, second]) => (item, [first, second]),
                _ => anyhow::bail!("{function} requires {} arguments", if rtx { 4 } else { 3 }),
            };
            let request = &mut self.registry.globals.item_viewer;
            request.pending = true;
            request.no_rtx |= !rtx;
            request.item = item;
            request.words = words;
            return Ok(Some(CallResult::Complete(0)));
        }

        if kind == CallKind::Function
            && (function == Native::DisplayHandModel || function == Native::DisplayHiltModel)
        {
            let [name, x, y, z] = arguments else {
                anyhow::bail!("{function} requires four arguments")
            };
            let Argument::String(name) = name else {
                anyhow::bail!("{function} requires a model name")
            };
            let data = self
                .inputs
                .animations
                .with_context(|| format!("{function} installed animation models unavailable"))?
                .0;
            let id = self.registry.id(self.index)?;
            self.registry.display_model_attachment(
                id,
                data,
                name,
                [x.integer()?, y.integer()?, z.integer()?],
                function == Native::DisplayHiltModel,
            )?;
            return Ok(Some(CallResult::Complete(0)));
        }

        if kind == CallKind::Function
            && (function == Native::DisplayHandItem || function == Native::DisplayHiltItem)
        {
            let [item, x, y, z] = arguments else {
                anyhow::bail!("{function} requires four arguments")
            };
            // The ordered item list is searched by unsigned identifier; without a match the
            // original requests a diagnostic before any model work.
            let item = item.integer()? as u32;
            let hud = self
                .inputs
                .hud
                .with_context(|| format!("{function} item list unavailable"))?;
            let model = hud
                .item_hand_model(item)?
                .with_context(|| format!("{function}: item {item} is not in the item list"))?;
            use cyrus_assets::hud::ItemHandModel;
            let model = match model {
                // An item without a hand model returns zero without allocating a record.
                ItemHandModel::Absent => return Ok(Some(CallResult::Complete(0))),
                ItemHandModel::Model(model) => model,
                ItemHandModel::Unexported => anyhow::bail!(
                    "{function}: item {item}'s hand model is not in this HUD export; export the \
                     installed assets again"
                ),
            };
            let data = self
                .inputs
                .animations
                .with_context(|| format!("{function} installed animation models unavailable"))?
                .0;
            let id = self.registry.id(self.index)?;
            self.registry.display_model_attachment(
                id,
                data,
                &model,
                [x.integer()?, y.integer()?, z.integer()?],
                function == Native::DisplayHiltItem,
            )?;
            return Ok(Some(CallResult::Complete(0)));
        }

        if kind == CallKind::Function && function == Native::AddItem {
            let [item, amount] = arguments else {
                anyhow::bail!("AddItem requires two arguments")
            };
            let item =
                u16::try_from(item.integer()?).context("AddItem item ID outside u16 range")?;
            self.add_session_item(item, amount.integer()?)?;
            return Ok(Some(CallResult::Complete(0)));
        }

        if kind == CallKind::Function && function == Native::ItemUsed {
            let [item] = arguments else {
                anyhow::bail!("ItemUsed requires one argument")
            };
            let value = self
                .inputs
                .hud
                .context("ItemUsed inventory host unavailable")?
                .item_used(item.integer()? as u32)?;
            return Ok(Some(CallResult::Complete(value)));
        }

        if kind == CallKind::Function && function == Native::UseItem {
            let [item] = arguments else {
                anyhow::bail!("UseItem requires one argument")
            };
            let item =
                u16::try_from(item.integer()?).context("UseItem item ID outside u16 range")?;
            self.use_item(item)?;
            return Ok(Some(CallResult::Complete(0)));
        }

        if kind == CallKind::Function && function == Native::SubItem {
            let [item, amount] = arguments else {
                anyhow::bail!("SubItem requires two arguments")
            };
            self.inputs
                .hud
                .context("SubItem inventory host unavailable")?
                .subtract_item(item.integer()? as u32, amount.integer()?)?;
            return Ok(Some(CallResult::Complete(0)));
        }

        if kind == CallKind::Function && function == Native::PushItem {
            let [item, value] = arguments else {
                anyhow::bail!("PushItem requires two arguments")
            };
            let value = self
                .inputs
                .hud
                .context("PushItem inventory host unavailable")?
                .push_item(item.integer()? as u32, value.integer()?)?;
            return Ok(Some(CallResult::Complete(value)));
        }

        if kind == CallKind::Function && function == Native::PushAllItems {
            let [value] = arguments else {
                anyhow::bail!("PushAllItems requires one argument")
            };
            self.inputs
                .hud
                .context("PushAllItems inventory host unavailable")?
                .push_all_items(value.integer()?)?;
            return Ok(Some(CallResult::Complete(0)));
        }

        if kind == CallKind::Function && function == Native::PopItem {
            let [item] = arguments else {
                anyhow::bail!("PopItem requires one argument")
            };
            let value = self
                .inputs
                .hud
                .context("PopItem inventory host unavailable")?
                .pop_item(item.integer()? as u32)?;
            return Ok(Some(CallResult::Complete(value)));
        }

        if kind == CallKind::Function && function == Native::PopAllItems {
            anyhow::ensure!(arguments.is_empty(), "PopAllItems requires no arguments");
            let hud = self
                .inputs
                .hud
                .context("PopAllItems inventory host unavailable")?;
            let player = self.selected_player();
            let generation = self.generation;
            let actors = &mut self.registry.actors;
            hud.pop_all_items(&mut |weapon| {
                let player = player.context("PopAllItems fallback selected player unavailable")?;
                anyhow::ensure!(
                    player.generation == generation && player.index < actors.len(),
                    "PopAllItems fallback selected player identity is stale"
                );
                actors[player.index].attributes[Attribute::Weapon] = weapon;
                Ok(())
            })?;
            return Ok(Some(CallResult::Complete(0)));
        }

        if kind == CallKind::Function && function == Native::RemoveItem {
            let [item] = arguments else {
                anyhow::bail!("RemoveItem requires one argument")
            };
            let item =
                u16::try_from(item.integer()?).context("RemoveItem item ID outside u16 range")?;
            self.inputs
                .hud
                .context("RemoveItem inventory host unavailable")?
                .remove_item(item)?;
            return Ok(Some(CallResult::Complete(0)));
        }

        // chargeWeapon(1) swaps the player's sabre for the flaming sabre (the charged weapon) and any
        // other operand swaps back: add the new item, record it as the weapon attribute, empty the
        // old item and select the new one. A closed inventory skips the adds and removals, and the
        // selection then fails as the original's does.
        if kind == CallKind::Function && function == Native::ChargeWeapon {
            let [charge] = arguments else {
                anyhow::bail!("chargeWeapon requires one argument")
            };
            let (take, drop) = if charge.integer()? as i16 == 1 {
                (
                    crate::script::inventory::FLAMING_SABRE,
                    crate::script::inventory::SABRE,
                )
            } else {
                (
                    crate::script::inventory::SABRE,
                    crate::script::inventory::FLAMING_SABRE,
                )
            };
            self.add_session_item(take, 1)?;
            self.registry.actors[self.index].attributes[Attribute::Weapon] = take as u8;
            let hud = self
                .inputs
                .hud
                .context("chargeWeapon inventory host unavailable")?;
            hud.remove_item(drop)?;
            hud.select_item(take)?;
            return Ok(Some(CallResult::Complete(0)));
        }

        if kind == CallKind::Function && function == Native::ActiveItem {
            anyhow::ensure!(arguments.is_empty(), "ActiveItem requires no arguments");
            let selected = self
                .inputs
                .hud
                .context("ActiveItem inventory host unavailable")?
                .active_item()?;
            return Ok(Some(CallResult::Complete(i32::from(selected))));
        }

        if kind == CallKind::Function && function == Native::HandItem {
            let [item] = arguments else {
                anyhow::bail!("HandItem requires one argument")
            };
            let hud = self
                .inputs
                .hud
                .context("HandItem inventory host unavailable")?;
            if hud.inventory_ready()? {
                self.registry.actors[self.index].hand_item(
                    item.integer()?,
                    hud,
                    self.inputs.shell_execution,
                )?;
            }
            return Ok(Some(CallResult::Complete(0)));
        }

        if kind == CallKind::Function && crate::script::ScriptHud::handles(function) {
            let hud = self.inputs.hud.context("script HUD host unavailable")?;
            return Ok(Some(CallResult::Complete(hud.call(function, arguments)?)));
        }
        Ok(None)
    }
}
