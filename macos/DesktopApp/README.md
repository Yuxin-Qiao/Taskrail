# Taskrail Desktop View

This is an optional thin SwiftUI view for the local Taskrail daemon. The
terminal TUI remains the primary interface for the early product.

Start the daemon first:

```bash
taskrail daemon --socket "$HOME/.local/share/taskrail/taskraild.sock"
swift run --package-path macos/DesktopApp
```

The app owns no scheduler, executor, or Registry logic. It reads local
automations, runs, logs, metrics, events, and attention items through the
restricted Unix socket.
