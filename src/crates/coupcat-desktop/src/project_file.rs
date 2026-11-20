// SPDX-License-Identifier: AGPL-3.0-or-later
//
//! Project files: save, open, recent projects and crash recovery.
//!
//! A CoupCat project is the engine's own document (`concat.json`, see
//! `concat_project::doc`) saved with the `.coupcat` extension: versioned,
//! migrated on read and tolerant of older or hand-edited files, so nothing is
//! invented here. This module only decides where files live and how they are
//! written.
//!
//! Generated data (background-removal masks) goes in a folder beside the
//! project, `<name>_datos`, so a project folder can be moved or backed up as a
//! whole. A project that was never saved keeps that data in the temp folder.

use std::path::{Path, PathBuf};

use concat_project::{DocumentSettings, Editor};
use serde_json::Value;

/// The project file extension, without the dot.
pub const EXT: &str = "coupcat";

/// How many recent projects are remembered.
const MAX_RECENT: usize = 8;

/// An opened project, plus the media files it names that are gone.
pub struct Loaded {
    pub editor: Editor,
    /// Paths of media the project uses that no longer exist on disk.
    pub missing: Vec<String>,
}

/// CoupCat's per-user data folder (recent list, crash recovery).
fn app_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let dir = base.join("CoupCat");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// The frame and rate of the active timeline, as the document settings.
fn settings(editor: &Editor, name: &str) -> DocumentSettings {
    let v = editor.project().active().video;
    DocumentSettings {
        name: name.to_string(),
        width: v.width,
        height: v.height,
        rate_num: v.rate_num,
        rate_den: v.rate_den,
        canvas: v.canvas,
    }
}

/// The whole edit as compact JSON. Two snapshots being equal means "nothing
/// to save", so this is what the unsaved-changes marker compares.
pub fn snapshot(editor: &Editor) -> String {
    serde_json::to_string(&editor.to_document(&settings(editor, ""))).unwrap_or_default()
}

/// A project's display name: its file name without the extension.
pub fn name_of(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Proyecto".to_string())
}

/// `path` with the project extension, added only when it is not there already
/// (so `video.final` becomes `video.final.coupcat`, not `video.coupcat`).
pub fn with_ext(path: PathBuf) -> PathBuf {
    let has = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(EXT));
    if has {
        return path;
    }
    let mut s = path.into_os_string();
    s.push(format!(".{EXT}"));
    PathBuf::from(s)
}

/// The folder beside a project file that holds its generated data.
pub fn data_dir(path: &Path) -> PathBuf {
    path.with_file_name(format!("{}_datos", name_of(path)))
}

/// Write `text` to `path` through a temp file and a rename, so a crash in the
/// middle of a save can never leave a half-written project behind.
fn write_atomic(path: &Path, text: &str) -> Result<(), String> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("{}: {e}", path.display())
    })
}

/// Save the project to `path`.
pub fn save(editor: &Editor, path: &Path) -> Result<(), String> {
    let doc = editor.to_document(&settings(editor, &name_of(path)));
    let text = serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())?;
    write_atomic(path, &text)
}

/// Read a project file, reporting the media it names that is no longer there.
pub fn load(path: &Path) -> Result<Loaded, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    // Windows editors (Notepad, PowerShell) may prefix the file with a UTF-8
    // byte-order mark, which JSON parsers reject; it is not part of the data.
    let doc: Value = serde_json::from_str(text.trim_start_matches('\u{feff}'))
        .map_err(|_| "El archivo no es un proyecto de CoupCat (JSON no válido).".to_string())?;
    let editor = Editor::from_document(&doc).ok_or_else(|| {
        "El archivo no es un proyecto válido, o lo creó una versión más nueva de CoupCat."
            .to_string()
    })?;
    let missing = editor
        .project()
        .media
        .iter()
        .filter(|m| !Path::new(&m.path).exists())
        .map(|m| m.path.clone())
        .collect();
    Ok(Loaded { editor, missing })
}

/// Copy a folder tree (used to carry generated data to a project's new home).
/// A missing source is not an error: there is just nothing to carry.
pub fn copy_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
    if !src.is_dir() {
        return Ok(());
    }
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &to)?;
        } else {
            std::fs::copy(entry.path(), to)?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- recents

fn recent_file() -> PathBuf {
    app_dir().join("recientes.txt")
}

/// Recent projects, newest first, leaving out the ones that no longer exist.
pub fn recents() -> Vec<PathBuf> {
    std::fs::read_to_string(recent_file())
        .unwrap_or_default()
        .lines()
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .take(MAX_RECENT)
        .collect()
}

/// Put `path` at the top of the recent list.
pub fn push_recent(path: &Path) {
    let mut list = recents();
    list.retain(|p| p != path);
    list.insert(0, path.to_path_buf());
    list.truncate(MAX_RECENT);
    let text = list
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("\n");
    let _ = std::fs::write(recent_file(), text);
}

// --------------------------------------------------------------- recovery

fn recovery_file() -> PathBuf {
    app_dir().join(format!("recuperacion.{EXT}"))
}

/// Whether an earlier session left unsaved work behind.
pub fn has_recovery() -> bool {
    recovery_file().is_file()
}

/// Write the autosave copy of the project.
pub fn write_recovery(editor: &Editor) -> Result<(), String> {
    let doc = editor.to_document(&settings(editor, "Recuperado"));
    let text = serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())?;
    write_atomic(&recovery_file(), &text)
}

/// Read the autosave copy.
pub fn load_recovery() -> Result<Loaded, String> {
    load(&recovery_file())
}

/// Drop the autosave copy (the work is saved, or was thrown away on purpose).
pub fn clear_recovery() {
    let _ = std::fs::remove_file(recovery_file());
}
