use clap::{Parser, Subcommand};
use std::collections::HashMap;
use std::io::IsTerminal;
use std::os::unix::process::CommandExt;
use std::process::Command;

const AFTER_HELP: &str = r#"EXAMPLES:
    dotshell                      Load .env from the current directory
    dotshell .env.production
    dotshell .env .env.production
    dotshell --reload
    dotshell --reload .env.staging

SHELL INTEGRATION:
    To use `dotshell --reload` without eval, add this to your shell rc file:

    # ~/.zshrc or ~/.bashrc
    eval "$(dotshell shell-init)"

    # ~/.config/fish/config.fish
    dotshell shell-init fish | source

    After that, `dotshell --reload` will update the current shell session in place."#;

const ZSH_BASH_INIT: &str = r#"export DOTSHELL_INIT=1
dotshell() {
  local arg has_reload=0
  for arg in "$@"; do
    [ "$arg" = "--reload" ] && has_reload=1 && break
  done
  if [ $has_reload -eq 1 ]; then
    source <(command dotshell "$@")
  else
    command dotshell "$@"
  fi
}"#;

const FISH_INIT: &str = r"set -x DOTSHELL_INIT 1
function dotshell
  if contains -- --reload $argv
    command dotshell $argv | source
  else
    command dotshell $argv
  end
end";

#[derive(Parser)]
#[command(
    name = "dotshell",
    version,
    about = "Start a shell session with environment variables loaded from files",
    after_help = AFTER_HELP
)]
struct Cli {
    /// Env file(s) to load; defaults to .env if omitted
    files: Vec<String>,

    /// Reload vars into the current shell session
    #[arg(long)]
    reload: bool,

    #[command(subcommand)]
    command: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Print shell integration setup
    ShellInit {
        /// Shell name (zsh, bash, fish); defaults to $SHELL
        shell: Option<String>,
    },
}

fn main() {
    let mut cli = Cli::parse();

    if let Some(Cmd::ShellInit { shell }) = cli.command {
        shell_init(shell.as_deref());
        return;
    }

    if !cli.reload && cli.files.is_empty() {
        cli.files.push(".env".to_string());
    }

    if cli.reload {
        let resolved = if cli.files.is_empty() {
            std::env::var("DOTSHELL_FILES")
                .unwrap_or_else(|_| {
                    eprintln!("dotshell: DOTSHELL_FILES is not set; pass file paths explicitly");
                    std::process::exit(1);
                })
                .split(':')
                .map(str::to_string)
                .collect()
        } else {
            cli.files
        };

        let previous = tracked_keys(std::env::var("DOTSHELL_KEYS").ok().as_deref());
        let loaded = load_files(&resolved).unwrap_or_else(|e| {
            eprintln!("dotshell: {e}");
            std::process::exit(1);
        });
        let keys = keys_list(&loaded);

        if std::io::stdout().is_terminal() {
            let depth = next_depth(std::env::var("DOTSHELL_DEPTH").ok().as_deref());
            let mut merged = loaded;
            merged.insert("DOTSHELL_FILES".to_string(), resolved.join(":"));
            merged.insert("DOTSHELL_DEPTH".to_string(), depth.to_string());
            merged.insert("DOTSHELL_KEYS".to_string(), keys);
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
            let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("/"));
            println!(
                "[dotshell] shell has not been initialized; starting a new session instead (depth: {depth})."
            );
            let mut cmd = Command::new(&shell);
            cmd.envs(&merged).current_dir(cwd);
            for key in &previous {
                cmd.env_remove(key);
            }
            let err = cmd.exec();
            eprintln!("dotshell: failed to exec \"{shell}\": {err}");
            std::process::exit(1);
        } else {
            for key in &previous {
                println!("unset {key}");
            }
            for (key, value) in &loaded {
                let escaped = value.replace('\'', r"'\''");
                println!("export {key}='{escaped}'");
            }
            println!("export DOTSHELL_KEYS='{keys}'");
            eprintln!("[dotshell] Reloaded.");
        }
    } else {
        let depth = next_depth(std::env::var("DOTSHELL_DEPTH").ok().as_deref());
        let loaded = load_files(&cli.files).unwrap_or_else(|e| {
            eprintln!("dotshell: {e}");
            std::process::exit(1);
        });
        let keys = keys_list(&loaded);
        let mut merged = loaded;
        merged.insert("DOTSHELL_FILES".to_string(), cli.files.join(":"));
        merged.insert("DOTSHELL_DEPTH".to_string(), depth.to_string());
        merged.insert("DOTSHELL_KEYS".to_string(), keys);

        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("/"));

        println!("[dotshell] Started a new shell session (depth: {depth}).");

        let err = Command::new(&shell).envs(&merged).current_dir(cwd).exec();

        eprintln!("dotshell: failed to exec \"{shell}\": {err}");
        std::process::exit(1);
    }
}

fn tracked_keys(raw: Option<&str>) -> Vec<String> {
    raw.unwrap_or_default()
        .split(':')
        .filter(|key| !key.is_empty())
        .map(str::to_string)
        .collect()
}

fn keys_list(loaded: &HashMap<String, String>) -> String {
    loaded.keys().cloned().collect::<Vec<_>>().join(":")
}

fn next_depth(current: Option<&str>) -> u32 {
    current.and_then(|v| v.parse::<u32>().ok()).unwrap_or(0) + 1
}

fn shell_init_script(shell_name: &str) -> Result<&'static str, String> {
    match shell_name {
        "zsh" | "bash" => Ok(ZSH_BASH_INIT),
        "fish" => Ok(FISH_INIT),
        other => Err(format!("unsupported shell: {other}")),
    }
}

fn shell_init(shell: Option<&str>) {
    let detected = std::env::var("SHELL").unwrap_or_default();
    let shell_name = shell.unwrap_or_else(|| detected.rsplit('/').next().unwrap_or("sh"));

    match shell_init_script(shell_name) {
        Ok(script) => println!("{script}"),
        Err(e) => {
            eprintln!("dotshell: {e}");
            std::process::exit(1);
        }
    }
}

fn load_files(paths: &[String]) -> Result<HashMap<String, String>, String> {
    let mut merged: HashMap<String, String> = HashMap::new();

    for path in paths {
        let iter = match dotenvy::from_path_iter(path) {
            Ok(iter) => iter,
            Err(e) if e.not_found() => continue,
            Err(e) => return Err(format!("error reading \"{path}\": {e}")),
        };

        for item in iter {
            let (key, value) = item.map_err(|e| format!("error parsing \"{path}\": {e}"))?;
            merged.insert(key, value);
        }
    }

    Ok(merged)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "dotshell-test-{label}-{:?}-{}",
            std::thread::current().id(),
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn next_depth_defaults_to_one_when_unset() {
        assert_eq!(next_depth(None), 1);
    }

    #[test]
    fn next_depth_increments_valid_value() {
        assert_eq!(next_depth(Some("3")), 4);
    }

    #[test]
    fn next_depth_ignores_invalid_value() {
        assert_eq!(next_depth(Some("not-a-number")), 1);
    }

    #[test]
    fn tracked_keys_empty_when_unset() {
        assert_eq!(tracked_keys(None), Vec::<String>::new());
    }

    #[test]
    fn tracked_keys_splits_on_colon() {
        assert_eq!(tracked_keys(Some("FOO:BAR")), vec!["FOO", "BAR"]);
    }

    #[test]
    fn tracked_keys_ignores_empty_segments() {
        assert_eq!(tracked_keys(Some("")), Vec::<String>::new());
        assert_eq!(tracked_keys(Some("FOO::BAR")), vec!["FOO", "BAR"]);
    }

    #[test]
    fn keys_list_empty_for_empty_map() {
        assert_eq!(keys_list(&HashMap::new()), "");
    }

    #[test]
    fn keys_list_round_trips_through_tracked_keys() {
        let mut loaded = HashMap::new();
        loaded.insert("FOO".to_string(), "1".to_string());
        loaded.insert("BAR".to_string(), "2".to_string());

        let joined = keys_list(&loaded);
        let mut keys = tracked_keys(Some(&joined));
        keys.sort();

        assert_eq!(keys, vec!["BAR", "FOO"]);
    }

    #[test]
    fn shell_init_script_supports_zsh_and_bash() {
        assert!(shell_init_script("zsh").unwrap().contains("dotshell()"));
        assert!(shell_init_script("bash").unwrap().contains("dotshell()"));
    }

    #[test]
    fn shell_init_script_supports_fish() {
        assert!(
            shell_init_script("fish")
                .unwrap()
                .contains("function dotshell")
        );
    }

    #[test]
    fn shell_init_script_rejects_unknown_shell() {
        assert_eq!(
            shell_init_script("powershell").unwrap_err(),
            "unsupported shell: powershell"
        );
    }

    #[test]
    fn load_files_skips_missing_files_but_loads_existing_ones() {
        let dir = temp_dir("missing-ok");
        let existing = dir.join(".env");
        fs::write(&existing, "FOO=1\n").unwrap();
        let missing = dir.join(".env.missing");

        let loaded = load_files(&[
            existing.to_string_lossy().to_string(),
            missing.to_string_lossy().to_string(),
        ])
        .unwrap();

        assert_eq!(loaded.get("FOO"), Some(&"1".to_string()));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_files_errors_on_malformed_file() {
        let dir = temp_dir("malformed");
        let bad = dir.join(".env.bad");
        fs::write(&bad, "this is not valid\n").unwrap();

        let result = load_files(&[bad.to_string_lossy().to_string()]);

        assert!(result.is_err());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_files_last_file_wins_on_duplicate_keys() {
        let dir = temp_dir("override");
        let base = dir.join(".env");
        let overrides = dir.join(".env.local");
        fs::write(&base, "FOO=base\n").unwrap();
        fs::write(&overrides, "FOO=override\n").unwrap();

        let loaded = load_files(&[
            base.to_string_lossy().to_string(),
            overrides.to_string_lossy().to_string(),
        ])
        .unwrap();

        assert_eq!(loaded.get("FOO"), Some(&"override".to_string()));
        fs::remove_dir_all(&dir).ok();
    }
}
