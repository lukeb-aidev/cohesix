// Author: Lukas Bower
// Purpose: Verify gateway job IDs, authenticated request shapes and refusal of contradictory results.
// Copyright 2026 Lukas Bower

import CohesixApple
import Foundation
import Testing

private final class StubProtocol: URLProtocol {
    private static let lock = NSLock()
    nonisolated(unsafe) private static var responses: [String: (Int, Data)] = [:]
    nonisolated(unsafe) private static var requests: [String: URLRequest] = [:]

    static func prepare(host: String, status: Int, body: Data) {
        lock.lock()
        defer { lock.unlock() }
        responses[host] = (status, body)
    }

    static func observed(host: String) -> URLRequest? {
        lock.lock()
        defer { lock.unlock() }
        return requests[host]
    }

    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }

    override func startLoading() {
        let host = request.url!.host!
        Self.lock.lock()
        Self.requests[host] = request
        let selected = Self.responses[host]!
        Self.lock.unlock()
        let response = HTTPURLResponse(
            url: request.url!, statusCode: selected.0,
            httpVersion: "HTTP/1.1", headerFields: ["Content-Type": "application/json"]
        )!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: selected.1)
        client?.urlProtocolDidFinishLoading(self)
    }

    override func stopLoading() {}
}

private func client(host: String) throws -> GatewayJobs {
    let credentials = try GatewayCredentials(
        endpoint: "https://\(host)",
        requestToken: "request-token",
        delegatedTicket: "delegated-ticket"
    )
    let configuration = URLSessionConfiguration.ephemeral
    configuration.protocolClasses = [StubProtocol.self]
    return GatewayJobs(credentials: credentials, session: URLSession(configuration: configuration))
}

private func record(id: String) -> Data {
    Data("""
    {"binding":{"admission_id":"\(id)"},"execution":"uncertain",\
    "delivery":"pending","cancel_requested":false,"result_sha256":null}
    """.utf8)
}

@Test func rejectsUnsafeGatewayInputs() throws {
    #expect(throws: GatewayJobError.self) {
        try GatewayCredentials(endpoint: "http://hive.example.invalid", requestToken: "token", delegatedTicket: "ticket")
    }
    #expect(throws: GatewayJobError.self) {
        try GatewayCredentials(endpoint: "https://hive.example.invalid/path", requestToken: "token", delegatedTicket: "ticket")
    }
    #expect(throws: GatewayJobError.self) { try GatewayJobs.validateAdmissionID("one/two") }
    #expect(throws: GatewayJobError.self) { try GatewayJobs.validateAdmissionID(String(repeating: "a", count: 129)) }
}

@Test func inspectsOriginalAdmissionWithoutClaimingSuccess() async throws {
    let host = "inspect.example.invalid"
    StubProtocol.prepare(host: host, status: 200, body: record(id: "job_123"))
    let job = try await client(host: host).inspect("job_123")
    #expect(job.summary.contains("execution uncertain"))
    #expect(!job.summary.contains("verified"))
    #expect(StubProtocol.observed(host: host)?.httpMethod == "GET")
    #expect(StubProtocol.observed(host: host)?.url?.path == "/v1/jobs/job_123")
    #expect(StubProtocol.observed(host: host)?.value(forHTTPHeaderField: "Authorization") == "Bearer request-token")
    #expect(StubProtocol.observed(host: host)?.value(forHTTPHeaderField: "x-cohesix-ticket") == "delegated-ticket")
}

@Test func requestsCancellationWithoutAssertingTermination() async throws {
    let host = "cancel.example.invalid"
    StubProtocol.prepare(host: host, status: 202, body: record(id: "job_123"))
    let job = try await client(host: host).requestCancellation("job_123")
    #expect(job.execution == "uncertain")
    #expect(StubProtocol.observed(host: host)?.httpMethod == "POST")
    #expect(StubProtocol.observed(host: host)?.url?.path == "/v1/jobs/job_123/cancel")
}

@Test func rejectsCrossAdmissionAndOversizedResponses() async throws {
    StubProtocol.prepare(host: "cross.example.invalid", status: 200, body: record(id: "other_job"))
    await #expect(throws: GatewayJobError.self) {
        try await client(host: "cross.example.invalid").inspect("job_123")
    }
    StubProtocol.prepare(host: "large.example.invalid", status: 200, body: Data(repeating: 0x20, count: 65_537))
    await #expect(throws: GatewayJobError.self) {
        try await client(host: "large.example.invalid").inspect("job_123")
    }
}
