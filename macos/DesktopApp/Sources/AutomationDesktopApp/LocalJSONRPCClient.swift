import Foundation
import Darwin

enum LocalJSONRPCError: LocalizedError {
    case socketPathTooLong
    case connectFailed(String)
    case writeFailed(String)
    case readFailed(String)
    case responseError(String)
    case invalidResponse

    var errorDescription: String? {
        switch self {
        case .socketPathTooLong: return "The Unix socket path is too long."
        case .connectFailed(let message): return "Could not connect to Taskrail daemon: \(message)"
        case .writeFailed(let message): return "Could not write to Taskrail daemon: \(message)"
        case .readFailed(let message): return "Could not read from Taskrail daemon: \(message)"
        case .responseError(let message): return message
        case .invalidResponse: return "Taskrail daemon returned an invalid JSON-RPC response."
        }
    }
}

final class LocalJSONRPCClient: @unchecked Sendable {
    let socketPath: URL
    private var nextID = 1

    init(socketPath: URL) {
        self.socketPath = socketPath
    }

    func request<T: Decodable>(method: String, params: [String: Any], decode: T.Type) throws -> T {
        let id = nextID
        nextID += 1
        let request: [String: Any] = [
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        ]
        let requestData = try JSONSerialization.data(withJSONObject: request)
        let responseData = try send(requestData + Data([0x0a]))
        guard let object = try JSONSerialization.jsonObject(with: responseData) as? [String: Any] else {
            throw LocalJSONRPCError.invalidResponse
        }
        if let error = object["error"] as? [String: Any], let message = error["message"] as? String {
            throw LocalJSONRPCError.responseError(message)
        }
        guard let result = object["result"] else {
            throw LocalJSONRPCError.invalidResponse
        }
        let data = try JSONSerialization.data(withJSONObject: result)
        return try JSONDecoder().decode(T.self, from: data)
    }

    private func send(_ request: Data) throws -> Data {
        let fd = socket(AF_UNIX, SOCK_STREAM, 0)
        guard fd >= 0 else { throw LocalJSONRPCError.connectFailed(String(cString: strerror(errno))) }
        defer { _ = Darwin.close(fd) }

        var address = sockaddr_un()
        address.sun_family = sa_family_t(AF_UNIX)
        let path = socketPath.path
        let pathBytes = Array(path.utf8)
        guard pathBytes.count < MemoryLayout.size(ofValue: address.sun_path) else {
            throw LocalJSONRPCError.socketPathTooLong
        }
        withUnsafeMutableBytes(of: &address.sun_path) { buffer in
            buffer.initializeMemory(as: UInt8.self, repeating: 0)
            for (index, byte) in pathBytes.enumerated() { buffer[index] = byte }
        }
        let addressLength = socklen_t(MemoryLayout<sockaddr_un>.size)
        let connected = withUnsafePointer(to: &address) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                Darwin.connect(fd, $0, addressLength)
            }
        }
        guard connected == 0 else { throw LocalJSONRPCError.connectFailed(String(cString: strerror(errno))) }

        try writeAll(fd, data: request)
        var response = Data()
        var buffer = [UInt8](repeating: 0, count: 4096)
        while true {
            let count = recv(fd, &buffer, buffer.count, 0)
            if count < 0 { throw LocalJSONRPCError.readFailed(String(cString: strerror(errno))) }
            if count == 0 { break }
            response.append(contentsOf: buffer[0..<count])
            if response.contains(0x0a) { break }
        }
        guard let newline = response.firstIndex(of: 0x0a) else { throw LocalJSONRPCError.invalidResponse }
        return response[..<newline]
    }

    private func writeAll(_ fd: Int32, data: Data) throws {
        try data.withUnsafeBytes { bytes in
            guard let base = bytes.baseAddress else { return }
            var offset = 0
            while offset < bytes.count {
                let count = Darwin.write(fd, base.advanced(by: offset), bytes.count - offset)
                if count < 0 { throw LocalJSONRPCError.writeFailed(String(cString: strerror(errno))) }
                offset += count
            }
        }
    }
}
