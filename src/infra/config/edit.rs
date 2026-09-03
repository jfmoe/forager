use std::collections::HashSet;
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use fs2::FileExt;
use tempfile::{Builder, NamedTempFile};
use toml_edit::{Array, DocumentMut, InlineTable, Item, Table, TableLike, Value};

use crate::secure_fs::{ensure_private_directory, restrict_private_file};

use super::load::diagnostic_without_source;
use super::location::{ConfigError, ConfigLocation, EditError};
use super::schema::{Config, LEAVES, ValueKind, env_name, is_leaf, leaf, parse_integer, path_kind};
use super::validate::{invalid_value, validate_edit_value};

const LOCK_WAIT: Duration = Duration::from_millis(100);

/// Sets one schema leaf in the file layer without strictly loading other keys.
///
/// # Errors
///
/// Returns an argument error for an invalid target or value, and a
/// configuration error when the document cannot be parsed or written.
pub fn set_file_value(path: &str, raw: &str) -> Result<(), EditError> {
    let value = parse_edit_value(path, raw)?;
    let location = ConfigLocation::discover().map_err(EditError::Config)?;
    let file = location.config_file();
    let _lock = acquire_location_lock(&location).map_err(EditError::Config)?;
    let content = read_edit_document(&file)?;
    let mut document = parse_edit_document(&file, &content)?;
    set_document_path(&mut document, path, value)?;
    atomic_write(&location.config_dir, &file, document.to_string().as_bytes())
        .map_err(|error| EditError::Config(ConfigError::io(&file, error)))
}

/// Removes one schema leaf from the file layer without strictly loading it.
///
/// Returns whether an environment value for the same leaf remains effective.
///
/// # Errors
///
/// Returns an argument error for an invalid target and a configuration error
/// when the document cannot be parsed or written.
pub fn unset_file_value(path: &str) -> Result<bool, EditError> {
    if !is_leaf(path) {
        return Err(EditError::Argument(format!(
            "unknown configuration key `{path}`"
        )));
    }
    let location = ConfigLocation::discover().map_err(EditError::Config)?;
    let file = location.config_file();
    let _lock = acquire_location_lock(&location).map_err(EditError::Config)?;
    let content = read_edit_document(&file)?;
    let mut document = parse_edit_document(&file, &content)?;
    remove_document_path(&mut document, path)?;
    atomic_write(&location.config_dir, &file, document.to_string().as_bytes())
        .map_err(|error| EditError::Config(ConfigError::io(&file, error)))?;
    Ok(env::var_os(env_name(path)).is_some())
}

/// A parseable configuration document being updated by the setup wizard.
pub struct SetupDocument {
    _lock: File,
    location: ConfigLocation,
    file: PathBuf,
    document: DocumentMut,
    defaults: DocumentMut,
}

impl SetupDocument {
    /// Loads the existing document without validating unrelated schema values.
    ///
    /// # Errors
    ///
    /// Returns a configuration error when the file cannot be read or parsed.
    pub fn load() -> Result<Self, EditError> {
        let location = ConfigLocation::discover().map_err(EditError::Config)?;
        let file = location.config_file();
        let lock = acquire_location_lock(&location).map_err(EditError::Config)?;
        let content = read_edit_document(&file)?;
        let document = parse_edit_document(&file, &content)?;
        let defaults = default_document().map_err(EditError::Config)?;
        Ok(Self {
            _lock: lock,
            location,
            file,
            document,
            defaults,
        })
    }

    /// Returns the first configured search backend, falling back to `xai`.
    #[must_use]
    pub fn primary_backend(&self) -> &str {
        document_array(&self.document, "search.backends")
            .and_then(|array| array.iter().find_map(Value::as_str))
            .filter(|backend| matches!(*backend, "xai" | "openai_compatible"))
            .unwrap_or("xai")
    }

    /// Returns a string leaf from the file or built-in defaults.
    #[must_use]
    pub fn string(&self, path: &str) -> &str {
        document_string(&self.document, path)
            .or_else(|| document_string(&self.defaults, path))
            .unwrap_or_default()
    }

    /// Returns whether the existing file contains classifier model configuration.
    #[must_use]
    pub fn classifier_is_configured(&self) -> bool {
        ["classifier.url", "classifier.model"].iter().any(|path| {
            document_string(&self.document, path).is_some_and(|value| !value.is_empty())
        }) || document_array(&self.document, "classifier.keys").is_some_and(|keys| {
            keys.iter()
                .any(|key| key.as_str().is_some_and(|key| !key.is_empty()))
        })
    }

    /// Updates a string leaf in memory.
    ///
    /// # Errors
    ///
    /// Returns an argument error when the path or value is invalid.
    pub fn set_string(&mut self, path: &str, value: &str) -> Result<(), EditError> {
        let value = parse_edit_value(path, value)?;
        set_document_path(&mut self.document, path, value)
    }

    /// Updates a string-array leaf in memory.
    ///
    /// # Errors
    ///
    /// Returns an argument error when the path or values are invalid.
    pub fn set_strings(&mut self, path: &str, values: &[String]) -> Result<(), EditError> {
        if !matches!(path_kind(path), Some(ValueKind::Array)) {
            return Err(invalid_value(path));
        }
        let mut array = Array::new();
        for value in values {
            array.push(value.as_str());
        }
        let value = if config_leaf(path) == "keys" {
            Value::Array(normalize_array(&array))
        } else {
            Value::Array(array)
        };
        validate_edit_value(path, &value)?;
        set_document_path(&mut self.document, path, value)
    }

    /// Persists all wizard changes with one atomic replacement.
    ///
    /// # Errors
    ///
    /// Returns a configuration error when the document cannot be written.
    pub fn save(self) -> Result<(), EditError> {
        atomic_write(
            &self.location.config_dir,
            &self.file,
            self.document.to_string().as_bytes(),
        )
        .map_err(|error| EditError::Config(ConfigError::io(&self.file, error)))
    }
}

/// Creates the complete commented configuration template without overwriting.
///
/// # Errors
///
/// Returns a configuration error when the target exists or cannot be created.
pub fn create_setup_template() -> Result<PathBuf, ConfigError> {
    let location = ConfigLocation::discover()?;
    let file = location.config_file();
    let _lock = acquire_location_lock(&location)?;
    if file
        .try_exists()
        .map_err(|error| ConfigError::io(&file, error))?
    {
        return Err(ConfigError::Message(format!(
            "{} already exists; refusing to overwrite",
            file.display()
        )));
    }

    let document = default_document()?;
    let template = commented_template(&document)?;
    atomic_create(&location.config_dir, &file, template.as_bytes())
        .map_err(|error| ConfigError::io(&file, error))?;
    Ok(file)
}

fn default_document() -> Result<DocumentMut, ConfigError> {
    toml::to_string(&Config::default())
        .map_err(|error| ConfigError::Message(error.to_string()))?
        .parse::<DocumentMut>()
        .map_err(|error| ConfigError::Message(error.to_string()))
}

fn document_item<'a>(document: &'a DocumentMut, path: &str) -> Option<&'a Item> {
    let mut table: &dyn TableLike = document.as_table();
    let mut segments = path.split('.').peekable();
    while let Some(segment) = segments.next() {
        let item = table.get(segment)?;
        if segments.peek().is_none() {
            return Some(item);
        }
        table = item.as_table_like()?;
    }
    None
}

fn document_string<'a>(document: &'a DocumentMut, path: &str) -> Option<&'a str> {
    document_item(document, path)?.as_str()
}

fn document_array<'a>(document: &'a DocumentMut, path: &str) -> Option<&'a Array> {
    document_item(document, path)?.as_array()
}

pub(super) fn config_leaf(path: &str) -> &str {
    path.rsplit_once('.').map_or(path, |(_, leaf)| leaf)
}

fn commented_template(document: &DocumentMut) -> Result<String, ConfigError> {
    let mut table = String::new();
    let mut template = String::new();
    let mut annotated = 0;
    for line in document.to_string().lines() {
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|line| line.strip_suffix(']'))
        {
            table.clear();
            table.push_str(name);
        } else if let Some((key, _)) = line.split_once(" = ") {
            let path = if table.is_empty() {
                key.to_owned()
            } else {
                format!("{table}.{key}")
            };
            if !is_leaf(&path) {
                return Err(ConfigError::Message(format!(
                    "built-in configuration contains unknown `{path}`"
                )));
            }
            template.push_str("# ");
            template.push_str(&template_comment(&path));
            template.push('\n');
            annotated += 1;
        }
        template.push_str(line);
        template.push('\n');
    }
    if annotated != LEAVES.len() {
        return Err(ConfigError::Message(
            "built-in configuration does not cover the complete key surface".into(),
        ));
    }
    Ok(template)
}

fn template_comment(path: &str) -> String {
    let purpose = leaf(path).map_or("configuration value", |leaf| leaf.comment);
    format!("{path}: {purpose}")
}

fn read_edit_document(path: &Path) -> Result<String, EditError> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(content),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(EditError::Config(ConfigError::io(path, error))),
    }
}

fn parse_edit_document(path: &Path, content: &str) -> Result<DocumentMut, EditError> {
    content.parse::<DocumentMut>().map_err(|error| {
        EditError::Config(ConfigError::Document {
            path: path.to_path_buf(),
            detail: diagnostic_without_source(&error.to_string()),
        })
    })
}

fn set_document_path(
    document: &mut DocumentMut,
    path: &str,
    value: Value,
) -> Result<(), EditError> {
    let segments = path.split('.').collect::<Vec<_>>();
    set_table_path(document.as_table_mut(), false, &segments, value, path)
}

fn set_table_path(
    table: &mut dyn TableLike,
    inline: bool,
    segments: &[&str],
    value: Value,
    path: &str,
) -> Result<(), EditError> {
    let Some((segment, remaining)) = segments.split_first() else {
        return Err(EditError::Argument(format!(
            "unknown configuration key `{path}`"
        )));
    };
    if remaining.is_empty() {
        table.insert(segment, Item::Value(value));
        return Ok(());
    }
    let item = table.entry(segment).or_insert_with(|| {
        if inline {
            Item::Value(Value::InlineTable(InlineTable::new()))
        } else {
            Item::Table(Table::new())
        }
    });
    match item {
        Item::Table(table) => set_table_path(table, false, remaining, value, path),
        Item::Value(Value::InlineTable(table)) => {
            set_table_path(table, true, remaining, value, path)
        }
        _ => Err(non_table_path(path, segment)),
    }
}

fn remove_document_path(document: &mut DocumentMut, path: &str) -> Result<bool, EditError> {
    let segments = path.split('.').collect::<Vec<_>>();
    remove_table_path(document.as_table_mut(), &segments, path)
}

fn remove_table_path(
    table: &mut dyn TableLike,
    segments: &[&str],
    path: &str,
) -> Result<bool, EditError> {
    let Some((segment, remaining)) = segments.split_first() else {
        return Ok(false);
    };
    if remaining.is_empty() {
        return Ok(table.remove(segment).is_some());
    }
    let Some(item) = table.get_mut(segment) else {
        return Ok(false);
    };
    match item {
        Item::Table(table) => remove_table_path(table, remaining, path),
        Item::Value(Value::InlineTable(table)) => remove_table_path(table, remaining, path),
        _ => Err(non_table_path(path, segment)),
    }
}

fn non_table_path(path: &str, segment: &str) -> EditError {
    EditError::Config(ConfigError::Message(format!(
        "cannot edit `{path}` because `{segment}` is not a table"
    )))
}

pub(super) fn parse_edit_value(path: &str, raw: &str) -> Result<Value, EditError> {
    if !is_leaf(path) {
        return Err(EditError::Argument(format!(
            "unknown configuration key `{path}`"
        )));
    }
    let value = match path_kind(path) {
        Some(ValueKind::String) => Value::from(raw),
        Some(ValueKind::Boolean) => raw
            .parse::<bool>()
            .map(Value::from)
            .map_err(|_| invalid_value(path))?,
        Some(ValueKind::Integer) => parse_integer(raw)
            .map_err(|()| invalid_value(path))
            .and_then(|value| i64::try_from(value).map_err(|_| invalid_value(path)))
            .map(Value::from)?,
        Some(ValueKind::Float) => raw
            .parse::<f64>()
            .map(Value::from)
            .map_err(|_| invalid_value(path))?,
        Some(ValueKind::Array) => {
            let document = format!("value = {raw}")
                .parse::<DocumentMut>()
                .map_err(|_| invalid_value(path))?;
            let array = document["value"]
                .as_array()
                .cloned()
                .ok_or_else(|| invalid_value(path))?;
            if array.iter().any(|item| item.as_str().is_none()) {
                return Err(invalid_value(path));
            }
            if config_leaf(path) == "keys" {
                Value::Array(normalize_array(&array))
            } else {
                Value::Array(array)
            }
        }
        None => return Err(invalid_value(path)),
    };
    validate_edit_value(path, &value)?;
    Ok(value)
}

fn normalize_array(array: &Array) -> Array {
    let mut seen = HashSet::new();
    array
        .iter()
        .filter_map(Value::as_str)
        .filter(|value| !value.is_empty())
        .filter(|value| seen.insert((*value).to_owned()))
        .fold(Array::new(), |mut normalized, value| {
            normalized.push(value);
            normalized
        })
}

fn atomic_write(config_dir: &Path, destination: &Path, bytes: &[u8]) -> io::Result<()> {
    write_private_tempfile(config_dir, bytes)?
        .persist(destination)
        .map_err(|error| error.error)?;
    restrict_private_file(destination)
}

fn atomic_create(config_dir: &Path, destination: &Path, bytes: &[u8]) -> io::Result<()> {
    write_private_tempfile(config_dir, bytes)?
        .persist_noclobber(destination)
        .map_err(|error| error.error)?;
    restrict_private_file(destination)
}

fn write_private_tempfile(config_dir: &Path, bytes: &[u8]) -> io::Result<NamedTempFile> {
    ensure_private_directory(config_dir)?;
    let mut temporary = Builder::new()
        .prefix(".config.toml.")
        .suffix(".tmp")
        .tempfile_in(config_dir)?;
    restrict_private_file(temporary.path())?;
    temporary.write_all(bytes)?;
    temporary.flush()?;
    temporary.as_file().sync_all()?;
    Ok(temporary)
}

fn acquire_config_lock(config_dir: &Path) -> io::Result<File> {
    ensure_private_directory(config_dir)?;
    let lock = open_private_lock(&config_dir.join(".config.lock"))?;
    let deadline = Instant::now() + LOCK_WAIT;
    loop {
        match lock.try_lock_exclusive() {
            Ok(()) => return Ok(lock),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err(io::Error::new(
                        io::ErrorKind::WouldBlock,
                        "config lock timed out",
                    ));
                }
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => return Err(error),
        }
    }
}

fn acquire_location_lock(location: &ConfigLocation) -> Result<File, ConfigError> {
    let lock_path = location.config_dir.join(".config.lock");
    acquire_config_lock(&location.config_dir).map_err(|error| ConfigError::io(&lock_path, error))
}

fn open_private_lock(path: &Path) -> io::Result<File> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)?;
    restrict_private_file(path)?;
    Ok(file)
}
