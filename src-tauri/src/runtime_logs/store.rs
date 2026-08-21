use std::{
    collections::{BTreeMap, VecDeque},
    fs::{self, File, OpenOptions},
    io::{self, BufRead as _, BufReader, Write as _},
    path::{Path, PathBuf},
    sync::Mutex,
};

use chrono::Utc;

use super::{ApplicationLogEntry, ApplicationLogLevel, ApplicationLogPage, ApplicationLogQuery};

const MAX_LOG_MESSAGE_CHARS: usize = 65_536;
const MAX_LOG_TARGET_CHARS: usize = 1_024;
const MIN_FILE_BYTES: u64 = 256;
const ROTATION_TARGET_PERCENT: u64 = 75;
const TRUNCATION_SUFFIX: &str = "…[truncated]";

#[derive(Debug)]
pub(crate) struct RuntimeLogStore {
    capacity: usize,
    max_file_bytes: u64,
    state: Mutex<RuntimeLogState>,
}

#[derive(Debug)]
struct RuntimeLogState {
    entries: VecDeque<RetainedLogEntry>,
    // 与实际 JSONL 编码使用同一口径，保证内存保留集和持久化文件共享字节上限。
    retained_bytes: u64,
    next_log_id: u64,
    evicted_count: u64,
    corrupt_line_count: u64,
    persistence_error: Option<String>,
    path: Option<PathBuf>,
    file: Option<File>,
    persisted_bytes: u64,
    persistence_dirty: bool,
    // 仅用于回归验证重写频率，不属于生产状态或对外日志查询合同。
    #[cfg(test)]
    persistence_rewrite_count: u64,
}

#[derive(Debug)]
struct RetainedLogEntry {
    entry: ApplicationLogEntry,
    // 包含行尾换行符；重写文件时必须得到同样的长度。
    jsonl_bytes: u64,
}

impl RuntimeLogStore {
    #[cfg(test)]
    pub(crate) fn memory(capacity: usize) -> Self {
        assert!(capacity > 0, "runtime log capacity must be positive");
        Self {
            capacity,
            max_file_bytes: u64::MAX,
            state: Mutex::new(RuntimeLogState {
                entries: VecDeque::with_capacity(capacity),
                retained_bytes: 0,
                next_log_id: 1,
                evicted_count: 0,
                corrupt_line_count: 0,
                persistence_error: None,
                path: None,
                file: None,
                persisted_bytes: 0,
                persistence_dirty: false,
                #[cfg(test)]
                persistence_rewrite_count: 0,
            }),
        }
    }

    pub(crate) fn open(path: PathBuf, capacity: usize, max_file_bytes: u64) -> io::Result<Self> {
        if capacity == 0 || max_file_bytes < MIN_FILE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "runtime log capacity must be positive and file budget minimum is {MIN_FILE_BYTES} bytes"
                ),
            ));
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut parsed = BTreeMap::new();
        let mut corrupt_line_count = 0_u64;
        let mut highest_id = 0_u64;
        let mut needs_rewrite = false;
        if path.is_file() {
            for line in BufReader::new(File::open(&path)?).lines() {
                if let Some(entry) = line
                    .ok()
                    .and_then(|line| serde_json::from_str::<ApplicationLogEntry>(&line).ok())
                {
                    highest_id = highest_id.max(entry.log_id);
                    if parsed.insert(entry.log_id, entry).is_some() {
                        needs_rewrite = true;
                    }
                } else {
                    corrupt_line_count = corrupt_line_count.saturating_add(1);
                    needs_rewrite = true;
                }
            }
        }
        if highest_id == u64::MAX {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "runtime log id space is exhausted",
            ));
        }
        let mut entries = VecDeque::with_capacity(capacity);
        let mut retained_bytes = 0_u64;
        for entry in parsed.into_values() {
            let jsonl_bytes = encoded_entry(&entry)?.len() as u64;
            if jsonl_bytes > max_file_bytes {
                needs_rewrite = true;
                continue;
            }
            retained_bytes = retained_bytes.saturating_add(jsonl_bytes);
            entries.push_back(RetainedLogEntry { entry, jsonl_bytes });
            while entries.len() > capacity || retained_bytes > max_file_bytes {
                let removed = entries
                    .pop_front()
                    .expect("retention overflow requires one stored entry");
                retained_bytes = retained_bytes.saturating_sub(removed.jsonl_bytes);
                needs_rewrite = true;
            }
        }
        let evicted_count = highest_id.saturating_sub(entries.len() as u64);
        let existing_bytes = path.metadata().map_or(0, |metadata| metadata.len());
        needs_rewrite |= existing_bytes > max_file_bytes;
        let persisted_bytes = if needs_rewrite {
            replace_with_entries(&path, &entries)?;
            retained_bytes
        } else {
            existing_bytes
        };
        let file = append_file(&path)?;
        Ok(Self {
            capacity,
            max_file_bytes,
            state: Mutex::new(RuntimeLogState {
                entries,
                retained_bytes,
                next_log_id: highest_id.saturating_add(1).max(1),
                evicted_count,
                corrupt_line_count,
                persistence_error: None,
                path: Some(path),
                file: Some(file),
                persisted_bytes,
                persistence_dirty: false,
                #[cfg(test)]
                persistence_rewrite_count: u64::from(needs_rewrite),
            }),
        })
    }

    pub(crate) fn record(&self, level: ApplicationLogLevel, target: &str, message: &str) -> u64 {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let log_id = state.next_log_id;
        state.next_log_id = state
            .next_log_id
            .checked_add(1)
            .expect("runtime log id space exhausted after validated startup");
        let (message, message_truncated) = bounded_message(message);
        let entry = ApplicationLogEntry {
            log_id,
            occurred_at: Utc::now(),
            level,
            target: bounded_text(target, MAX_LOG_TARGET_CHARS),
            message,
            message_truncated,
        };
        let (entry, encoded) = fit_entry_to_budget(entry, self.max_file_bytes)
            .expect("validated runtime log file budget must fit one entry");
        let jsonl_bytes = encoded.len() as u64;
        state.retained_bytes = state.retained_bytes.saturating_add(jsonl_bytes);
        state
            .entries
            .push_back(RetainedLogEntry { entry, jsonl_bytes });
        let mut retention_changed = false;
        if state.entries.len() > self.capacity {
            // 条数容量也采用低水位批量淘汰，避免容量满后每新增一条都全量重写文件。
            let rotation_target = count_rotation_target(self.capacity);
            while state.entries.len() > rotation_target {
                evict_oldest(&mut state);
                retention_changed = true;
            }
        }
        if state.retained_bytes > self.max_file_bytes {
            // 超预算时回落到低水位，为后续追加留出空间，避免每写一条都全量重写。
            let rotation_target = self.max_file_bytes.saturating_mul(ROTATION_TARGET_PERCENT) / 100;
            while state.entries.len() > 1 && state.retained_bytes > rotation_target {
                evict_oldest(&mut state);
                retention_changed = true;
            }
        }
        if let Err(error) =
            persist_latest(&mut state, &encoded, self.max_file_bytes, retention_changed)
        {
            state.persistence_error = Some(error.to_string());
            state.persistence_dirty = true;
        } else {
            state.persistence_error = None;
            state.persistence_dirty = false;
        }
        log_id
    }

    pub(crate) fn get(&self, log_id: u64) -> Option<ApplicationLogEntry> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entries
            .iter()
            .find(|stored| stored.entry.log_id == log_id)
            .map(|stored| stored.entry.clone())
    }

    pub(crate) fn query(&self, query: &ApplicationLogQuery) -> ApplicationLogPage {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let target = normalized_filter(query.target.as_deref());
        let keyword = normalized_filter(query.keyword.as_deref());
        let limit = usize::from(query.limit.clamp(1, 500));
        let mut matching = state
            .entries
            .iter()
            .rev()
            .map(|stored| &stored.entry)
            .filter(|entry| {
                query.level.is_none_or(|level| entry.level == level)
                    && query
                        .occurred_from
                        .is_none_or(|from| entry.occurred_at >= from)
                    && query.occurred_to.is_none_or(|to| entry.occurred_at <= to)
                    && query
                        .before_log_id
                        .is_none_or(|before| entry.log_id < before)
                    && target
                        .as_ref()
                        .is_none_or(|needle| entry.target.to_lowercase().contains(needle))
                    && keyword.as_ref().is_none_or(|needle| {
                        entry.message.to_lowercase().contains(needle)
                            || entry.target.to_lowercase().contains(needle)
                    })
            })
            .cloned()
            .collect::<Vec<_>>();
        let has_more = matching.len() > limit;
        matching.truncate(limit);
        ApplicationLogPage {
            rows: matching,
            oldest_retained_log_id: state.entries.front().map(|stored| stored.entry.log_id),
            newest_retained_log_id: state.entries.back().map(|stored| stored.entry.log_id),
            evicted_count: state.evicted_count,
            corrupt_line_count: state.corrupt_line_count,
            has_more,
            persistence_error: state.persistence_error.clone(),
            storage_path: state
                .path
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned()),
            retained_capacity: self.capacity,
            max_file_bytes: self.max_file_bytes,
        }
    }

    #[cfg(test)]
    pub(crate) fn persistence_rewrite_count(&self) -> u64 {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .persistence_rewrite_count
    }
}

fn count_rotation_target(capacity: usize) -> usize {
    let whole_hundreds = capacity / 100;
    let remainder = capacity % 100;
    let rotation_percent =
        usize::try_from(ROTATION_TARGET_PERCENT).expect("rotation target percentage fits usize");
    whole_hundreds
        .saturating_mul(rotation_percent)
        .saturating_add(remainder.saturating_mul(rotation_percent).div_ceil(100))
        .clamp(1, capacity)
}

fn bounded_message(message: &str) -> (String, bool) {
    let count = message.chars().count();
    if count <= MAX_LOG_MESSAGE_CHARS {
        return (message.to_owned(), false);
    }
    let prefix_length = MAX_LOG_MESSAGE_CHARS.saturating_sub(TRUNCATION_SUFFIX.chars().count());
    let mut bounded = message.chars().take(prefix_length).collect::<String>();
    bounded.push_str(TRUNCATION_SUFFIX);
    (bounded, true)
}

fn bounded_text(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_owned();
    }
    let prefix_length = max_chars.saturating_sub(TRUNCATION_SUFFIX.chars().count());
    let mut bounded = value.chars().take(prefix_length).collect::<String>();
    bounded.push_str(TRUNCATION_SUFFIX);
    bounded
}

fn normalized_filter(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_lowercase)
}

fn append_file(path: &Path) -> io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}

fn persist_latest(
    state: &mut RuntimeLogState,
    encoded: &[u8],
    max_file_bytes: u64,
    retention_changed: bool,
) -> io::Result<()> {
    let Some(path) = state.path.clone() else {
        return Ok(());
    };
    if retention_changed
        || state.persistence_dirty
        || state.persisted_bytes.saturating_add(encoded.len() as u64) > max_file_bytes
    {
        state.file.take();
        state.persisted_bytes = replace_with_entries(&path, &state.entries)?;
        #[cfg(test)]
        {
            state.persistence_rewrite_count = state.persistence_rewrite_count.saturating_add(1);
        }
        state.file = Some(append_file(&path)?);
        return Ok(());
    }
    let file = state
        .file
        .as_mut()
        .ok_or_else(|| io::Error::other("runtime log file is unavailable"))?;
    file.write_all(encoded)?;
    file.flush()?;
    state.persisted_bytes = state.persisted_bytes.saturating_add(encoded.len() as u64);
    Ok(())
}

fn replace_with_entries(path: &Path, entries: &VecDeque<RetainedLogEntry>) -> io::Result<u64> {
    let temporary_path = path.with_extension("jsonl.tmp");
    let mut temporary = File::create(&temporary_path)?;
    let mut bytes = 0_u64;
    for stored in entries {
        let encoded = encoded_entry(&stored.entry)?;
        temporary.write_all(&encoded)?;
        bytes = bytes.saturating_add(encoded.len() as u64);
    }
    temporary.sync_all()?;
    drop(temporary);
    replace_file(&temporary_path, path)?;
    Ok(bytes)
}

fn encoded_entry(entry: &ApplicationLogEntry) -> io::Result<Vec<u8>> {
    let mut encoded = serde_json::to_vec(entry).map_err(io::Error::other)?;
    encoded.push(b'\n');
    Ok(encoded)
}

fn fit_entry_to_budget(
    mut entry: ApplicationLogEntry,
    max_file_bytes: u64,
) -> io::Result<(ApplicationLogEntry, Vec<u8>)> {
    let mut encoded = encoded_entry(&entry)?;
    if encoded.len() as u64 <= max_file_bytes {
        return Ok((entry, encoded));
    }

    entry.message_truncated = true;
    entry.message = fit_text_field(&entry.message, |candidate| {
        let mut candidate_entry = entry.clone();
        candidate.clone_into(&mut candidate_entry.message);
        encoded_entry(&candidate_entry).is_ok_and(|bytes| bytes.len() as u64 <= max_file_bytes)
    });
    encoded = encoded_entry(&entry)?;
    if encoded.len() as u64 <= max_file_bytes {
        return Ok((entry, encoded));
    }

    entry.target = fit_text_field(&entry.target, |candidate| {
        let mut candidate_entry = entry.clone();
        candidate.clone_into(&mut candidate_entry.target);
        encoded_entry(&candidate_entry).is_ok_and(|bytes| bytes.len() as u64 <= max_file_bytes)
    });
    encoded = encoded_entry(&entry)?;
    if encoded.len() as u64 > max_file_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("runtime log entry cannot fit {max_file_bytes}-byte file budget"),
        ));
    }
    Ok((entry, encoded))
}

fn fit_text_field<F>(value: &str, fits: F) -> String
where
    F: Fn(&str) -> bool,
{
    let characters = value.chars().collect::<Vec<_>>();
    let mut low = 0_usize;
    let mut high = characters.len();
    let mut best = String::new();
    while low <= high {
        let middle = low + (high - low) / 2;
        let mut candidate = characters[..middle].iter().collect::<String>();
        candidate.push_str(TRUNCATION_SUFFIX);
        if fits(&candidate) {
            best = candidate;
            low = middle.saturating_add(1);
        } else if middle == 0 {
            break;
        } else {
            high = middle - 1;
        }
    }
    best
}

fn evict_oldest(state: &mut RuntimeLogState) {
    if let Some(removed) = state.entries.pop_front() {
        state.retained_bytes = state.retained_bytes.saturating_sub(removed.jsonl_bytes);
        state.evicted_count = state.evicted_count.saturating_add(1);
    }
}

#[cfg(not(windows))]
fn replace_file(temporary_path: &Path, path: &Path) -> io::Result<()> {
    fs::rename(temporary_path, path)
}

#[cfg(windows)]
fn replace_file(temporary_path: &Path, path: &Path) -> io::Result<()> {
    // Windows 不允许覆盖仍存在的目标文件。调用方已释放追加句柄；这里保留旧文件，
    // 只有新文件就位后才删除备份，并在第二次 rename 失败时尽力恢复。
    let backup_path = path.with_extension("jsonl.previous");
    if backup_path.exists() {
        fs::remove_file(&backup_path)?;
    }
    let had_existing = path.exists();
    if had_existing {
        fs::rename(path, &backup_path)?;
    }
    if let Err(error) = fs::rename(temporary_path, path) {
        if had_existing {
            let _ = fs::rename(&backup_path, path);
        }
        return Err(error);
    }
    if had_existing {
        fs::remove_file(backup_path)?;
    }
    Ok(())
}
