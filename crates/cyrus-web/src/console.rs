//! Browser console preferences retain CPF bytes in local storage; logs download as text.
use cyrus_console::{Console, Preferences, Store};

struct LocalConsole {
    installed: Result<Option<Vec<u8>>, String>,
}
impl Store for LocalConsole {
    fn load(&self) -> anyhow::Result<Option<Preferences>> {
        let local = crate::storage::read("cyrus-console")?
            .map(|text| serde_json::from_str::<Vec<u8>>(&text))
            .transpose()?;
        let bytes = match local {
            Some(bytes) => Some(bytes),
            None => self
                .installed
                .as_ref()
                .map_err(|error| anyhow::anyhow!("{error}"))?
                .clone(),
        };
        bytes.as_deref().map(Preferences::decode).transpose()
    }
    fn save(&self, preferences: &Preferences) -> anyhow::Result<()> {
        crate::storage::write(
            "cyrus-console",
            &serde_json::to_string(&preferences.encode())?,
        )
    }
    fn write_log(&self, text: &str) -> anyhow::Result<()> {
        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen::prelude::*;
            #[wasm_bindgen(inline_js = r#"
export function consoleLog(text) {
  const url = URL.createObjectURL(new Blob([text], {type: 'text/plain'}));
  const link = document.createElement('a'); link.href = url; link.download = 'CONSOLE.LOG';
  document.body.append(link); link.click(); link.remove();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}
"#)]
            extern "C" {
                #[wasm_bindgen(catch, js_name = consoleLog)]
                fn download(text: &str) -> Result<(), JsValue>;
            }
            download(text).map_err(|error| anyhow::anyhow!("CONSOLE.LOG: {error:?}"))
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = text;
            anyhow::bail!("browser downloads unavailable")
        }
    }
}

pub(crate) fn initial() -> Console {
    #[cfg(target_arch = "wasm32")]
    let installed = {
        use wasm_bindgen::prelude::*;
        #[wasm_bindgen(
            inline_js = "export function consoleSeed() { return globalThis.cyrusConsoleSeed; }"
        )]
        extern "C" {
            #[wasm_bindgen(js_name = consoleSeed)]
            fn seed() -> String;
        }
        serde_json::from_str(&seed()).map_err(|error| format!("console startup data: {error}"))
    };
    #[cfg(not(target_arch = "wasm32"))]
    let installed = Err("browser console startup unavailable".into());
    Console::with_store(LocalConsole { installed })
}
