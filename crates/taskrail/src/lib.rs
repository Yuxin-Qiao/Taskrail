#[cfg(not(any(
    all(target_os = "macos", target_arch = "aarch64"),
    all(target_os = "linux", target_arch = "aarch64", target_env = "gnu")
)))]
compile_error!(
    "Taskrail currently supports only ARM64 macOS (Apple Silicon) and ARM64 Linux targets"
);

pub mod adoption;
pub mod codex;
pub mod core;
pub mod discovery;
pub mod executor;
pub mod fleet;
pub mod github;
pub mod integrations;
pub mod mcp;
pub mod responses;
pub mod rpc;
pub mod scheduler;
pub mod service;
pub mod storage;
pub mod tui;
pub mod verification;
pub mod worktree;

pub use core::{Automation, CommandSpec, DiscoveredSource, Ownership, RunResult};
