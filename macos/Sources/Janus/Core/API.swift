import Foundation

/// A failed request: the orchestrator answers errors with a status and a Chinese message meant to
/// be shown as it is (docs/interfaces/http_api.md#errors).
struct APIError: LocalizedError {
    let status: Int
    let message: String
    var errorDescription: String? { message }
}

/// The orchestrator's HTTP API. Every screen reads and writes through it, exactly like the web page.
final class API: Sendable {
    let base: URL
    private let session: URLSession
    private let streams: URLSession

    init(base: URL) {
        self.base = base
        let cfg = URLSessionConfiguration.ephemeral
        cfg.timeoutIntervalForRequest = 60
        session = URLSession(configuration: cfg)
        // An event stream stays open for the whole run; the server sends keep-alives.
        let long = URLSessionConfiguration.ephemeral
        long.timeoutIntervalForRequest = 24 * 3600
        long.timeoutIntervalForResource = 7 * 24 * 3600
        streams = URLSession(configuration: long)
    }

    func url(_ path: String) -> URL { URL(string: base.absoluteString + path)! }

    static func q(_ s: String) -> String {
        var allowed = CharacterSet.urlQueryAllowed
        allowed.remove(charactersIn: "&=+?#/")
        return s.addingPercentEncoding(withAllowedCharacters: allowed) ?? s
    }

    func get(_ path: String, timeout: TimeInterval? = nil) async throws -> JSON {
        var req = URLRequest(url: url(path))
        if let timeout { req.timeoutInterval = timeout }
        return try await send(req)
    }

    func post(_ path: String, _ body: JSON = [:]) async throws -> JSON {
        var req = URLRequest(url: url(path))
        req.httpMethod = "POST"
        req.setValue("application/json", forHTTPHeaderField: "Content-Type")
        req.httpBody = body.data()
        return try await send(req)
    }

    func delete(_ path: String) async throws {
        var req = URLRequest(url: url(path))
        req.httpMethod = "DELETE"
        _ = try await send(req)
    }

    /// Raw text of a response (artifacts, exports).
    func text(_ path: String) async throws -> String {
        let (data, resp) = try await session.data(from: url(path))
        let status = (resp as? HTTPURLResponse)?.statusCode ?? 0
        let text = String(decoding: data, as: UTF8.self)
        guard (200..<300).contains(status) else { throw APIError(status: status, message: text.isEmpty ? "HTTP \(status)" : text) }
        return text
    }

    private func send(_ req: URLRequest) async throws -> JSON {
        let data: Data, resp: URLResponse
        do { (data, resp) = try await session.data(for: req) } catch {
            throw APIError(status: 0, message: "连不上 Janus 服务：\(error.localizedDescription)")
        }
        let status = (resp as? HTTPURLResponse)?.statusCode ?? 0
        guard (200..<300).contains(status) else {
            let text = String(decoding: data, as: UTF8.self)
            throw APIError(status: status, message: text.isEmpty ? "HTTP \(status)" : text)
        }
        return try JSON.parse(data)
    }

    /// Server-sent events of a run (`GET /api/runs/{id}/stream?after=`), one JSON event per
    /// message. A `lagged` message arrives as `.lagged`; the caller reloads the run.
    enum StreamItem { case event(JSON), lagged }

    func stream(run: String, after: Int) -> AsyncThrowingStream<StreamItem, Error> {
        let req = URLRequest(url: url("/api/runs/\(run)/stream?after=\(after)"))
        let streams = streams
        return AsyncThrowingStream { cont in
            let task = Task {
                do {
                    let (bytes, resp) = try await streams.bytes(for: req)
                    let status = (resp as? HTTPURLResponse)?.statusCode ?? 0
                    guard status == 200 else { throw APIError(status: status, message: "事件流打开失败（HTTP \(status)）") }
                    var name = ""
                    for try await line in bytes.lines {
                        if line.hasPrefix("event:") {
                            name = line.dropFirst(6).trimmingCharacters(in: .whitespaces)
                        } else if line.hasPrefix("data:") {
                            let payload = line.dropFirst(5).trimmingCharacters(in: .whitespaces)
                            if name == "lagged" { cont.yield(.lagged) } else if let v = try? JSON.parse(Data(payload.utf8)) { cont.yield(.event(v)) }
                            name = ""
                        }
                    }
                    cont.finish()
                } catch { cont.finish(throwing: error) }
            }
            cont.onTermination = { _ in task.cancel() }
        }
    }
}
