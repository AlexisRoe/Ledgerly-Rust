//! Embeds Ledgerly-Java's Flyway migrations (vendored, ADR 0006/0007) into the crate, so that the binary can
//! check the database it is pointed at without carrying the SQL files next to it.
//!
//! The list is built from the directory, not written by hand: a migration added upstream and re-vendored is
//! picked up by the next build, and the startup check then demands it.

use std::error::Error;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    let directory = manifest.join("../../vendor/ledgerly-java/src/main/resources/db/migration");
    println!("cargo::rerun-if-changed={}", directory.display());

    let mut migrations = Vec::new();
    for entry in std::fs::read_dir(&directory)? {
        let path = entry?.path();
        let Some(script) = path.file_name().and_then(|name| name.to_str()).map(str::to_owned) else {
            return Err(format!("not a UTF-8 file name: {}", path.display()).into());
        };
        let (version, description) = versioned_name(&script)
            .ok_or_else(|| format!("{script} is not named like a versioned Flyway migration (V<n>__<name>.sql)"))?;
        migrations.push((sort_key(&version)?, version, description, script, path.canonicalize()?));
    }
    migrations.sort_by(|a, b| a.0.cmp(&b.0));
    if let Some(pair) = migrations.windows(2).find(|pair| pair[0].0 == pair[1].0) {
        return Err(format!("two migrations with version {}", pair[0].1).into());
    }

    let mut code = String::from("&[\n");
    for (_, version, description, script, path) in &migrations {
        writeln!(
            code,
            "    VendoredMigration {{ version: {version:?}, description: {description:?}, script: {script:?}, \
             sql: include_str!({:?}) }},",
            path_text(path)?
        )?;
    }
    code.push(']');
    std::fs::write(PathBuf::from(std::env::var("OUT_DIR")?).join("migrations.rs"), code)?;
    Ok(())
}

/// `V10__external_identity.sql` → `("10", "external identity")`, as Flyway reads the name.
fn versioned_name(script: &str) -> Option<(String, String)> {
    let (version, description) = script.strip_prefix('V')?.strip_suffix(".sql")?.split_once("__")?;
    let well_formed = !version.is_empty()
        && version
            .split(['.', '_'])
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()));
    well_formed.then(|| (version.replace('_', "."), description.replace('_', " ")))
}

fn sort_key(version: &str) -> Result<Vec<u64>, Box<dyn Error>> {
    Ok(version.split('.').map(str::parse).collect::<Result<_, _>>()?)
}

fn path_text(path: &Path) -> Result<&str, Box<dyn Error>> {
    path.to_str()
        .ok_or_else(|| format!("not a UTF-8 path: {}", path.display()).into())
}
