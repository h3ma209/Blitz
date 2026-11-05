//! Footer console log lines (request URL, status, script messages).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ConsoleLevel {
    #[default]
    Info,
    Warn,
    Error,
    Script,
    Network,
    Cloud,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsoleLine {
    pub at: DateTime<Utc>,
    pub level: ConsoleLevel,
    pub message: String,
}

impl ConsoleLine {
    pub fn info(msg: impl Into<String>) -> Self {
        Self {
            at: Utc::now(),
            level: ConsoleLevel::Info,
            message: msg.into(),
        }
    }

    pub fn warn(msg: impl Into<String>) -> Self {
        Self {
            at: Utc::now(),
            level: ConsoleLevel::Warn,
            message: msg.into(),
        }
    }

    pub fn error(msg: impl Into<String>) -> Self {
        Self {
            at: Utc::now(),
            level: ConsoleLevel::Error,
            message: msg.into(),
        }
    }

    pub fn script(msg: impl Into<String>) -> Self {
        Self {
            at: Utc::now(),
            level: ConsoleLevel::Script,
            message: msg.into(),
        }
    }

    pub fn network(msg: impl Into<String>) -> Self {
        Self {
            at: Utc::now(),
            level: ConsoleLevel::Network,
            message: msg.into(),
        }
    }

    pub fn cloud(msg: impl Into<String>) -> Self {
        Self {
            at: Utc::now(),
            level: ConsoleLevel::Cloud,
            message: msg.into(),
        }
    }
}

pub const CONSOLE_MAX: usize = 500;

pub fn push_console(lines: &mut Vec<ConsoleLine>, line: ConsoleLine) {
    lines.push(line);
    if lines.len() > CONSOLE_MAX {
        let drain = lines.len() - CONSOLE_MAX;
        lines.drain(0..drain);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_console_levels_and_constructors() {
        let info = ConsoleLine::info("info test");
        assert_eq!(info.level, ConsoleLevel::Info);
        assert_eq!(info.message, "info test");

        let warn = ConsoleLine::warn("warn test");
        assert_eq!(warn.level, ConsoleLevel::Warn);

        let err = ConsoleLine::error("err test");
        assert_eq!(err.level, ConsoleLevel::Error);

        let script = ConsoleLine::script("script test");
        assert_eq!(script.level, ConsoleLevel::Script);

        let net = ConsoleLine::network("GET https://example.com");
        assert_eq!(net.level, ConsoleLevel::Network);

        let cloud = ConsoleLine::cloud("switched to branch blitz-env-prod");
        assert_eq!(cloud.level, ConsoleLevel::Cloud);
    }

    #[test]
    fn test_push_console_max_limit() {
        let mut lines = Vec::new();
        for i in 0..(CONSOLE_MAX + 50) {
            push_console(&mut lines, ConsoleLine::info(format!("msg {i}")));
        }
        assert_eq!(lines.len(), CONSOLE_MAX);
        assert_eq!(lines.first().unwrap().message, "msg 50");
        assert_eq!(lines.last().unwrap().message, format!("msg {}", CONSOLE_MAX + 49));
    }

    #[test]
    fn test_console_level_serde_roundtrip() {
        for level in [
            ConsoleLevel::Info,
            ConsoleLevel::Warn,
            ConsoleLevel::Error,
            ConsoleLevel::Script,
            ConsoleLevel::Network,
            ConsoleLevel::Cloud,
        ] {
            let json = serde_json::to_string(&level).unwrap();
            let de: ConsoleLevel = serde_json::from_str(&json).unwrap();
            assert_eq!(de, level);
        }
    }
}
