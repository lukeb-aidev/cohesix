// Author: Lukas Bower
// Purpose: Verify approved job start, stable identities, authenticated request shapes and refusal of contradictory results.
// Copyright 2026 Lukas Bower

import CohesixApple
import Foundation
import Testing

private final class StubProtocol: URLProtocol {
    private static let lock = NSLock()
    nonisolated(unsafe) private static var responses: [String: (Int, Data)] = [:]
    nonisolated(unsafe) private static var requests: [String: URLRequest] = [:]
    nonisolated(unsafe) private static var requestBodies: [String: Data] = [:]

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

    static func observedBody(host: String) -> Data? {
        lock.lock()
        defer { lock.unlock() }
        return requestBodies[host]
    }

    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }

    override func startLoading() {
        let host = request.url!.host!
        var body = request.httpBody ?? Data()
        if let stream = request.httpBodyStream {
            stream.open()
            defer { stream.close() }
            var buffer = [UInt8](repeating: 0, count: 4096)
            let count = stream.read(&buffer, maxLength: buffer.count)
            if count > 0 { body = Data(buffer.prefix(count)) }
        }
        Self.lock.lock()
        Self.requests[host] = request
        Self.requestBodies[host] = body
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
    {"binding":{"admission_id":"\(id)","ticket_id":"ticket_123",\
    "action":"gpu.workload.submit","target":"/gpu/gpu_1/workload"},"execution":"uncertain",\
    "delivery":"pending","cancel_requested":false,"result_sha256":null}
    """.utf8)
}

private func started(scope: String, id: String) -> Data {
    Data("""
    {"schema":"cohesix-selected-job-response/v1","record":{"binding":{\
    "scope_id":"\(scope)","admission_id":"mac-\(id)","ticket_id":"mac-\(id)",\
    "action":"systemd.restart","target":"/host/systemd/cohesix-agent.service/restart"},\
    "execution":"reserved","delivery":"pending","cancel_requested":false,\
    "result_sha256":null},"submission":"target_write_ack"}
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
    #expect(throws: GatewayJobError.self) { try GatewayJobs.validateRequestID("../escape") }
    #expect(throws: GatewayJobError.self) { try GatewayJobs.validateRequestID(String(repeating: "a", count: 97)) }
}

@Test func startsOnlyAnApprovedScopeWithStableIdentity() async throws {
    let host = "start.example.invalid"
    StubProtocol.prepare(host: host, status: 202, body: started(scope: "service-1", id: "run-123"))
    let job = try await client(host: host).startApproved(scopeID: "service-1", requestID: "run-123")
    #expect(job.binding.admissionID == "mac-run-123")
    #expect(job.execution == "reserved")
    let request = StubProtocol.observed(host: host)
    #expect(request?.httpMethod == "POST")
    #expect(request?.url?.path == "/v1/jobs/approved/service-1/start")
    #expect(request?.value(forHTTPHeaderField: "Authorization") == "Bearer request-token")
    #expect(request?.value(forHTTPHeaderField: "x-cohesix-ticket") == "delegated-ticket")
    #expect(StubProtocol.observedBody(host: host) == Data(#"{"request_id":"run-123"}"#.utf8))
}

@Test func rejectsWrongApprovedScopeOrIdentity() async throws {
    StubProtocol.prepare(host: "start-wrong.example.invalid", status: 202,
                         body: started(scope: "other-scope", id: "run-123"))
    await #expect(throws: GatewayJobError.self) {
        try await client(host: "start-wrong.example.invalid")
            .startApproved(scopeID: "service-1", requestID: "run-123")
    }
    StubProtocol.prepare(host: "start-refused.example.invalid", status: 403,
                         body: Data(#"{"error":"EPERM revoked approved scope"}"#.utf8))
    await #expect(throws: GatewayJobError.self) {
        try await client(host: "start-refused.example.invalid")
            .startApproved(scopeID: "service-1", requestID: "run-123")
    }
}

@Test func inspectsOriginalAdmissionWithoutClaimingSuccess() async throws {
    let host = "inspect.example.invalid"
    StubProtocol.prepare(host: host, status: 200, body: record(id: "job_123"))
    let job = try await client(host: host).inspect("job_123")
    #expect(job.summary.contains("execution uncertain"))
    #expect(job.summary.contains("gpu.workload.submit at /gpu/gpu_1/workload"))
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

@Test func rejectsContradictorySelectedActionTarget() async throws {
    let changed = String(decoding: record(id: "job_123"), as: UTF8.self)
        .replacingOccurrences(of: "/gpu/gpu_1/workload", with: "/models/gpu_1/release")
    StubProtocol.prepare(host: "wrong-target.example.invalid", status: 200,
                         body: Data(changed.utf8))
    await #expect(throws: GatewayJobError.self) {
        try await client(host: "wrong-target.example.invalid").inspect("job_123")
    }
}

@Test func rejectsClaimedTerminalWithoutItsRetainedResultDigest() async throws {
    let changed = String(decoding: record(id: "job_123"), as: UTF8.self)
        .replacingOccurrences(of: "\"execution\":\"uncertain\"", with: "\"execution\":\"confirmed\"")
    StubProtocol.prepare(host: "false-terminal.example.invalid", status: 200,
                         body: Data(changed.utf8))
    await #expect(throws: GatewayJobError.self) {
        try await client(host: "false-terminal.example.invalid").inspect("job_123")
    }
}
