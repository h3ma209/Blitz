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

    pub fn script(msg: impl Into<String>) -> Self {
        Self {
            at: Utc::now(),
            level: ConsoleLevel::Script,
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
}

pub const CONSOLE_MAX: usize = 200;

pub fn push_console(lines: &mut Vec<ConsoleLine>, line: ConsoleLine) {
    lines.push(line);
    if lines.len() > CONSOLE_MAX {
        let drain = lines.len() - CONSOLE_MAX;
        lines.drain(0..drain);
    }
}
