// SPDX-License-Identifier: AGPL-3.0-or-later
//
//! The state every region of the interface reads and writes.
//!
//! Signals are `Copy` and the rest is cheap to clone, so a region copies out
//! what it needs. The regions are plain functions called from `app`, so
//! rendering behaves exactly as when they were one big function.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;

use dioxus::prelude::*;

use super::files::{Files, Pending};
use super::inspector::InspTab;
use super::resources::ResTab;
use super::timeline::Drag;
use crate::editor::{self, TimelineView};
use crate::monitor::Monitor;
use crate::{captions, cutout, effects, export};

/// Everything the regions share. Built once per render in `app::app`.
#[derive(Clone)]
pub struct Ctx {
    // ---- The engine and what feeds the window
    /// The edit itself: the engine's editor.
    pub project: Rc<RefCell<concat_project::Editor>>,
    /// The playback thread (composed preview and audio).
    pub monitor: Monitor,
    /// Project file actions (new, open, save, recents, recovery).
    pub files: Files,
    /// The built-in effect catalogue, listed once.
    pub fx_catalogue: Rc<Vec<effects::EffectInfo>>,

    // ---- Player: mirrored from the monitor thread
    pub frame: Signal<Option<String>>,
    pub pos: Signal<f64>,
    pub dur: Signal<f64>,
    pub playing: Signal<bool>,
    pub loading: Signal<bool>,
    pub error: Signal<Option<String>>,

    // ---- Timeline
    /// The edit as the interface draws it, refreshed after every command.
    pub timeline: Signal<TimelineView>,
    /// The height the person gave the panel by dragging its edge (None: follow the window).
    pub tl_height: Signal<Option<f64>>,
    /// The window's inner height in pixels, kept up to date on resize.
    pub win_h: Signal<f64>,
    /// A resize of the timeline panel in progress: (mouse y, height) at the start.
    pub resizing: Signal<Option<(f64, f64)>>,
    pub scrubbing: Signal<bool>,
    /// A clip drag (move or trim) in progress, and the lane under the cursor.
    pub drag: Signal<Option<Drag>>,
    pub drop_lane: Signal<Option<usize>>,
    /// Per-lane interface state, keyed by track id (the engine does not model it).
    pub locked_tracks: Signal<HashSet<String>>,
    pub track_names: Signal<HashMap<String, String>>,
    pub renaming: Signal<Option<String>>,
    /// Bumped when waveform peaks arrive, to redraw the timeline.
    pub wave_rev: Signal<u64>,

    // ---- Resources panel (library, audio, titles, effects, transitions, captions)
    /// The active tab, its active category (0 is "Todo") and the search text.
    pub res_tab: Signal<ResTab>,
    /// The inspector's active tab, and its animation slot (in / out / combined).
    pub insp_tab: Signal<InspTab>,
    /// Timeline zoom in pixels per second, and whether edges snap while dragging.
    pub pps: Signal<f64>,
    pub snap: Signal<bool>,
    /// The player: loop playback, and the full-screen view.
    pub looping: Signal<bool>,
    pub stage_full: Signal<bool>,
    pub anim_slot: Signal<editor::AnimSlot>,
    pub res_cat: Signal<usize>,
    pub res_search: Signal<String>,
    pub media: Signal<Vec<editor::MediaView>>,
    /// The library item being dragged onto the timeline.
    pub dragging_media: Signal<Option<String>>,
    /// Bumped when a thumbnail arrives, to redraw the library.
    pub thumb_rev: Signal<u64>,

    // ---- Selection and its tools
    pub selected: Signal<Option<String>>,
    pub cutout_job: Signal<Option<cutout::Job>>,
    pub cutting: Signal<bool>,
    pub cut_done: Signal<usize>,
    pub cut_total: Signal<usize>,
    pub cc_job: Signal<Option<captions::Job>>,
    pub generating_cc: Signal<bool>,
    pub cc_stage: Signal<String>,

    // ---- Export
    pub export_handle: Signal<Option<export::Handle>>,
    pub exporting: Signal<bool>,
    pub export_stage: Signal<String>,
    pub export_pct: Signal<f64>,
    pub export_msg: Signal<Option<String>>,

    // ---- Project file
    pub project_path: Signal<Option<PathBuf>>,
    pub file_msg: Signal<Option<(bool, String)>>,
    pub recovery_offer: Signal<bool>,
    /// What is waiting on the "unsaved changes" question.
    pub pending: Signal<Option<Pending>>,
    pub menu_open: Signal<bool>,
    /// True when the edit differs from the saved one.
    pub dirty: Memo<bool>,
    /// Local time (`hh:mm:ss`) of the last save or autosave, for the top bar.
    pub saved_at: Signal<Option<String>>,

    // ---- Dialogs
    pub shortcuts_open: Signal<bool>,
}
