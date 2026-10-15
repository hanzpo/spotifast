import Foundation

/// A connection to SwiftSpot's local API: one JSON object per line over a
/// Unix socket. Requests await their reply; events arrive on `onEvent`.
final class Client: @unchecked Sendable {
    enum Failure: Error, LocalizedError {
        case notRunning(String)
        case server(String)
        case closed

        var errorDescription: String? {
            switch self {
            case .notRunning: "SwiftSpot's engine didn't start."
            case .server(let message): message
            case .closed: "The connection to SwiftSpot closed."
            }
        }
    }

    /// Where the running SwiftSpot listens. `SWIFTSPOT_SOCKET` points
    /// elsewhere, such as at a demo instance.
    static var defaultPath: String {
        if let path = ProcessInfo.processInfo.environment["SWIFTSPOT_SOCKET"], !path.isEmpty {
            return path
        }
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        return support.appendingPathComponent("com.hanzpo.swiftspot/api.sock").path
    }

    private let fd: Int32
    private let lock = NSLock()
    private var nextID = 1
    private var waiting: [Int: CheckedContinuation<Data, Error>] = [:]
    /// Called on the main actor with each event's name and its data as JSON.
    var onEvent: (@MainActor @Sendable (String, Data) -> Void)?
    var onClose: (@MainActor @Sendable () -> Void)?

    init(path: String = Client.defaultPath) throws {
        fd = socket(AF_UNIX, SOCK_STREAM, 0)
        var address = sockaddr_un()
        address.sun_family = sa_family_t(AF_UNIX)
        let bytes = Array(path.utf8)
        guard bytes.count < MemoryLayout.size(ofValue: address.sun_path) else {
            close(fd)
            throw Failure.notRunning(path)
        }
        withUnsafeMutableBytes(of: &address.sun_path) { raw in
            raw.copyBytes(from: bytes)
            raw[bytes.count] = 0
        }
        let connected = withUnsafePointer(to: &address) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                connect(fd, $0, socklen_t(MemoryLayout<sockaddr_un>.size))
            }
        }
        guard connected == 0 else {
            close(fd)
            throw Failure.notRunning(path)
        }
        // A write after the engine has gone fails instead of ending the app.
        var one: Int32 = 1
        setsockopt(fd, SOL_SOCKET, SO_NOSIGPIPE, &one, socklen_t(MemoryLayout<Int32>.size))
        let thread = Thread { [weak self] in self?.readLoop() }
        thread.name = "swiftspot-api"
        thread.start()
    }

    deinit { close(fd) }

    /// Calls `method` and returns its `result` as JSON.
    @MainActor
    func call(_ method: String, _ params: [String: Any] = [:]) async throws -> Data {
        let id = takeID()
        let line = try encode(id: id, method: method, params: params)
        return try await withCheckedThrowingContinuation { continuation in
            lock.lock()
            waiting[id] = continuation
            lock.unlock()
            if !write(line) {
                finish(id, .failure(Failure.closed))
            }
        }
    }

    /// Calls `method` and decodes its result.
    @MainActor
    func call<T: Decodable>(_ method: String, _ params: [String: Any] = [:], as type: T.Type) async throws -> T {
        try JSONDecoder.api.decode(T.self, from: try await call(method, params))
    }

    /// Fire and forget, for the player's buttons: the reply is dropped.
    @MainActor
    func send(_ method: String, _ params: [String: Any] = [:]) {
        if let line = try? encode(id: takeID(), method: method, params: params) {
            _ = write(line)
        }
    }

    private func takeID() -> Int {
        lock.lock()
        defer { lock.unlock() }
        nextID += 1
        return nextID
    }

    private func encode(id: Int, method: String, params: [String: Any]) throws -> Data {
        var data = try JSONSerialization.data(
            withJSONObject: ["id": id, "method": method, "params": params] as [String: Any])
        data.append(0x0A)
        return data
    }

    private func write(_ data: Data) -> Bool {
        data.withUnsafeBytes { Foundation.write(fd, $0.baseAddress, data.count) } == data.count
    }

    private func finish(_ id: Int, _ result: Result<Data, Error>) {
        lock.lock()
        let continuation = waiting.removeValue(forKey: id)
        lock.unlock()
        continuation?.resume(with: result)
    }

    private func readLoop() {
        var buffer = Data()
        var chunk = [UInt8](repeating: 0, count: 64 * 1024)
        while true {
            let count = read(fd, &chunk, chunk.count)
            if count <= 0 { break }
            buffer.append(contentsOf: chunk[0..<count])
            while let newline = buffer.firstIndex(of: 0x0A) {
                let line = buffer[buffer.startIndex..<newline]
                buffer.removeSubrange(buffer.startIndex...newline)
                handle(line: Data(line))
            }
        }
        lock.lock()
        let pending = waiting
        waiting.removeAll()
        lock.unlock()
        for continuation in pending.values { continuation.resume(throwing: Failure.closed) }
        let onClose = onClose
        Task { @MainActor in onClose?() }
    }

    private func handle(line: Data) {
        guard let object = (try? JSONSerialization.jsonObject(with: line)) as? [String: Any] else { return }
        if let event = object["event"] as? String {
            let payload = (try? JSONSerialization.data(
                withJSONObject: object["data"] ?? NSNull(), options: [.fragmentsAllowed])) ?? Data()
            let onEvent = onEvent
            Task { @MainActor in onEvent?(event, payload) }
            return
        }
        guard let id = object["id"] as? Int else { return }
        if let error = object["error"] as? String {
            finish(id, .failure(Failure.server(error)))
        } else {
            let result = object["result"] ?? NSNull()
            let data = (try? JSONSerialization.data(withJSONObject: result, options: [.fragmentsAllowed])) ?? Data()
            finish(id, .success(data))
        }
    }
}

extension JSONDecoder {
    static let api: JSONDecoder = {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        return decoder
    }()
}
