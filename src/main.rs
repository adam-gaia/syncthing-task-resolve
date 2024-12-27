use clap::Parser;
use color_eyre::eyre::ContextCompat;
use color_eyre::{eyre::bail, Result};
use directories::ProjectDirs;
use jiff::civil::date;
use jiff::tz::TimeZone;
use jiff::{civil::DateTime, Zoned};
use log::{debug, warn};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{DirEntry, File};
use std::io::{stderr, stdout, Write};
use std::ops::Deref;
use std::path::Path;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::{env, fs};
use task_hookrs::task::Task;
use task_hookrs::tw;
use taskchampion::TaskData;
use tempfile::TempDir;
use uuid::Uuid;
mod config;
use config::{Config, DEFAULT_KEEP_NUM};

const THIS_BIN_NAME: &str = env!("CARGO_PKG_NAME");
const PATTERN: &str = r"^taskchampion\.sync-conflict-(\d{8}-\d{6})-([A-Z0-9]{7})\.sqlite3$";
const DATE_FORMAT: &str = "%Y-%m-%d_%H-%M-%S";
const SYNCTHING_DATE_FORMAT: &str = "%Y%m%d-%H%M%S";

#[derive(Debug, Parser)]
struct Cli {
    /// Path to taskwarrior data directory
    #[clap(short, long)]
    task_dir: Option<PathBuf>,

    /// Do not actually make changes, only report what would happen
    #[clap(short, long)]
    dry_run: bool,
}

fn default_task_dir() -> Result<PathBuf> {
    let data_dir = PathBuf::from(std::env::var("XDG_DATA_HOME")?);
    let task_dir = data_dir.join("task");
    Ok(task_dir)
}

#[derive(Debug)]
struct History {
    tasks: HashMap<Uuid, Vec<Task>>,
}
impl History {
    fn new() -> Self {
        Self {
            tasks: HashMap::new(),
        }
    }

    fn insert(&mut self, task: Task) {
        let key = task.uuid();

        if let Some(vals) = self.tasks.get_mut(&key) {
            vals.push(task);
        } else {
            self.tasks.insert(*key, vec![task]);
        }
    }

    fn merge(&self) -> Vec<Task> {
        let num_tasks = self.tasks.len();
        let mut merged_tasks = Vec::with_capacity(num_tasks);

        // Loop over each task, compare the last modified time of the each task's history snapshot and take the one
        // that was most recently modified
        for task in self.tasks.values() {
            let mut history = task.iter();

            // Start with the first task in the history list
            let mut saved = history.next().unwrap();
            let mut modified_time = match saved.modified() {
                Some(m) => m,
                None => {
                    // Fall back to the entry time
                    saved.entry()
                }
            };
            while let Some(next_task) = history.next() {
                let next_modified_time = match next_task.modified() {
                    Some(m) => m,
                    None => {
                        // Fall back to the entry time
                        next_task.entry()
                    }
                };

                // Deref because taskhook_rs::Date holds a Chrono::NativeDateTime
                if next_modified_time.deref() > modified_time.deref() {
                    saved = next_task;
                    modified_time = next_modified_time;
                }
            }

            merged_tasks.push(saved.clone());
        }

        merged_tasks
    }
}

fn main() -> Result<()> {
    color_eyre::install()?;
    env_logger::init();
    let args = Cli::parse();

    let Some(proj_dirs) = ProjectDirs::from("", "", THIS_BIN_NAME) else {
        bail!("Unable to get XDG project dirs");
    };
    let config_dir = proj_dirs.config_dir();
    let config_file = config_dir.join("config.toml");
    let config = if config_file.is_file() {
        let contents = fs::read_to_string(&config_file)?;
        toml::from_str(&contents)?
    } else {
        let config = Config::default();
        let contents = toml::to_string_pretty(&config)?;
        let parent = config_file.parent().unwrap();
        fs::create_dir_all(parent)?;
        fs::write(&config_file, contents)?;
        config
    };

    let Some(state_dir) = proj_dirs.state_dir() else {
        bail!("Unable to get XDG state dir")
    };

    let task_dir = match args.task_dir {
        Some(dir) => dir,
        None => match config.task_dir {
            Some(dir) => dir,
            None => default_task_dir()?,
        },
    };

    let Ok(task_bin) = which::which("task") else {
        bail!("Unable to find taskwarrior binary ('task') on the $PATH");
    };

    let mut conflicts = Vec::new();
    let re = Regex::new(PATTERN).unwrap();
    for entry in fs::read_dir(&task_dir)? {
        let entry = entry?;
        let name = entry.file_name().into_string().unwrap();
        let path = entry.path();
        let ttype = entry.file_type()?;
        if ttype.is_file() {
            if let Some(caps) = re.captures(&name) {
                let timestamp_str = caps.get(1).unwrap().as_str();
                let timestamp = DateTime::strptime(SYNCTHING_DATE_FORMAT, timestamp_str)?;
                let device = caps.get(2).unwrap().as_str().to_owned();
                conflicts.push((timestamp, device, path));
            }
        }
    }

    // Only perform operations if there are conflicts
    if !conflicts.is_empty() {
        // Create a dir to back up conflicted task DBs to prevent data loss
        let timestamp = Zoned::now().with_time_zone(TimeZone::UTC);
        let timestamp = timestamp.strftime(DATE_FORMAT).to_string();
        let action_history_dir = state_dir.join(timestamp);
        fs::create_dir_all(&action_history_dir)?;

        // Also add the main db to list of conflicts, so it is part of our history merging
        let main_db_path = task_dir.join("taskchampion.sqlite3");
        let device = String::from("------"); // Fake device ID, shouldn't matter, we don't use the device ID right now
        let metadata = fs::metadata(&main_db_path)?;
        let modified = metadata.modified()?;
        let timestamp = Zoned::try_from(modified)?;
        let timestamp = DateTime::from(timestamp);
        conflicts.push((timestamp, device, main_db_path.clone()));

        // Sort by timestamp
        conflicts.sort_by_key(|x| x.0);

        // Walk over history, figuring out conflicts
        let mut hist = History::new();
        for (timestamp, device, path) in &conflicts {
            debug!("Timestamp: {}", timestamp);

            let tmp = TempDir::new()?;
            let tmp_dir = tmp.path();
            let dest = tmp_dir.join("taskchampion.sqlite3");
            fs::copy(&path, dest)?;

            // Tell taskwarrior to use the tmpdir to find its DB
            env::set_var("TASKDATA", &tmp_dir);

            // Get all task with an empty query string
            debug!("DB: {}", path.display());
            let tasks = tw::query("")?;
            for task in tasks {
                hist.insert(task);
            }
        }

        // Sort out history conflicts
        let tasks = hist.merge();

        // Save our tasks in a taskchampion database
        let json_tasks = serde_json::to_string(&tasks)?;
        let tmp = TempDir::new()?;
        let tmp_dir = tmp.path();
        env::set_var("TASKDATA", &tmp_dir);
        let Ok(mut child) = Command::new(&task_bin)
            .args(["import"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        else {
            bail!("Unable to run '{}'", &task_bin.display());
        };

        // Pass tasks as json to stdin
        let mut stdin = child.stdin.take().expect("Failed to open stdin");
        std::thread::spawn(move || {
            stdin
                .write_all(json_tasks.as_bytes())
                .expect("Failed to write to stdin");
        });

        let output = child.wait_with_output().expect("Failed to read stdout");
        let stdout = &output.stdout;
        if !stdout.is_empty() {
            let stdout = String::from_utf8_lossy(stdout);
            debug!("task stdout: {}", stdout);
        }
        let stderr = &output.stderr;
        if !stderr.is_empty() {
            let stderr = String::from_utf8_lossy(&stderr);
            warn!("task stderr: {}", stderr);
        }

        // Backup and remove conflict databases (this includes the main db!)
        for (_, _, path) in &conflicts {
            let file_name = path.file_name().unwrap();
            let dest = action_history_dir.join(&file_name);
            debug!("Backing up {}", &dest.display());
            fs::copy(&path, &dest)?;
            fs::remove_file(path)?
        }

        // Replace the main db with the updated tasks
        let updated_db = tmp_dir.join("taskchampion.sqlite3");
        fs::copy(&updated_db, &main_db_path)?;
    }

    // Finally, do a little cleanup in the state dir if we have too many entries
    let num_to_keep = match config.keep {
        Some(n) => n,
        None => DEFAULT_KEEP_NUM,
    };
    let mut entries: Vec<(DateTime, PathBuf)> = fs::read_dir(&state_dir)?
        .map(|x| {
            let path = x.unwrap().path();
            let file_name = path.file_name().unwrap().to_str().unwrap();
            let timestamp = DateTime::strptime(DATE_FORMAT, file_name).unwrap();
            (timestamp, path)
        })
        .collect();
    let num_entries = entries.len();
    if num_entries > num_to_keep {
        let diff = num_entries - num_to_keep;

        // Sort by timestamp
        entries.sort_by_key(|x| x.0);

        // Remove the first n entries to get down to num_to_keep
        let mut iter = entries.iter();
        for _ in 0..diff {
            let (_, path) = iter.next().unwrap();
            fs::remove_dir_all(&path)?;
        }
    }

    Ok(())
}
