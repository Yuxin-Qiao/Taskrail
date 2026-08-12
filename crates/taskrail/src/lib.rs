pub mod adoption;
pub mod codex;
pub mod core;
pub mod discovery;
pub mod executor;
pub mod github;
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
