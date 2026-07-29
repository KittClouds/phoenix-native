mod graph_run;
mod materialized;

use anyhow::{bail, Context, Result};
use std::env;
use std::path::PathBuf;

fn main() -> Result<()> {
    let mut arguments = env::args_os().skip(1);
    let command = arguments
        .next()
        .and_then(|value| value.into_string().ok())
        .context("missing command")?;
    match command.as_str() {
        "inspect" => {
            let root = path(&mut arguments, "graph-run root")?;
            let manifest = path(&mut arguments, "manifest")?;
            let output = path(&mut arguments, "output directory")?;
            require_end(arguments)?;
            let receipt = graph_run::decode_verified_sections(&root, &manifest, &output)?;
            println!("{}", serde_json::to_string_pretty(&receipt)?);
        }
        "publish-materialized" => {
            let scene = path(&mut arguments, "materialized scene bundle directory")?;
            let root = path(&mut arguments, "scene publication root")?;
            require_end(arguments)?;
            let receipt = materialized::publish(&scene, &root)?;
            println!("{}", serde_json::to_string_pretty(&receipt)?);
        }
        _ => bail!("unknown command {command:?}; expected inspect or publish-materialized"),
    }
    Ok(())
}

fn path(arguments: &mut impl Iterator<Item = std::ffi::OsString>, name: &str) -> Result<PathBuf> {
    arguments
        .next()
        .map(PathBuf::from)
        .with_context(|| format!("missing {name} argument"))
}

fn require_end(mut arguments: impl Iterator<Item = std::ffi::OsString>) -> Result<()> {
    if arguments.next().is_some() {
        bail!("unexpected extra argument");
    }
    Ok(())
}
