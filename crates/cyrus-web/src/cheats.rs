//! Browser CHT storage uses the same codec and startup rules as the native host.
use cyrus_assets::cheats::{self, CheatFile, CheatSeed};
use cyrus_game::cheats::{CheatStore, Cheats};

struct LocalCheats {
    seed: Result<CheatSeed, String>,
}
impl CheatStore for LocalCheats {
    fn load(&self) -> anyhow::Result<CheatFile> {
        let seed = self
            .seed
            .as_ref()
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        let bytes = crate::storage::read("cyrus-cheats")?
            .map(|text| serde_json::from_str::<Vec<u8>>(&text))
            .transpose()?;
        let mut state = match bytes.as_ref().or(seed.installed.as_ref()) {
            Some(bytes) => cheats::decode(bytes)?,
            None => cheats::empty(),
        };
        seed.defaults.apply(&mut state)?;
        Ok(state)
    }
    fn save(&self, state: &CheatFile) -> anyhow::Result<()> {
        crate::storage::write(
            "cyrus-cheats",
            &serde_json::to_string(&cheats::encode(state))?,
        )
    }
}

pub(crate) fn initial() -> Cheats {
    #[cfg(target_arch = "wasm32")]
    let seed = {
        use wasm_bindgen::prelude::*;
        #[wasm_bindgen(
            inline_js = "export function cheatSeed() { return globalThis.cyrusCheatSeed; }"
        )]
        extern "C" {
            #[wasm_bindgen(js_name = cheatSeed)]
            fn seed_json() -> String;
        }
        serde_json::from_str(&seed_json()).map_err(|error| format!("cheat startup data: {error}"))
    };
    #[cfg(not(target_arch = "wasm32"))]
    let seed = Err("browser cheat startup unavailable".into());
    Cheats::new(LocalCheats { seed })
}

#[cfg(target_arch = "wasm32")]
mod transfers {
    use super::*;
    use bevy::prelude::*;
    use cyrus_game::cheats::{CheatFileAction, CheatTransfer};
    use wasm_bindgen::prelude::*;
    thread_local! { static IMPORT: std::cell::RefCell<Option<Result<Option<Vec<u8>>, String>>> = const { std::cell::RefCell::new(None) }; }
    #[wasm_bindgen(inline_js = r#"
export async function chooseCheats(size) {
  const input = document.createElement('input'); input.type = 'file'; input.accept = '.cht';
  return await new Promise((resolve, reject) => {
    input.oncancel = () => resolve(null);
    input.onchange = async () => {
      try {
        const file = input.files[0];
        if (!file) { resolve(null); return; }
        if (file.size !== size) throw new Error('REDGUARD.CHT must contain ' + size + ' bytes');
        resolve(new Uint8Array(await file.arrayBuffer()));
      } catch (error) { reject(error); }
    };
    input.click();
  });
}
export function downloadCheats(bytes) {
  const url = URL.createObjectURL(new Blob([bytes], {type: 'application/octet-stream'}));
  const link = document.createElement('a'); link.href = url; link.download = 'REDGUARD.CHT';
  document.body.append(link); link.click(); link.remove();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}
"#)]
    extern "C" {
        #[wasm_bindgen(catch, js_name = chooseCheats)]
        async fn choose(size: usize) -> Result<JsValue, JsValue>;
        #[wasm_bindgen(catch, js_name = downloadCheats)]
        fn download(bytes: &[u8]) -> Result<(), JsValue>;
    }
    pub(super) fn update(
        mut cheats: ResMut<Cheats>,
        mut transfer: ResMut<CheatTransfer>,
        mut pending: Local<bool>,
    ) {
        transfer.available = true;
        if let Some(result) = IMPORT.with(|slot| slot.borrow_mut().take()) {
            *pending = false;
            transfer.status = Some(match result {
                Ok(None) => "Import cancelled; cheat state unchanged.".into(),
                Ok(Some(bytes)) => {
                    match cheats::decode(&bytes).and_then(|state| cheats.import(state)) {
                        Ok(()) => "Cheat file imported and saved.".into(),
                        Err(error) => format!("Import failed: {error:#}"),
                    }
                }
                Err(error) => format!("Import failed: {error}"),
            });
        }
        match transfer.action.take() {
            Some(CheatFileAction::Import) if !*pending => {
                *pending = true;
                transfer.status = Some("Choose a cheat file to replace local cheat state.".into());
                wasm_bindgen_futures::spawn_local(async {
                    let result = choose(cheats::encode(&cheats::empty()).len())
                        .await
                        .map(|value| {
                            if value.is_null() {
                                None
                            } else {
                                Some(js_sys::Uint8Array::new(&value).to_vec())
                            }
                        })
                        .map_err(|error| format!("{error:?}"));
                    IMPORT.with(|slot| *slot.borrow_mut() = Some(result));
                });
            }
            Some(CheatFileAction::Export) => {
                transfer.status = Some(
                    match cheats.state().and_then(|state| {
                        download(&cheats::encode(state))
                            .map_err(|error| anyhow::anyhow!("{error:?}"))
                    }) {
                        Ok(()) => "Cheat file exported.".into(),
                        Err(error) => format!("Export failed: {error:#}"),
                    },
                );
            }
            _ => {}
        }
    }
}
pub(crate) fn register(app: &mut bevy::prelude::App) {
    #[cfg(target_arch = "wasm32")]
    app.add_systems(bevy::prelude::Update, transfers::update);
    #[cfg(not(target_arch = "wasm32"))]
    let _ = app;
}
