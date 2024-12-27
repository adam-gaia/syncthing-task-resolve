use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub const DEFAULT_KEEP_NUM: usize = 100;

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    /// Number of history records to keep in application cache dir
    pub keep: Option<usize>,

    /// If omitted, defaults to taskwarrior's default (${XDG_DATA_HOME}/task/)
    pub task_dir: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            keep: Some(DEFAULT_KEEP_NUM),
            task_dir: None,
        }
    }
}
