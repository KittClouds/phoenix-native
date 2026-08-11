use anyhow::{bail, Context, Result};
use phoenix_agent_control::{call_running_app, AgentControlRequestV1};
use phoenix_workspace::default_workspace_path;
use std::path::PathBuf;

fn main() {
    if let Err(error) = run() {
        eprintln!("phoenixctl: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let mut args = std::env::args().skip(1).collect::<Vec<_>>();
    let mut workspace = None;
    let mut pretty = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--workspace" => {
                let value = args.get(index + 1).context("--workspace requires a path")?;
                workspace = Some(PathBuf::from(value));
                args.drain(index..=index + 1);
            }
            "--pretty" => {
                pretty = true;
                args.remove(index);
            }
            _ => index += 1,
        }
    }
    if args.is_empty() {
        bail!("usage: phoenixctl [--workspace PATH] [--pretty] phx <domain> <verb> [arguments]");
    }
    let command = args
        .iter()
        .map(|argument| quote_phx_argument(argument))
        .collect::<Vec<_>>()
        .join(" ");
    let workspace = workspace.unwrap_or(default_workspace_path()?);
    let response = call_running_app(&workspace, &AgentControlRequestV1::new(command))?;
    if pretty {
        println!("{}", serde_json::to_string_pretty(&response)?);
    } else {
        println!("{}", serde_json::to_string(&response)?);
    }
    if response.error.is_some() {
        std::process::exit(2);
    }
    Ok(())
}

fn quote_phx_argument(argument: &str) -> String {
    if argument
        .chars()
        .any(|character| character.is_whitespace() || matches!(character, '\\' | '"' | '\''))
    {
        format!(
            "\"{}\"",
            argument.replace('\\', "\\\\").replace('"', "\\\"")
        )
    } else {
        argument.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::quote_phx_argument;

    #[test]
    fn preserves_multiword_text_as_one_phx_argument() {
        assert_eq!(quote_phx_argument("hello world"), "\"hello world\"");
    }
}
