# AutomationDesktopApp

Thin SwiftUI client for the local `auto` JSON-RPC daemon.

Start the daemon first:

```bash
auto daemon --socket "$HOME/.local/share/auto/automationd.sock"
swift run --package-path macos/DesktopApp
```

The app owns no scheduler, executor, policy or Registry logic. It only reads
automations/approvals/metrics and requests a named automation run over the
restricted Unix socket.
