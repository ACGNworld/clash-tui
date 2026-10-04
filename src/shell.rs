use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

pub const BEGIN: &str = "# >>> clash-tui proxy environment >>>";
pub const END: &str = "# <<< clash-tui proxy environment <<<";

fn block(port: u16) -> String {
    format!(
        "{BEGIN}\nexport HTTP_PROXY=http://127.0.0.1:{port}\nexport HTTPS_PROXY=http://127.0.0.1:{port}\nexport ALL_PROXY=http://127.0.0.1:{port}\nexport http_proxy=$HTTP_PROXY\nexport https_proxy=$HTTPS_PROXY\nexport all_proxy=$ALL_PROXY\n{END}"
    )
}

fn replace_block(text: &str, enabled: bool, port: u16) -> Result<String> {
    let start = text.find(BEGIN);
    let end = text.find(END);
    match (start, end) {
        (Some(start), Some(end)) if end >= start => {
            let end = end + END.len();
            let suffix = text[end..].strip_prefix('\n').unwrap_or(&text[end..]);
            let mut output = String::with_capacity(text.len() + 160);
            output.push_str(&text[..start]);
            if enabled {
                output.push_str(&block(port));
                output.push('\n');
            }
            output.push_str(suffix);
            Ok(output)
        }
        (None, None) => {
            if !enabled {
                return Ok(text.to_string());
            }
            let separator = if text.is_empty() || text.ends_with('\n') {
                ""
            } else {
                "\n"
            };
            Ok(format!("{text}{separator}{}\n", block(port)))
        }
        _ => anyhow::bail!("发现不完整的 clash-tui .bashrc 标记，请手动修复"),
    }
}

fn bashrc_path() -> Result<PathBuf> {
    let home = std::env::var_os("HOME").context("无法确定 HOME，不能更新 .bashrc")?;
    Ok(Path::new(&home).join(".bashrc"))
}

pub fn set_proxy_environment(enabled: bool, port: u16) -> Result<()> {
    let path = bashrc_path()?;
    let original = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error).with_context(|| format!("无法读取 {}", path.display())),
    };
    let updated = replace_block(&original, enabled, port)?;
    if updated != original {
        fs::write(&path, updated).with_context(|| format!("无法写入 {}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{replace_block, BEGIN, END};

    #[test]
    fn adds_and_updates_proxy_block_idempotently() {
        let first = replace_block("# user config\n", true, 7890).unwrap();
        assert!(first.contains("export HTTP_PROXY=http://127.0.0.1:7890"));
        assert!(first.contains(BEGIN));
        assert!(first.contains(END));

        let second = replace_block(&first, true, 7891).unwrap();
        assert!(!second.contains("127.0.0.1:7890"));
        assert!(second.contains("127.0.0.1:7891"));
        assert_eq!(replace_block(&second, true, 7891).unwrap(), second);
    }

    #[test]
    fn removes_only_managed_proxy_block() {
        let with_block = replace_block("before\n", true, 7890).unwrap();
        let removed = replace_block(&with_block, false, 7890).unwrap();
        assert_eq!(removed, "before\n");
    }

    #[test]
    fn rejects_incomplete_markers() {
        assert!(replace_block(BEGIN, true, 7890).is_err());
    }
}
