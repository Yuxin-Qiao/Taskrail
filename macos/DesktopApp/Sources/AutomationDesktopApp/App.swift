import Foundation
import SwiftUI

@main
struct AutomationDesktopApp: App {
    @StateObject private var model = ControlPlaneModel()

    var body: some Scene {
        WindowGroup("Taskrail") {
            ContentView(model: model)
                .frame(minWidth: 820, minHeight: 560)
        }
    }
}

struct ContentView: View {
    @ObservedObject var model: ControlPlaneModel

    var body: some View {
        NavigationSplitView {
            List(selection: $model.selectedSection) {
                Label("Automations", systemImage: "bolt.circle")
                    .tag(ControlPlaneSection.automations)
                Label("Runs", systemImage: "clock.arrow.circlepath")
                    .tag(ControlPlaneSection.runs)
                Label("Inbox", systemImage: "exclamationmark.bubble")
                    .tag(ControlPlaneSection.inbox)
                Label("Metrics", systemImage: "chart.xyaxis.line")
                    .tag(ControlPlaneSection.metrics)
                Label("Events", systemImage: "list.bullet.rectangle")
                    .tag(ControlPlaneSection.events)
            }
            .navigationTitle("Taskrail")
        } detail: {
            VStack(alignment: .leading, spacing: 0) {
                HStack {
                    VStack(alignment: .leading, spacing: 3) {
                        Text("Local Automation Manager")
                            .font(.title2.weight(.semibold))
                        Text(model.connectionMessage)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                    Spacer()
                    Button("Refresh", systemImage: "arrow.clockwise") {
                        model.refresh()
                    }
                    .keyboardShortcut("r", modifiers: [.command])
                }
                .padding()

                Divider()

                Group {
                    switch model.selectedSection {
                    case .automations:
                        AutomationsView(model: model)
                    case .runs:
                        RunsView(model: model, runs: model.runs)
                    case .inbox:
                        InboxView(items: model.inbox)
                    case .metrics:
                        MetricsView(metrics: model.metrics)
                    case .events:
                        EventsView(events: model.events)
                    }
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            }
        }
        .task {
            model.refresh()
        }
        .alert("Control Plane", isPresented: $model.showingError) {
            Button("OK", role: .cancel) { }
        } message: {
            Text(model.lastError ?? "Unknown error")
        }
        .sheet(item: $model.selectedRunLogs) { logs in
            RunLogsView(logs: logs)
        }
    }
}

struct InboxView: View {
    let items: [InboxItemSummary]

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text("Needs Attention")
                    .font(.headline)
                Spacer()
                Text("\(items.count) items")
                    .foregroundStyle(.secondary)
            }
            .padding(.horizontal)
            if items.isEmpty {
                ContentUnavailableView("Inbox is clear", systemImage: "checkmark.circle", description: Text("No recovery items or failed runs.") )
            } else {
                List(items) { item in
                    VStack(alignment: .leading, spacing: 4) {
                        HStack {
                            Text(item.title)
                                .font(.body.weight(.medium))
                            Spacer()
                            Text(item.severity.uppercased())
                                .font(.caption.weight(.semibold))
                                .foregroundStyle(item.severity == "critical" ? .red : .orange)
                        }
                        Text("\(item.kind) · \(item.status)")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                        if let createdAt = item.createdAt {
                            Text(createdAt)
                                .font(.caption2)
                                .foregroundStyle(.secondary)
                        }
                        Text(item.id)
                            .font(.caption2.monospaced())
                            .foregroundStyle(.secondary)
                    }
                    .padding(.vertical, 4)
                }
                .listStyle(.inset)
            }
        }
        .padding(.top)
    }
}

struct AutomationsView: View {
    @ObservedObject var model: ControlPlaneModel

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text("Automations")
                    .font(.headline)
                Spacer()
                Text("\(model.automations.count) registered")
                    .foregroundStyle(.secondary)
            }
            .padding(.horizontal)
            if model.automations.isEmpty {
                ContentUnavailableView("No automations", systemImage: "bolt.slash", description: Text("Run taskrail scan or register a managed definition."))
            } else {
                List(model.automations) { automation in
                    HStack {
                        VStack(alignment: .leading, spacing: 4) {
                            Text(automation.name)
                                .font(.body.weight(.medium))
                            Text("\(automation.ownership) · \(automation.runtimeState)")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                        Spacer()
                        if automation.runtimeState == "paused" {
                            Button("Resume") {
                                model.setPaused(automation, paused: false)
                            }
                            .buttonStyle(.bordered)
                            .disabled(automation.ownership == "observed")
                        } else {
                            Button("Pause") {
                                model.setPaused(automation, paused: true)
                            }
                            .buttonStyle(.bordered)
                            .disabled(automation.ownership == "observed" || automation.runtimeState == "needs_attention")
                        }
                        Button("Run") {
                            model.run(automation)
                        }
                        .buttonStyle(.bordered)
                        .disabled(automation.ownership == "observed")
                    }
                    .padding(.vertical, 4)
                }
                .listStyle(.inset)
            }
        }
        .padding(.top)
    }
}

struct RunsView: View {
    @ObservedObject var model: ControlPlaneModel
    let runs: [RunRecord]

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Runs")
                .font(.headline)
                .padding(.horizontal)
            if runs.isEmpty {
                ContentUnavailableView("No runs", systemImage: "clock.arrow.circlepath", description: Text("Completed and in-progress automation runs will appear here."))
            } else {
                List(runs) { run in
                    VStack(alignment: .leading, spacing: 4) {
                        HStack {
                            Text(run.automationID)
                                .font(.body.weight(.medium))
                            Spacer()
                            Text(run.status.uppercased())
                                .font(.caption.weight(.semibold))
                                .foregroundStyle(run.status == "succeeded" ? .green : .orange)
                        }
                        Text("Revision \(run.automationRevision) · \(run.startedAt)")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                        Text(run.id)
                            .font(.caption2.monospaced())
                            .foregroundStyle(.secondary)
                        Button("Logs") {
                            model.showLogs(for: run)
                        }
                        .buttonStyle(.bordered)
                        if run.status == "running" {
                            Button("Cancel", role: .destructive) {
                                model.cancel(run)
                            }
                            .buttonStyle(.bordered)
                        }
                    }
                    .padding(.vertical, 4)
                }
                .listStyle(.inset)
            }
        }
        .padding(.top)
    }
}

struct RunLogsView: View {
    let logs: RunLogsSummary

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    Text("\(logs.automationID) · \(logs.status)")
                        .font(.headline)
                    LogBlock(title: "stdout", value: logs.stdout)
                    LogBlock(title: "stderr", value: logs.stderr)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding()
            }
            .navigationTitle("Run Logs")
        }
        .frame(minWidth: 640, minHeight: 420)
    }
}

private struct LogBlock: View {
    let title: String
    let value: String

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(title.uppercased())
                .font(.caption.weight(.semibold))
                .foregroundStyle(.secondary)
            Text(value.isEmpty ? "(empty)" : value)
                .font(.system(.body, design: .monospaced))
                .textSelection(.enabled)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(10)
                .background(.quaternary, in: RoundedRectangle(cornerRadius: 8))
        }
    }
}

struct MetricsView: View {
    let metrics: [MetricSummary]

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Metrics")
                .font(.headline)
                .padding(.horizontal)
            if metrics.isEmpty {
                ContentUnavailableView("No metrics", systemImage: "chart.xyaxis.line", description: Text("Provider usage will appear here when reported."))
            } else {
                List(metrics) { metric in
                    HStack {
                        VStack(alignment: .leading, spacing: 4) {
                            Text(metric.key).font(.body.weight(.medium))
                            Text(metric.source).font(.caption).foregroundStyle(.secondary)
                        }
                        Spacer()
                        Text("\(Int(metric.value)) \(metric.unit)")
                            .monospacedDigit()
                    }
                    .padding(.vertical, 4)
                }
                .listStyle(.inset)
            }
        }
        .padding(.top)
    }
}

struct EventsView: View {
    let events: [EventSummary]

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Events")
                .font(.headline)
                .padding(.horizontal)
            if events.isEmpty {
                ContentUnavailableView("No events", systemImage: "list.bullet.rectangle", description: Text("Run, adoption, and watcher changes will appear here."))
            } else {
                List(events) { event in
                    VStack(alignment: .leading, spacing: 4) {
                        HStack {
                            Text(event.eventType)
                                .font(.body.weight(.medium))
                            Spacer()
                            Text("#\(event.seq)")
                                .font(.caption.monospacedDigit())
                                .foregroundStyle(.secondary)
                        }
                        Text(event.occurredAt)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                        Text(event.payload.keys.sorted().joined(separator: ", "))
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                    }
                    .padding(.vertical, 4)
                }
                .listStyle(.inset)
            }
        }
        .padding(.top)
    }
}
