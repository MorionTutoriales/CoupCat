// SPDX-License-Identifier: AGPL-3.0-or-later
//
//! The window's interface, split by region. `app::app` owns the state and the
//! background tasks and composes these.

pub mod banners;
pub mod ctx;
pub mod files;
pub mod inspector;
pub mod library;
pub mod stage;
pub mod timeline;
pub mod toolbar;
pub mod topbar;
pub mod util;

pub use ctx::Ctx;

/// The stylesheet, in its own file so it can be read and edited as CSS.
pub const STYLE: &str = include_str!("style.css");
