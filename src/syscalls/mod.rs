// SPDX-License-Identifier: MIT
//! Syscall wrappers for cntr
//!
//! This module provides low-level syscall wrappers that are not available
//! in the standard library or libc crate.

pub mod capability;
pub(crate) mod mount_api;
pub(crate) mod prctl;

pub(crate) use prctl::prctl;
