//! Presentation of configuration fields decoded by preservation.
use anyhow::Result;
use rgpre::import;
use serde_json::{Value, json};

pub fn decode(text: &str, path: &str) -> Result<Value> {
    let name = path.rsplit('/').next().unwrap_or(path).to_ascii_uppercase();
    let decoded = match name.as_str() {
        "KEYS.INI" => {
            let keys = import::keys_ini::KeysIni::parse(text).map_err(anyhow::Error::msg)?;
            json!({"keyboard":keys.keyboard,"user":keys.user,"extra":keys.extra,"names":keys.names,"waiting_message":keys.waiting_message})
        }
        "FOG.INI" => json!({"fog":import::fog_ini::parse_fog_ini(text)?.to_vec()}),
        "SYSTEM.INI" => {
            let player = import::player_ini::PlayerConfig::parse(text)?;
            json!({"player":{"standing_height":player.standing_height,"post_collide_height":player.post_collide_height,"slide_range":player.slide_range,"turn_speed":player.turn_speed,"turn_max_speed":player.turn_max_speed,"poly_push_units":player.poly_push_units}})
        }
        "WORLD.INI" => {
            json!({"worlds":import::world_ini::WorldIni::parse(text).entries.iter().map(|w|json!({"index":w.index,"map":w.map,"palette":w.palette,"terrain":w.world})).collect::<Vec<_>>()})
        }
        "MENU.INI" => {
            let mut pages = serde_json::Map::new();
            for line in text.lines() {
                let line = line.trim().to_ascii_lowercase();
                if let Some(index) = line
                    .strip_prefix("[page")
                    .and_then(|s| s.strip_suffix(']'))
                    .and_then(|s| s.parse::<usize>().ok())
                {
                    let page = import::menu_ini::MenuPage::parse(text, index)
                        .map_err(anyhow::Error::msg)?;
                    pages.insert(index.to_string(),json!({
                        "textures":page.textures.map(|t|t.map(|t|json!({"set":t.set,"index":t.index}))),
                        "entries":page.entries.iter().map(|e|json!({"text":e.text,"position":e.position,"texture":e.texture,"action":e.action,"selectable":e.selectable,"default":e.default,"grayed":e.grayed,"slider":e.slider,"justify":e.justify,"output_position":e.output_position,"output_justify":e.output_justify})).collect::<Vec<_>>()
                    }));
                }
            }
            json!({"pages":pages})
        }
        "SOUP386.DEF" => {
            let soup = import::soup_def::parse_soup_def(text);
            json!({"functions":soup.functions.iter().map(|f|json!({"name":f.name,"parameters":f.param_count})).collect::<Vec<_>>(),"flags":soup.flags.iter().map(|f|json!({"name":f.name,"type":f.flag_type})).collect::<Vec<_>>(),"references":soup.references,"attributes":soup.attributes,"animation_groups":soup.anim_groups})
        }
        _ => Value::Null,
    };
    Ok(json!({"decoded":decoded,"source_lines":text.lines().collect::<Vec<_>>()}))
}
