///! This module contains service definitions for RPC system.

use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Debug)]
pub enum RemoteStatus {
    /// Ciel is idle (no active job)
    Idle,
    /// Ciel encountered an irrecoverable error
    Error(String),
    /// Ciel is currently busy running a build job (maintainer, package name, current index, total)
    Busy(String, String, usize, usize),
    /// Ciel is currently busy running a maintenance job (e.g. update-os/clean/config)
    Maint
}

/// Provides Ciel RPC interface abstraction
#[tarpc::service]
pub trait CielService {
    /// Ping
    async fn ping();
    /// Change (remote) settings
    async fn config(apt_sources: String) -> bool;
    /// Akin to `ciel clean`
    async fn clean() -> bool;
    /// Akin to `ciel update-os`
    async fn update_os() -> bool;
    /// Queue a build job. A build job can contain one branch with multiple packages.
    async fn queue_build(maintainer: String, branch: String, packages: Vec<String>) -> bool;
    /// Query the current status of a remote Ciel
    async fn status() -> Option<RemoteStatus>;
}
