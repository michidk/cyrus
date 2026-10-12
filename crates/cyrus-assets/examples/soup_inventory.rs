//! Read-only installed census; redirect output into magic/ for local investigation.
#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
fn main() -> anyhow::Result<()> {
    use anyhow::Context;
    use cyrus_assets::{install::GameInstall, scene::find_file, script::inventory, script_export};
    use rgpre::import::{rgm, world_ini};
    let install = GameInstall::locate(None)?;
    let path = world_ini::find_world_ini(&install.root).context("WORLD.INI absent")?;
    let worlds = world_ini::WorldIni::parse(&std::fs::read_to_string(path)?);
    anyhow::ensure!(!worlds.entries.is_empty(), "WORLD.INI contains no worlds");
    let mut reports = Vec::new();
    let mut failures = 0;
    for world in worlds.entries {
        let result = (|| -> anyhow::Result<_> {
            let filename = world
                .map
                .rsplit(['\\', '/'])
                .next()
                .context("empty map path")?;
            let path = find_file(&install.maps(), filename).context("map absent")?;
            let map = rgm::parse_rgm_file(&std::fs::read(&path)?)?;
            let scripts = script_export::prepare(&map, &install.root, world.index)?;
            Ok(inventory::inspect(&scripts))
        })();
        let report = match result {
            Ok(report) => serde_json::json!({"inventory": report}),
            Err(error) => {
                failures += 1;
                serde_json::json!({"error": format!("{error:#}")})
            }
        };
        reports.push(serde_json::json!({
            "world": world.index, "map": world.map, "palette": world.palette,
            "report": report
        }));
    }
    serde_json::to_writer_pretty(
        std::io::stdout().lock(),
        &serde_json::json!({
            "evidence": "static authored occurrences only; execution, host support and original parity not assessed",
            "worlds": reports
        }),
    )?;
    anyhow::ensure!(
        failures == 0,
        "{failures} worlds could not be inventoried; see report"
    );
    Ok(())
}

#[cfg(not(all(feature = "native", not(target_arch = "wasm32"))))]
fn main() {
    eprintln!("soup_inventory requires the native feature and a native target");
    std::process::exit(1);
}
