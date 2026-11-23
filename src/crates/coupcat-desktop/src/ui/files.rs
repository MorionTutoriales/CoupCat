// SPDX-License-Identifier: AGPL-3.0-or-later
//
//! Project file actions: new, open, save, recents and recovery, plus the
//! "unsaved changes" question that guards them.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;

use dioxus::prelude::*;

use crate::editor::{self, TimelineView};
use crate::monitor::{self, Cmd, Monitor};
use crate::{cutout, project_file};

/// Something that replaces the open project; asks about unsaved changes first.
#[derive(Clone, PartialEq)]
pub enum Pending {
    /// Start an empty project.
    New,
    /// Pick a project file and open it.
    Open,
    /// Open this recent project.
    Recent(PathBuf),
    /// Restore the autosaved work of a previous session.
    Recover,
}

/// Everything the project-file actions touch, bundled so each button can clone
/// one value instead of a dozen. Signals are `Copy`; the rest is cheap to clone.
#[derive(Clone)]
pub struct Files {
    pub project: Rc<RefCell<concat_project::Editor>>,
    pub monitor: Monitor,
    pub media: Signal<Vec<editor::MediaView>>,
    pub timeline: Signal<TimelineView>,
    pub selected: Signal<Option<String>>,
    /// The project file on disk, once the project was saved or opened.
    pub project_path: Signal<Option<PathBuf>>,
    /// The project as last saved/opened/created: "unsaved changes" is any
    /// difference from this.
    pub saved_snap: Signal<String>,
    /// A message about the last file action: `(ok, text)`.
    pub file_msg: Signal<Option<(bool, String)>>,
    pub recovery_offer: Signal<bool>,
    pub pending: Signal<Option<Pending>>,
    pub busy: Signal<bool>,
    /// Per-lane interface state, keyed by track id. Every project starts with
    /// the same ids (`T1`…`T4`), so it must not carry over to another project.
    pub locked_tracks: Signal<HashSet<String>>,
    pub track_names: Signal<HashMap<String, String>>,
    pub renaming: Signal<Option<String>>,
    /// Local time of the last save or autosave (see `util::now_hms`).
    pub saved_at: Signal<Option<String>>,
}

impl Files {
    /// Record "now" as the time of the last save, for the top bar's indicator.
    pub fn stamp_saved(&self) {
        let mut saved_at = self.saved_at;
        spawn(async move {
            if let Some(t) = super::util::now_hms().await {
                saved_at.set(Some(t));
            }
        });
    }

    /// Forget the lane locks, custom names and any rename in progress.
    pub fn reset_lane_state(&self) {
        let (mut locked, mut names, mut renaming) =
            (self.locked_tracks, self.track_names, self.renaming);
        locked.set(HashSet::new());
        names.set(HashMap::new());
        renaming.set(None);
    }

    /// Show the (new) project: library, timeline and the monitor.
    pub fn sync(&self) {
        // Only ever called when the project is replaced (open, new, recover).
        self.reset_lane_state();
        let mut saved_at = self.saved_at;
        saved_at.set(None);
        let (mut media, mut timeline, mut selected) = (self.media, self.timeline, self.selected);
        media.set(editor::media_list(&self.project.borrow()));
        timeline.set(editor::view(&self.project.borrow()));
        selected.set(None);
        match monitor::timeline_data(&self.project.borrow()) {
            Some(d) => {
                self.monitor.send(Cmd::SetTimeline(d));
                self.monitor.send(Cmd::Seek(0.0));
            }
            None => self.monitor.send(Cmd::Clear),
        }
    }

    pub fn say(&self, ok: bool, text: String) {
        let mut file_msg = self.file_msg;
        file_msg.set(Some((ok, text)));
    }

    /// Mark the current edit as the saved one.
    pub fn mark_saved(&self) {
        let mut saved_snap = self.saved_snap;
        saved_snap.set(project_file::snapshot(&self.project.borrow()));
    }

    /// Replace the open project with `loaded`, from `path` (or unsaved).
    pub fn install(&self, loaded: project_file::Loaded, path: Option<PathBuf>) {
        *self.project.borrow_mut() = loaded.editor;
        cutout::set_project_dir(path.as_deref().map(project_file::data_dir));
        let mut project_path = self.project_path;
        project_path.set(path.clone());
        self.sync();
        match &path {
            Some(p) => {
                self.mark_saved();
                project_file::push_recent(p);
            }
            // Recovered work was never saved: leave it marked as changed.
            None => {
                let mut saved_snap = self.saved_snap;
                saved_snap.set(project_file::snapshot(&concat_project::Editor::new()));
            }
        }
        if loaded.missing.is_empty() {
            self.say(
                true,
                match &path {
                    Some(p) => format!("Proyecto abierto: {}", project_file::name_of(p)),
                    None => {
                        "Trabajo recuperado. Guárdalo con «Guardar» para conservarlo.".to_string()
                    }
                },
            );
        } else {
            let names: Vec<String> = loaded
                .missing
                .iter()
                .take(3)
                .map(|m| {
                    std::path::Path::new(m)
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| m.clone())
                })
                .collect();
            let more = loaded.missing.len().saturating_sub(names.len());
            self.say(false, format!(
                "Faltan {} archivo(s) de medios: {}{}. Los clips que los usan no se verán hasta que los devuelvas a su sitio.",
                loaded.missing.len(),
                names.join(", "),
                if more > 0 { format!(" y {more} más") } else { String::new() },
            ));
        }
    }

    pub fn new_project(&self) {
        *self.project.borrow_mut() = concat_project::Editor::new();
        cutout::set_project_dir(None);
        let mut project_path = self.project_path;
        project_path.set(None);
        self.sync();
        self.mark_saved();
        project_file::clear_recovery();
        self.say(true, "Proyecto nuevo.".to_string());
    }

    pub fn open_path(&self, path: PathBuf) {
        match project_file::load(&path) {
            Ok(loaded) => self.install(loaded, Some(path)),
            Err(e) => self.say(false, format!("No se pudo abrir el proyecto: {e}")),
        }
    }

    pub fn recover(&self) {
        match project_file::load_recovery() {
            Ok(loaded) => self.install(loaded, None),
            Err(e) => self.say(false, format!("No se pudo recuperar el trabajo: {e}")),
        }
        let mut offer = self.recovery_offer;
        offer.set(false);
    }

    /// Save to `path`, carrying the generated data along if the folder changes.
    pub fn save_to(&self, path: PathBuf) -> bool {
        let new_dir = project_file::data_dir(&path);
        let old_dir = cutout::project_dir();
        if old_dir != new_dir {
            if let Err(e) = project_file::copy_dir(&old_dir, &new_dir) {
                self.say(
                    false,
                    format!("No se pudieron copiar los datos del proyecto: {e}"),
                );
                return false;
            }
            cutout::set_project_dir(Some(new_dir));
        }
        match project_file::save(&self.project.borrow(), &path) {
            Ok(()) => {
                let mut project_path = self.project_path;
                project_path.set(Some(path.clone()));
                self.mark_saved();
                self.stamp_saved();
                project_file::push_recent(&path);
                project_file::clear_recovery();
                self.say(true, format!("Guardado: {}", project_file::name_of(&path)));
                true
            }
            Err(e) => {
                self.say(false, format!("No se pudo guardar: {e}"));
                false
            }
        }
    }

    pub async fn save_as(&self) -> bool {
        let current = (self.project_path)()
            .map(|p| project_file::name_of(&p))
            .unwrap_or_else(|| "proyecto".to_string());
        let Some(file) = rfd::AsyncFileDialog::new()
            .add_filter("Proyecto de CoupCat", &[project_file::EXT])
            .set_title("Guardar proyecto")
            .set_file_name(format!("{current}.{}", project_file::EXT))
            .save_file()
            .await
        else {
            return false;
        };
        self.save_to(project_file::with_ext(file.path().to_path_buf()))
    }

    /// Save to the project's file, asking for one the first time.
    pub async fn save(&self) -> bool {
        match (self.project_path)() {
            Some(path) => self.save_to(path),
            None => self.save_as().await,
        }
    }

    pub async fn open_dialog(&self) {
        let Some(file) = rfd::AsyncFileDialog::new()
            .add_filter("Proyecto de CoupCat", &[project_file::EXT])
            .set_title("Abrir proyecto")
            .pick_file()
            .await
        else {
            return;
        };
        self.open_path(file.path().to_path_buf());
    }

    /// Do `action` now, replacing the open project.
    pub async fn run(&self, action: Pending) {
        match action {
            Pending::New => self.new_project(),
            Pending::Open => self.open_dialog().await,
            Pending::Recent(path) => self.open_path(path),
            Pending::Recover => self.recover(),
        }
    }

    /// Save (or save as) from a button, one dialog at a time.
    pub fn spawn_save(&self, as_new: bool) {
        let me = self.clone();
        spawn(async move {
            if (me.busy)() {
                return;
            }
            let mut busy = me.busy;
            busy.set(true);
            if as_new {
                me.save_as().await;
            } else {
                me.save().await;
            }
            busy.set(false);
        });
    }

    /// The unsaved-changes dialog: save first (`true`) or discard (`false`),
    /// then do what was asked. A cancelled save keeps the dialog open.
    pub fn resolve(&self, save_first: bool) {
        let me = self.clone();
        spawn(async move {
            let mut pending = me.pending;
            let Some(action) = pending() else { return };
            if (me.busy)() {
                return;
            }
            let mut busy = me.busy;
            busy.set(true);
            let go = if save_first { me.save().await } else { true };
            if go {
                pending.set(None);
                me.run(action).await;
            }
            busy.set(false);
        });
    }

    /// Ask to do `action`: straight away when nothing is unsaved, otherwise
    /// after the person has chosen what to do with the changes.
    pub fn request(&self, action: Pending, dirty: bool) {
        if dirty {
            let mut pending = self.pending;
            pending.set(Some(action));
            return;
        }
        let me = self.clone();
        spawn(async move {
            if (me.busy)() {
                return;
            }
            let mut busy = me.busy;
            busy.set(true);
            me.run(action).await;
            busy.set(false);
        });
    }
}
