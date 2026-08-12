import Foundation
import SwiftUI

enum ControlPlaneSection: Hashable {
    case automations
    case discovery
    case integrations
    case approvals
    case runs
    case inbox
    case metrics
    case events
}

struct AutomationSummary: Codable, Identifiable {
    let id: String
    let name: String
    let ownership: String
    let runtimeState: String
    let nextRunAt: String?
    let steps: [[String: AnyCodable]]?

    enum CodingKeys: String, CodingKey {
        case id, name, ownership
        case runtimeState = "runtime_state"
        case nextRunAt = "next_run_at"
        case steps
    }
}

struct DiscoverySourceSummary: Codable, Identifiable {
    let sourceID: String
    let nativeID: String
    let provider: String
    let kind: String
    let enabled: Bool
    let path: String?
    let trigger: [String: AnyCodable]?

    var id: String { sourceID }

    enum CodingKeys: String, CodingKey {
        case sourceID = "source_id"
        case nativeID = "native_id"
        case provider, kind, enabled, path, trigger
    }
}

struct IntegrationCapabilitySummary: Codable, Identifiable {
    let action: String
    let risk: String
    let supportsDryRun: Bool

    var id: String { "\(action)-\(risk)" }

    enum CodingKeys: String, CodingKey {
        case action, risk
        case supportsDryRun = "supports_dry_run"
    }
}

struct IntegrationDescriptorSummary: Codable, Identifiable {
    let id: String
    let displayName: String
    let level: String
    let capabilities: [IntegrationCapabilitySummary]

    enum CodingKeys: String, CodingKey {
        case id
        case displayName = "display_name"
        case level, capabilities
    }
}

struct IntegrationDetectionSummary: Codable, Identifiable {
    let integration: String
    let status: String
    let executable: String?
    let version: String?
    let detail: String?

    var id: String { integration }
}

struct IntegrationDoctorCheckSummary: Codable, Identifiable {
    let name: String
    let ok: Bool
    let detail: String

    var id: String { name }
}

struct IntegrationDoctorSummary: Codable, Identifiable {
    let integration: String
    let status: String
    let checks: [IntegrationDoctorCheckSummary]

    var id: String { integration }
}

struct IntegrationCatalogSummary: Codable {
    let descriptors: [IntegrationDescriptorSummary]
    let detection: [IntegrationDetectionSummary]
    let doctor: [IntegrationDoctorSummary]
}

struct ApprovalSummary: Codable, Identifiable {
    let id: String
    let integration: String
    let action: String
    let risk: String
    let status: String
    let reason: String
    let expiresAt: String

    enum CodingKeys: String, CodingKey {
        case id, integration, action, risk, status, reason
        case expiresAt = "expires_at"
    }
}

struct InboxItemSummary: Codable, Identifiable {
    let id: String
    let kind: String
    let severity: String
    let status: String
    let title: String
    let createdAt: String?
    let detail: [String: AnyCodable]

    enum CodingKeys: String, CodingKey {
        case id, kind, severity, status, title
        case createdAt = "created_at"
        case detail
    }
}

struct MetricSummary: Codable, Identifiable {
    let id: String
    let key: String
    let value: Double
    let unit: String
    let source: String
}

struct EventSummary: Codable, Identifiable {
    let seq: Int
    let runID: String?
    let occurredAt: String
    let eventType: String
    let payload: [String: AnyCodable]

    var id: Int { seq }

    enum CodingKeys: String, CodingKey {
        case seq
        case runID = "run_id"
        case occurredAt = "occurred_at"
        case eventType = "event_type"
        case payload
    }
}

struct RunRecord: Codable, Identifiable {
    let id: String
    let automationID: String
    let automationRevision: Int
    let automationSnapshot: [String: AnyCodable]
    let status: String
    let scheduledAt: String?
    let startedAt: String
    let endedAt: String?
    let exitCode: Int?

    enum CodingKeys: String, CodingKey {
        case id
        case automationID = "automation_id"
        case automationRevision = "automation_revision"
        case automationSnapshot = "automation_snapshot"
        case status
        case scheduledAt = "scheduled_at"
        case startedAt = "started_at"
        case endedAt = "ended_at"
        case exitCode = "exit_code"
    }
}

struct RunLogsSummary: Codable, Identifiable {
    let runID: String
    let automationID: String
    let status: String
    let stdout: String
    let stderr: String

    var id: String { runID }

    enum CodingKeys: String, CodingKey {
        case runID = "run_id"
        case automationID = "automation_id"
        case status
        case stdout
        case stderr
    }
}

struct AnyCodable: Codable {
    init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        if container.decodeNil() {
            self.value = "null"
        } else if let string = try? container.decode(String.self) {
            self.value = string
        } else if let number = try? container.decode(Double.self) {
            self.value = String(number)
        } else if let bool = try? container.decode(Bool.self) {
            self.value = String(bool)
        } else {
            self.value = "object"
        }
    }

    let value: String
}

@MainActor
final class ControlPlaneModel: ObservableObject {
    @Published var selectedSection: ControlPlaneSection = .automations
    @Published private(set) var automations: [AutomationSummary] = []
    @Published private(set) var discoveredSources: [DiscoverySourceSummary] = []
    @Published private(set) var integrations: IntegrationCatalogSummary?
    @Published private(set) var approvals: [ApprovalSummary] = []
    @Published private(set) var runs: [RunRecord] = []
    @Published var selectedRunLogs: RunLogsSummary?
    @Published private(set) var inbox: [InboxItemSummary] = []
    @Published private(set) var metrics: [MetricSummary] = []
    @Published private(set) var events: [EventSummary] = []
    @Published private(set) var connectionMessage = "Not connected"
    @Published var showingError = false
    @Published private(set) var lastError: String?
    @Published private(set) var discoveryMessage = "No native scan has run yet"

    private let client = LocalJSONRPCClient(socketPath: ControlPlaneModel.defaultSocketPath)

    func refresh() {
        do {
            automations = try client.request(method: "automation.list", params: [:], decode: [AutomationSummary].self)
            integrations = try client.request(method: "integration.list", params: [:], decode: IntegrationCatalogSummary.self)
            approvals = try client.request(method: "approvals.list", params: ["limit": 100], decode: [ApprovalSummary].self)
            runs = try client.request(method: "runs.list", params: ["limit": 100], decode: [RunRecord].self)
            inbox = try client.request(method: "inbox.list", params: ["limit": 100], decode: [InboxItemSummary].self)
            metrics = try client.request(method: "metrics.list", params: [:], decode: [MetricSummary].self)
            events = try client.request(method: "events.list", params: ["limit": 100], decode: [EventSummary].self)
            connectionMessage = "Connected · local JSON-RPC"
            lastError = nil
        } catch {
            connectionMessage = "Daemon unavailable · start taskrail daemon --socket \(client.socketPath.path)"
            lastError = error.localizedDescription
        }
    }

    func discover() {
        do {
            discoveredSources = try client.request(
                method: "automation.scan",
                params: ["source": "all"],
                decode: [DiscoverySourceSummary].self
            )
            discoveryMessage = "Fresh scan · \(discoveredSources.count) native source(s)"
            refresh()
        } catch {
            lastError = error.localizedDescription
            showingError = true
        }
    }

    func decideApproval(_ approval: ApprovalSummary, approved: Bool) {
        do {
            _ = try client.request(
                method: approved ? "approval.approve" : "approval.reject",
                params: ["approval_id": approval.id],
                decode: ApprovalSummary.self
            )
            refresh()
        } catch {
            lastError = error.localizedDescription
            showingError = true
        }
    }

    func run(_ automation: AutomationSummary) {
        do {
            _ = try client.request(method: "automation.run", params: ["id": automation.id], decode: RunLaunchSummary.self)
            refresh()
        } catch {
            lastError = error.localizedDescription
            showingError = true
        }
    }

    func setPaused(_ automation: AutomationSummary, paused: Bool) {
        do {
            let method = paused ? "automation.pause" : "automation.resume"
            _ = try client.request(
                method: method,
                params: ["id": automation.id],
                decode: AutomationSummary.self
            )
            refresh()
        } catch {
            lastError = error.localizedDescription
            showingError = true
        }
    }

    func cancel(_ run: RunRecord) {
        do {
            _ = try client.request(
                method: "run.cancel",
                params: ["run_id": run.id],
                decode: RunCancelSummary.self
            )
            refresh()
        } catch {
            lastError = error.localizedDescription
            showingError = true
        }
    }

    func showLogs(for run: RunRecord) {
        do {
            selectedRunLogs = try client.request(
                method: "run.logs",
                params: ["run_id": run.id],
                decode: RunLogsSummary.self
            )
        } catch {
            lastError = error.localizedDescription
            showingError = true
        }
    }

    private static var defaultSocketPath: URL {
        FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent(".local/share/taskrail/taskraild.sock")
    }
}

private struct RunLaunchSummary: Codable {
    let runID: String
    let status: String

    enum CodingKeys: String, CodingKey {
        case runID = "run_id"
        case status
    }
}

private struct RunCancelSummary: Codable {
    let runID: String
    let status: String

    enum CodingKeys: String, CodingKey {
        case runID = "run_id"
        case status
    }
}
