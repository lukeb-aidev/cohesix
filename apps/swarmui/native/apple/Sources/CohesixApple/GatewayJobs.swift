// Author: Lukas Bower
// Purpose: Start selected standing-scope recipes and inspect or cancel their original Hive Gateway jobs without inventing outcomes.
// Copyright 2026 Lukas Bower

import Foundation

public enum GatewayJobError: Error, LocalizedError {
    case invalidEndpoint
    case invalidAdmissionID
    case invalidRequestID
    case invalidCredential
    case unexpectedResponse
    case responseTooLarge
    case rejected(Int)

    public var errorDescription: String? {
        switch self {
        case .invalidEndpoint: "Use an HTTPS Hive Gateway, or HTTP on loopback."
        case .invalidAdmissionID: "Choose a bounded admission ID from an existing job."
        case .invalidRequestID: "Choose a stable bounded request ID for this approved recipe."
        case .invalidCredential: "Enrol a request token and delegated ticket for this gateway."
        case .unexpectedResponse: "The gateway returned an invalid or mismatched job record."
        case .responseTooLarge: "The gateway job response exceeded its bound."
        case .rejected(let status): "The gateway refused this job request (HTTP \(status))."
        }
    }
}

public struct GatewayCredentials: Sendable {
    public let endpoint: URL
    public let requestToken: String
    public let delegatedTicket: String

    public init(endpoint: String, requestToken: String, delegatedTicket: String) throws {
        guard let url = URL(string: endpoint),
              let components = URLComponents(url: url, resolvingAgainstBaseURL: false),
              let host = components.host,
              !host.isEmpty,
              components.user == nil,
              components.password == nil,
              components.query == nil,
              components.fragment == nil,
              components.path.isEmpty || components.path == "/",
              (components.scheme == "https" ||
                  (components.scheme == "http" &&
                   ["localhost", "127.0.0.1", "::1"].contains(host.lowercased())))
        else { throw GatewayJobError.invalidEndpoint }
        guard !requestToken.isEmpty, !delegatedTicket.isEmpty,
              requestToken.utf8.count <= 4096,
              delegatedTicket.utf8.count <= 8192,
              !requestToken.unicodeScalars.contains(where: CharacterSet.controlCharacters.contains),
              !delegatedTicket.unicodeScalars.contains(where: CharacterSet.controlCharacters.contains)
        else { throw GatewayJobError.invalidCredential }
        self.endpoint = url
        self.requestToken = requestToken
        self.delegatedTicket = delegatedTicket
    }
}

public struct GatewayJob: Decodable, Sendable {
    public struct Binding: Decodable, Sendable {
        public let scopeID: String?
        public let admissionID: String
        public let ticketID: String
        public let action: String
        public let target: String
        enum CodingKeys: String, CodingKey {
            case scopeID = "scope_id"
            case admissionID = "admission_id"
            case ticketID = "ticket_id"
            case action, target
        }
    }

    public let binding: Binding
    public let execution: String
    public let delivery: String
    public let cancelRequested: Bool
    public let resultSHA256: String?

    enum CodingKeys: String, CodingKey {
        case binding, execution, delivery
        case cancelRequested = "cancel_requested"
        case resultSHA256 = "result_sha256"
    }

    public var summary: String {
        let cancellation = cancelRequested ? "; cancellation requested" : ""
        let result = resultSHA256.map { "; reported result SHA-256 \($0)" } ?? ""
        return "Job \(binding.admissionID): \(binding.action) at \(binding.target); " +
            "ticket \(binding.ticketID); execution \(execution); delivery \(delivery)\(cancellation)\(result)."
    }
}

/// A redirect must never carry the gateway token or delegated ticket to another origin.
private final class NoRedirects: NSObject, URLSessionTaskDelegate, @unchecked Sendable {
    func urlSession(
        _ session: URLSession,
        task: URLSessionTask,
        willPerformHTTPRedirection response: HTTPURLResponse,
        newRequest request: URLRequest,
        completionHandler: @escaping (URLRequest?) -> Void
    ) {
        completionHandler(nil)
    }
}

public struct GatewayJobs: Sendable {
    private let credentials: GatewayCredentials
    private let session: URLSession
    private static let maximumResponse = 65_536

    public init(credentials: GatewayCredentials) {
        self.credentials = credentials
        let configuration = URLSessionConfiguration.ephemeral
        configuration.timeoutIntervalForRequest = 10
        configuration.timeoutIntervalForResource = 20
        configuration.httpCookieAcceptPolicy = .never
        self.session = URLSession(configuration: configuration, delegate: NoRedirects(), delegateQueue: nil)
    }

    public init(credentials: GatewayCredentials, session: URLSession) {
        self.credentials = credentials
        self.session = session
    }

    public static func validateAdmissionID(_ id: String) throws {
        guard (1...128).contains(id.utf8.count),
              id.utf8.allSatisfy({ byte in
                  (65...90).contains(byte) || (97...122).contains(byte) ||
                  (48...57).contains(byte) || byte == 45 || byte == 95
              })
        else { throw GatewayJobError.invalidAdmissionID }
    }

    public static func validateRequestID(_ id: String) throws {
        guard (1...96).contains(id.utf8.count),
              !id.hasPrefix("-"), !id.contains(".."),
              id.utf8.allSatisfy({ byte in
                  (65...90).contains(byte) || (97...122).contains(byte) ||
                  (48...57).contains(byte) || [45, 46, 95].contains(byte)
              })
        else { throw GatewayJobError.invalidRequestID }
    }

    private static func validSelectedTarget(action: String, target: String) -> Bool {
        let pattern: String
        switch action {
        case "systemd.restart": pattern = #"^/host/systemd/[A-Za-z0-9._-]{1,128}/restart$"#
        case "gpu.workload.submit": pattern = #"^/gpu/[A-Za-z0-9_-]{1,128}/workload$"#
        case "peft.release": pattern = #"^/models/[A-Za-z0-9_-]{1,128}/release$"#
        default: return false
        }
        return target.range(of: pattern, options: .regularExpression) != nil
    }

    public func inspect(_ admissionID: String) async throws -> GatewayJob {
        try await request(admissionID, cancel: false)
    }

    /// A user-supplied stable ID makes a lost reply resolvable to the original
    /// job. The gateway derives the ticket, selected action and fresh facts.
    public func startApproved(scopeID: String, requestID: String) async throws -> GatewayJob {
        try Self.validateAdmissionID(scopeID)
        try Self.validateRequestID(requestID)
        guard let url = URL(
            string: "/v1/jobs/approved/\(scopeID)/start", relativeTo: credentials.endpoint
        )?.absoluteURL else { throw GatewayJobError.invalidEndpoint }
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("Bearer \(credentials.requestToken)", forHTTPHeaderField: "Authorization")
        request.setValue(credentials.delegatedTicket, forHTTPHeaderField: "x-cohesix-ticket")
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = try JSONSerialization.data(withJSONObject: ["request_id": requestID])
        let (bytes, response) = try await session.bytes(for: request)
        guard let response = response as? HTTPURLResponse else {
            throw GatewayJobError.unexpectedResponse
        }
        let data = try await Self.readBounded(bytes)
        guard [200, 202].contains(response.statusCode) else {
            throw GatewayJobError.rejected(response.statusCode)
        }
        struct Started: Decodable {
            let schema: String
            let record: GatewayJob
            let submission: String
        }
        guard let started = try? JSONDecoder().decode(Started.self, from: data),
              started.schema == "cohesix-selected-job-response/v1",
              ((response.statusCode == 200 && started.submission == "existing") ||
               (response.statusCode == 202 && started.submission == "target_write_ack")),
              started.record.binding.scopeID == scopeID,
              started.record.binding.admissionID == "mac-\(requestID)",
              started.record.binding.ticketID == "mac-\(requestID)",
              started.record.binding.action == "systemd.restart",
              Self.validSelectedTarget(
                  action: started.record.binding.action,
                  target: started.record.binding.target
              ),
              Self.validJob(started.record)
        else { throw GatewayJobError.unexpectedResponse }
        return started.record
    }

    /// The gateway records a cancellation request; this does not prove native termination.
    public func requestCancellation(_ admissionID: String) async throws -> GatewayJob {
        try await request(admissionID, cancel: true)
    }

    private func request(_ admissionID: String, cancel: Bool) async throws -> GatewayJob {
        try Self.validateAdmissionID(admissionID)
        let suffix = "/v1/jobs/\(admissionID)" + (cancel ? "/cancel" : "")
        guard let url = URL(string: suffix, relativeTo: credentials.endpoint)?.absoluteURL else {
            throw GatewayJobError.invalidEndpoint
        }
        var request = URLRequest(url: url)
        request.httpMethod = cancel ? "POST" : "GET"
        request.setValue("Bearer \(credentials.requestToken)", forHTTPHeaderField: "Authorization")
        request.setValue(credentials.delegatedTicket, forHTTPHeaderField: "x-cohesix-ticket")
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        let (bytes, response) = try await session.bytes(for: request)
        guard let response = response as? HTTPURLResponse else {
            throw GatewayJobError.unexpectedResponse
        }
        let data = try await Self.readBounded(bytes)
        guard response.statusCode == (cancel ? 202 : 200) else {
            throw GatewayJobError.rejected(response.statusCode)
        }
        guard let job = try? JSONDecoder().decode(GatewayJob.self, from: data),
              job.binding.admissionID == admissionID,
              (try? Self.validateAdmissionID(job.binding.ticketID)) != nil,
              Self.validSelectedTarget(action: job.binding.action, target: job.binding.target),
              Self.validJob(job)
        else { throw GatewayJobError.unexpectedResponse }
        return job
    }

    private static func readBounded(_ bytes: URLSession.AsyncBytes) async throws -> Data {
        var data = Data()
        for try await byte in bytes {
            guard data.count < maximumResponse else { throw GatewayJobError.responseTooLarge }
            data.append(byte)
        }
        return data
    }

    private static func validJob(_ job: GatewayJob) -> Bool {
        ["reserved", "dispatching", "uncertain", "confirmed", "refused_no_effect"]
            .contains(job.execution)
            && ["pending", "acknowledged"].contains(job.delivery)
            && (job.execution == "confirmed") == (job.resultSHA256 != nil)
            && (job.delivery != "acknowledged" ||
                ["confirmed", "refused_no_effect"].contains(job.execution))
            && (job.resultSHA256.map {
                $0.count == 64 && $0.utf8.allSatisfy {
                    (48...57).contains($0) || (97...102).contains($0)
                }
            } ?? true)
    }
}
