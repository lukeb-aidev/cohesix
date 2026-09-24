// Author: Lukas Bower
// Purpose: Read and request cancellation of an admitted Hive Gateway job without inventing an outcome.
// Copyright 2026 Lukas Bower

import Foundation

public enum GatewayJobError: Error, LocalizedError {
    case invalidEndpoint
    case invalidAdmissionID
    case invalidCredential
    case unexpectedResponse
    case responseTooLarge
    case rejected(Int)

    public var errorDescription: String? {
        switch self {
        case .invalidEndpoint: "Use an HTTPS Hive Gateway, or HTTP on loopback."
        case .invalidAdmissionID: "Choose a bounded admission ID from an existing job."
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
        public let admissionID: String
        enum CodingKeys: String, CodingKey { case admissionID = "admission_id" }
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
        return "Job \(binding.admissionID): execution \(execution); delivery \(delivery)\(cancellation)\(result)."
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

    public func inspect(_ admissionID: String) async throws -> GatewayJob {
        try await request(admissionID, cancel: false)
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
        var data = Data()
        for try await byte in bytes {
            guard data.count < Self.maximumResponse else { throw GatewayJobError.responseTooLarge }
            data.append(byte)
        }
        guard response.statusCode == (cancel ? 202 : 200) else {
            throw GatewayJobError.rejected(response.statusCode)
        }
        guard let job = try? JSONDecoder().decode(GatewayJob.self, from: data),
              job.binding.admissionID == admissionID,
              ["reserved", "dispatching", "uncertain", "confirmed", "refused_no_effect"].contains(job.execution),
              ["pending", "acknowledged"].contains(job.delivery),
              job.resultSHA256.map({ $0.count == 64 && $0.utf8.allSatisfy { (48...57).contains($0) || (97...102).contains($0) } }) ?? true
        else { throw GatewayJobError.unexpectedResponse }
        return job
    }
}
