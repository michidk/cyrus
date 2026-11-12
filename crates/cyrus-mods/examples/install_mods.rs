//! Builds the bundled mods (`mods/*`) for `wasm32-wasip2` and installs them into the mods folder
//! of the game's config directory, on any platform: `cargo run -p cyrus-mods --example install_mods`
//! (`just mods`). A mod folder is one with a `mod.toml`; one with a `Cargo.toml` too is built, one
//! without (a manifest only, such as `serpent`) is copied as it is.
use anyhow::{Context, Result, bail, ensure};
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() -> Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let bundled = root.join("mods");
    let target = root.join("target/mods");
    let installed = cyrus_config::config_dir().join("mods");
    let mut mods = Vec::new();
    for item in std::fs::read_dir(&bundled).with_context(|| bundled.display().to_string())? {
        let dir = item?.path();
        if dir.join("mod.toml").is_file() {
            mods.push(dir);
        }
    }
    ensure!(
        !mods.is_empty(),
        "no mods with a mod.toml in {}",
        bundled.display()
    );
    mods.sort();
    for dir in &mods {
        let id = dir
            .file_name()
            .and_then(|name| name.to_str())
            .context("mod folder name is not UTF-8")?;
        let out = installed.join(id);
        std::fs::create_dir_all(&out).with_context(|| out.display().to_string())?;
        copy(&dir.join("mod.toml"), &out.join("mod.toml"))?;
        if dir.join("Cargo.toml").is_file() {
            build(dir, &target)?;
            // The library is named after the package, with underscores.
            let wasm = target
                .join("wasm32-wasip2/release")
                .join(format!("{}.wasm", id.replace('-', "_")));
            copy(&wasm, &out.join("mod.wasm"))?;
        }
        for item in std::fs::read_dir(dir)? {
            let path = item?.path();
            if path
                .extension()
                .is_some_and(|extension| extension == "wesl")
            {
                copy(
                    &path,
                    &out.join(path.file_name().context("shader file name")?),
                )?;
            }
        }
        let files = dir.join("files");
        if files.is_dir() {
            let _ = std::fs::remove_dir_all(out.join("files"));
            copy_tree(&files, &out.join("files"))?;
        }
        println!("installed {id} into {}", out.display());
    }
    Ok(())
}

fn build(dir: &Path, target: &Path) -> Result<()> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let status = Command::new(cargo)
        .args(["build", "--release", "--target", "wasm32-wasip2"])
        .env("CARGO_TARGET_DIR", target)
        .current_dir(dir)
        .status()
        .with_context(|| format!("run cargo in {}", dir.display()))?;
    if !status.success() {
        bail!("building {} failed: {status}", dir.display());
    }
    Ok(())
}

fn copy(from: &Path, to: &Path) -> Result<()> {
    std::fs::copy(from, to)
        .map(drop)
        .with_context(|| format!("copy {} to {}", from.display(), to.display()))
}

fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for item in std::fs::read_dir(from)? {
        let item = item?;
        let target: PathBuf = to.join(item.file_name());
        if item.file_type()?.is_dir() {
            copy_tree(&item.path(), &target)?;
        } else {
            copy(&item.path(), &target)?;
        }
    }
    Ok(())
}
