//! IDE and Agent MCP setup / scanner CLI.
//! Detects Cursor, Claude Desktop, Antigravity, Windsurf, VS Code (Cline/Roo), Zed, Claude Code.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpConfigFormat {
    McpServers,     // "mcpServers": { "blitz": { ... } }
    ContextServers, // "context_servers": { "blitz": { ... } } (Zed)
}

impl McpConfigFormat {
    pub fn key(self) -> &'static str {
        match self {
            Self::McpServers => "mcpServers",
            Self::ContextServers => "context_servers",
        }
    }
}

#[derive(Debug, Clone)]
pub struct IdeTarget {
    pub id: &'static str,
    pub name: &'static str,
    pub config_path: PathBuf,
    pub format: McpConfigFormat,
    pub configured: bool,
}

/// Detect IDEs and AI agents installed on the system.
pub fn scan_installed_ides() -> Vec<IdeTarget> {
    let mut targets = Vec::new();

    let home = dirs::home_dir();
    let config_dir = dirs::config_dir();

    // 1. Antigravity / Gemini CLI
    if let Some(ref h) = home {
        let gemini_dir = h.join(".gemini");
        if gemini_dir.exists() {
            let antigravity_config = gemini_dir.join("antigravity").join("mcp_config.json");
            let config_config = gemini_dir.join("config").join("mcp_config.json");
            let target_path = if antigravity_config.exists() {
                antigravity_config
            } else if config_config.exists() {
                config_config
            } else {
                antigravity_config
            };
            targets.push(create_target(
                "antigravity",
                "Antigravity / Gemini",
                target_path,
                McpConfigFormat::McpServers,
            ));
        }
    }

    // 2. Cursor
    if let Some(ref h) = home {
        let cursor_dir = h.join(".cursor");
        let cursor_mac = Path::new("/Applications/Cursor.app");
        let detected = cursor_dir.exists() || cursor_mac.exists();
        if detected {
            targets.push(create_target(
                "cursor",
                "Cursor",
                cursor_dir.join("mcp.json"),
                McpConfigFormat::McpServers,
            ));
        }
    }

    // 3. Claude Desktop
    if let Some(ref c) = config_dir {
        let claude_dir = c.join("Claude");
        let claude_mac = Path::new("/Applications/Claude.app");
        let detected = claude_dir.exists() || claude_mac.exists();
        if detected {
            targets.push(create_target(
                "claude_desktop",
                "Claude Desktop",
                claude_dir.join("claude_desktop_config.json"),
                McpConfigFormat::McpServers,
            ));
        }
    }

    // 4. Windsurf
    if let Some(ref h) = home {
        let windsurf_dir = h.join(".codeium").join("windsurf");
        let windsurf_mac = Path::new("/Applications/Windsurf.app");
        let detected = windsurf_dir.exists() || windsurf_mac.exists();
        if detected {
            targets.push(create_target(
                "windsurf",
                "Windsurf",
                windsurf_dir.join("mcp_config.json"),
                McpConfigFormat::McpServers,
            ));
        }
    }

    // 5. VS Code (Official User MCP)
    if let Some(ref c) = config_dir {
        let code_user_dir = c.join("Code").join("User");
        let code_mac = Path::new("/Applications/Visual Studio Code.app");
        let detected = code_user_dir.exists() || code_mac.exists();
        if detected {
            targets.push(create_target(
                "vscode",
                "VS Code",
                code_user_dir.join("mcp.json"),
                McpConfigFormat::McpServers,
            ));

            // 6. Cline extension in VS Code
            let cline_dir = code_user_dir
                .join("globalStorage")
                .join("saoudrizwan.claude-dev");
            if cline_dir.exists() {
                targets.push(create_target(
                    "cline",
                    "Cline (VS Code)",
                    cline_dir.join("settings").join("cline_mcp_settings.json"),
                    McpConfigFormat::McpServers,
                ));
            }

            // 7. Roo Code extension in VS Code
            let roo_dir = code_user_dir
                .join("globalStorage")
                .join("rooveterinaryinc.roo-cline");
            if roo_dir.exists() {
                targets.push(create_target(
                    "roo_code",
                    "Roo Code (VS Code)",
                    roo_dir.join("settings").join("cline_mcp_settings.json"),
                    McpConfigFormat::McpServers,
                ));
            }
        }
    }

    // 8. Zed
    if let Some(ref c) = config_dir {
        let zed_dir = c.join("Zed");
        let zed_config_alt = home.as_ref().map(|h| h.join(".config").join("zed"));
        let zed_mac = Path::new("/Applications/Zed.app");
        let alt_exists = zed_config_alt.as_ref().is_some_and(|p| p.exists());
        let detected = zed_dir.exists() || alt_exists || zed_mac.exists();
        if detected {
            let target_path = if zed_dir.exists() || !alt_exists {
                zed_dir.join("settings.json")
            } else {
                zed_config_alt.expect("checked exists").join("settings.json")
            };
            targets.push(create_target(
                "zed",
                "Zed",
                target_path,
                McpConfigFormat::ContextServers,
            ));
        }
    }

    // 9. Claude Code CLI
    if let Some(ref h) = home {
        let claude_cli_file = h.join(".claude.json");
        let claude_cli_dir = h.join(".claude");
        if claude_cli_file.exists() || claude_cli_dir.exists() {
            targets.push(create_target(
                "claude_code",
                "Claude Code CLI",
                claude_cli_file,
                McpConfigFormat::McpServers,
            ));
        }
    }

    targets
}

fn create_target(
    id: &'static str,
    name: &'static str,
    config_path: PathBuf,
    format: McpConfigFormat,
) -> IdeTarget {
    let configured = check_if_configured(&config_path, format.key());
    IdeTarget {
        id,
        name,
        config_path,
        format,
        configured,
    }
}

/// Check if Blitz MCP is already configured in this JSON config file.
fn check_if_configured(path: &Path, key: &str) -> bool {
    let Ok(content) = fs::read_to_string(path) else {
        return false;
    };
    let clean = strip_json_comments(&content);
    let Ok(val) = serde_json::from_str::<Value>(&clean) else {
        return false;
    };
    val.get(key)
        .and_then(|m| m.get("blitz"))
        .is_some()
}

/// Strip line comments `// ...` to tolerate JSON with comments.
pub fn strip_json_comments(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_str = false;
    let mut escape = false;

    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if in_str {
            out.push(ch);
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == '"' {
                in_str = false;
            }
            i += 1;
            continue;
        }

        if ch == '"' {
            in_str = true;
            out.push(ch);
            i += 1;
            continue;
        }

        if ch == '/' && i + 1 < chars.len() && chars[i + 1] == '/' {
            // Skip until newline
            i += 2;
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }

        out.push(ch);
        i += 1;
    }

    out
}

/// Inject or update Blitz MCP server entry in target configuration file.
pub fn configure_target(target: &IdeTarget, blitz_bin: &Path) -> Result<(), String> {
    if let Some(parent) = target.config_path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create dir {}: {e}", parent.display()))?;
        }
    }

    let mut root: Value = if target.config_path.exists() {
        let raw = fs::read_to_string(&target.config_path)
            .map_err(|e| format!("failed to read {}: {e}", target.config_path.display()))?;
        let clean = strip_json_comments(&raw);
        if clean.trim().is_empty() {
            json!({})
        } else {
            serde_json::from_str(&clean).map_err(|e| {
                format!(
                    "invalid JSON in {}: {e} (aborting to avoid overwriting invalid file)",
                    target.config_path.display()
                )
            })?
        }
    } else {
        json!({})
    };

    let root_obj = root
        .as_object_mut()
        .ok_or_else(|| format!("root in {} is not a JSON object", target.config_path.display()))?;

    let key = target.format.key();
    let servers = root_obj
        .entry(key)
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| format!("'{key}' in {} is not a JSON object", target.config_path.display()))?;

    let bin_str = blitz_bin.to_string_lossy().to_string();
    servers.insert(
        "blitz".to_string(),
        json!({
            "command": bin_str,
            "args": ["mcp"]
        }),
    );

    let formatted = serde_json::to_string_pretty(&root)
        .map_err(|e| format!("failed to format JSON: {e}"))?;

    fs::write(&target.config_path, format!("{formatted}\n"))
        .map_err(|e| format!("failed to write {}: {e}", target.config_path.display()))?;

    Ok(())
}

/// Resolve absolute Blitz binary to embed in MCP configurations.
pub fn resolve_blitz_binary(override_bin: Option<&str>) -> PathBuf {
    if let Some(bin) = override_bin {
        return PathBuf::from(bin);
    }

    // If macOS app bundle exists, prefer standard path for stability across runs
    #[cfg(target_os = "macos")]
    {
        let standard_app = PathBuf::from("/Applications/Blitz.app/Contents/MacOS/Blitz");
        if standard_app.exists() {
            return standard_app;
        }
    }

    // Check ~/.local/bin/blitz
    if let Some(home) = dirs::home_dir() {
        let local_bin = home.join(".local").join("bin").join(if cfg!(windows) { "blitz.exe" } else { "blitz" });
        if local_bin.exists() {
            return local_bin;
        }
    }

    // Check current_exe
    if let Ok(exe) = std::env::current_exe() {
        if let Ok(canonical) = exe.canonicalize() {
            return canonical;
        }
        return exe;
    }

    // Fallback: command name directly
    PathBuf::from(if cfg!(windows) { "blitz.exe" } else { "blitz" })
}

/// CLI handler for `blitz mcp setup [flags]`
pub fn run_mcp_setup_cli(args: &[String]) -> i32 {
    let mut all = false;
    let mut scan_only = false;
    let mut override_bin: Option<String> = None;
    let mut target_filter: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--all" | "-a" | "-y" | "--yes" => all = true,
            "--scan" | "-s" => scan_only = true,
            "--bin" | "-b" => {
                i += 1;
                override_bin = args.get(i).cloned();
            }
            "--ide" | "--target" | "-t" => {
                i += 1;
                target_filter = args.get(i).cloned();
            }
            "help" | "--help" | "-h" => {
                print_mcp_setup_help();
                return 0;
            }
            unknown => {
                eprintln!("unknown arg: {unknown}");
                print_mcp_setup_help();
                return 1;
            }
        }
        i += 1;
    }

    let mut targets = scan_installed_ides();
    if let Some(ref filter) = target_filter {
        targets.retain(|t| {
            t.id.eq_ignore_ascii_case(filter)
                || t.name.to_lowercase().contains(&filter.to_lowercase())
        });
        if targets.is_empty() {
            eprintln!("No detected IDE matched target '{filter}'.");
            return 1;
        }
        all = true; // explicitly specified target -> configure without prompting
    }
    let blitz_bin = resolve_blitz_binary(override_bin.as_deref());

    println!("\n⚡ Blitz MCP Setup\n");
    println!("Blitz executable: {}\n", blitz_bin.display());

    if targets.is_empty() {
        println!("No supported IDEs or AI coding agents detected automatically.\n");
        println!("Supported targets: Cursor, Claude Desktop, Antigravity, Windsurf, VS Code (Cline/Roo), Zed, Claude Code.\n");
        println!("To configure manually, add this to your IDE's MCP settings:\n");
        println!(
            "{{\n  \"mcpServers\": {{\n    \"blitz\": {{\n      \"command\": \"{}\",\n      \"args\": [\"mcp\"]\n    }}\n  }}\n}}",
            blitz_bin.display()
        );
        return 0;
    }

    println!("Detected IDEs and AI coding agents:");
    for (idx, target) in targets.iter().enumerate() {
        let status = if target.configured {
            "[configured]"
        } else {
            "[ready]"
        };
        println!(
            "  [{}] {:<22} {}  ({})",
            idx + 1,
            target.name,
            status,
            target.config_path.display()
        );
    }
    println!();

    if scan_only {
        return 0;
    }

    let selected_indices = if all {
        (0..targets.len()).collect::<Vec<_>>()
    } else {
        prompt_selection(&targets)
    };

    if selected_indices.is_empty() {
        println!("No targets selected. Exiting.");
        return 0;
    }

    println!();
    let mut errors = 0;
    for idx in selected_indices {
        let target = &targets[idx];
        match configure_target(target, &blitz_bin) {
            Ok(()) => {
                println!("  ✓ Configured {:<20} -> {}", target.name, target.config_path.display());
            }
            Err(e) => {
                eprintln!("  ✗ Failed to configure {}: {e}", target.name);
                errors += 1;
            }
        }
    }

    println!();
    if errors == 0 {
        println!("Done! Restart your IDE/agent to activate Blitz MCP tools.");
    } else {
        eprintln!("Completed with {errors} error(s).");
    }

    if errors > 0 { 1 } else { 0 }
}

fn prompt_selection(targets: &[IdeTarget]) -> Vec<usize> {
    let total = targets.len();
    print!("Select targets to configure (e.g. '1, 2', 'cursor', 'a' for all, 'q' to quit): ");
    let _ = io::stdout().flush();

    let mut line = String::new();
    if io::stdin().read_line(&mut line).is_err() {
        return Vec::new();
    }

    let trimmed = line.trim().to_lowercase();
    if trimmed == "q" || trimmed == "quit" || trimmed == "exit" || trimmed.is_empty() {
        return Vec::new();
    }
    if trimmed == "a" || trimmed == "all" {
        return (0..total).collect();
    }

    let mut indices = Vec::new();
    for part in trimmed.split(&[',', ' ', ';'][..]) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Ok(num) = part.parse::<usize>() {
            if num >= 1 && num <= total {
                let idx = num - 1;
                if !indices.contains(&idx) {
                    indices.push(idx);
                }
            } else {
                eprintln!("warning: index {num} out of range (1..{total})");
            }
        } else if let Some(idx) = targets.iter().position(|t| {
            t.id.eq_ignore_ascii_case(part)
                || t.name.to_lowercase().contains(part)
        }) {
            if !indices.contains(&idx) {
                indices.push(idx);
            }
        }
    }

    indices
}

fn print_mcp_setup_help() {
    eprintln!(
        "blitz mcp setup — scan and configure IDEs & AI agents for Blitz MCP\n\n\
         USAGE:\n\
             blitz mcp setup [OPTIONS]\n\n\
         OPTIONS:\n\
             -a, --all               Configure all detected IDEs without interactive prompt\n\
             -t, --target <IDE_ID>   Target specific IDE (e.g. cursor, claude_desktop, vscode, zed)\n\
             -s, --scan              Scan and display detected IDEs without modifying configs\n\
             -b, --bin <PATH>        Specify custom Blitz executable path for MCP command\n\
             -h, --help              Show this help message\n\n\
         SUPPORTED IDES:\n\
             • Antigravity / Gemini CLI (~/.gemini/antigravity/mcp_config.json)\n\
             • Cursor (~/.cursor/mcp.json)\n\
             • Claude Desktop (claude_desktop_config.json)\n\
             • Windsurf (~/.codeium/windsurf/mcp_config.json)\n\
             • VS Code User MCP (~/Code/User/mcp.json)\n\
             • Cline Extension (cline_mcp_settings.json)\n\
             • Roo Code Extension (cline_mcp_settings.json)\n\
             • Zed (settings.json context_servers)\n\
             • Claude Code CLI (~/.claude.json)\n"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_json_comments() {
        let input = "{\n  // comment\n  \"key\": \"value\" // end line\n}";
        let stripped = strip_json_comments(input);
        assert!(!stripped.contains("// comment"));
        assert!(stripped.contains("\"key\": \"value\""));
    }

    #[test]
    fn test_configure_target_new_file() {
        let temp_dir = std::env::temp_dir().join(format!("blitz_test_{}", uuid::Uuid::new_v4()));
        let config_path = temp_dir.join("test_mcp.json");

        let target = IdeTarget {
            id: "test",
            name: "Test IDE",
            config_path: config_path.clone(),
            format: McpConfigFormat::McpServers,
            configured: false,
        };

        let fake_bin = PathBuf::from("/usr/local/bin/blitz");
        let res = configure_target(&target, &fake_bin);
        assert!(res.is_ok(), "configure failed: {:?}", res.err());

        let content = fs::read_to_string(&config_path).unwrap();
        let val: Value = serde_json::from_str(&content).unwrap();
        assert_eq!(
            val["mcpServers"]["blitz"]["command"].as_str().unwrap(),
            "/usr/local/bin/blitz"
        );
        assert_eq!(
            val["mcpServers"]["blitz"]["args"][0].as_str().unwrap(),
            "mcp"
        );

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn test_configure_target_preserves_existing() {
        let temp_dir = std::env::temp_dir().join(format!("blitz_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();
        let config_path = temp_dir.join("existing_mcp.json");

        let initial = json!({
            "mcpServers": {
                "other_server": {
                    "command": "python",
                    "args": ["-m", "server"]
                }
            },
            "custom_setting": true
        });
        fs::write(&config_path, serde_json::to_string(&initial).unwrap()).unwrap();

        let target = IdeTarget {
            id: "test",
            name: "Test IDE",
            config_path: config_path.clone(),
            format: McpConfigFormat::McpServers,
            configured: false,
        };

        let fake_bin = PathBuf::from("/Applications/Blitz.app/Contents/MacOS/Blitz");
        let res = configure_target(&target, &fake_bin);
        assert!(res.is_ok());

        let content = fs::read_to_string(&config_path).unwrap();
        let val: Value = serde_json::from_str(&content).unwrap();
        assert!(val["custom_setting"].as_bool().unwrap());
        assert_eq!(
            val["mcpServers"]["other_server"]["command"].as_str().unwrap(),
            "python"
        );
        assert_eq!(
            val["mcpServers"]["blitz"]["command"].as_str().unwrap(),
            "/Applications/Blitz.app/Contents/MacOS/Blitz"
        );

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn test_configure_zed_context_servers() {
        let temp_dir = std::env::temp_dir().join(format!("blitz_test_{}", uuid::Uuid::new_v4()));
        let config_path = temp_dir.join("zed_settings.json");

        let target = IdeTarget {
            id: "zed",
            name: "Zed",
            config_path: config_path.clone(),
            format: McpConfigFormat::ContextServers,
            configured: false,
        };

        let fake_bin = PathBuf::from("/usr/bin/blitz");
        let res = configure_target(&target, &fake_bin);
        assert!(res.is_ok());

        let content = fs::read_to_string(&config_path).unwrap();
        let val: Value = serde_json::from_str(&content).unwrap();
        assert_eq!(
            val["context_servers"]["blitz"]["command"].as_str().unwrap(),
            "/usr/bin/blitz"
        );

        let _ = fs::remove_dir_all(temp_dir);
    }
}
