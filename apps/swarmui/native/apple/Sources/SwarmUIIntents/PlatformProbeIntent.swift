// Author: Lukas Bower
// Purpose: Expose installed Apple actions for capability checks and governed gateway job start, inspection and cancellation.
// Copyright 2026 Lukas Bower

import AppIntents
#if canImport(CohesixApple)
import CohesixApple
#endif

struct PlatformProbeIntent: AppIntent {
    static let title: LocalizedStringResource = "Check Cohesix Apple Support"
    static let description = IntentDescription(
        "Check local Apple AI and Metal availability without accessing a hive."
    )

    func perform() async throws -> some IntentResult & ReturnsValue<String> {
        .result(value: PlatformCapabilities.current().summary)
    }
}

struct InspectCohesixJobIntent: AppIntent {
    static let title: LocalizedStringResource = "Inspect Cohesix Job"
    static let description = IntentDescription(
        "Read the original admitted job from the configured Hive Gateway."
    )

    @Parameter(title: "Admission ID") var admissionID: String

    func perform() async throws -> some IntentResult & ReturnsValue<String> {
        let credentials = try GatewayKeychain.load()
        let job = try await GatewayJobs(credentials: credentials).inspect(admissionID)
        return .result(value: job.summary)
    }
}

struct ApprovedCohesixScope: AppEntity {
    static let typeDisplayRepresentation: TypeDisplayRepresentation = "Approved Cohesix Scope"
    static let defaultQuery = ApprovedCohesixScopeQuery()

    let id: String
    let target: String

    var displayRepresentation: DisplayRepresentation {
        DisplayRepresentation(title: "\(id)", subtitle: "\(target)")
    }
}

struct ApprovedCohesixScopeQuery: EntityQuery {
    func entities(for identifiers: [String]) async throws -> [ApprovedCohesixScope] {
        let requested = Set(identifiers)
        return try await suggestedEntities().filter { requested.contains($0.id) }
    }

    func suggestedEntities() async throws -> [ApprovedCohesixScope] {
        let credentials = try GatewayKeychain.load()
        let scopes = try await GatewayJobs(credentials: credentials).availableScopes()
        return scopes.map { ApprovedCohesixScope(id: $0.id, target: $0.target) }
    }
}

struct StartApprovedCohesixJobIntent: AppIntent {
    static let title: LocalizedStringResource = "Start Approved Cohesix Job"
    static let description = IntentDescription(
        "Start one selected standing-scope service recipe using the original Hive Gateway authority."
    )

    @Parameter(title: "Approved Scope") var scope: ApprovedCohesixScope
    @Parameter(title: "Stable Request ID") var requestID: String

    func perform() async throws -> some IntentResult & ReturnsValue<String> {
        try GatewayJobs.validateRequestID(requestID)
        try await requestConfirmation(
            dialog: "Start approved Cohesix scope \(scope.id) for request \(requestID)?"
        )
        let credentials = try GatewayKeychain.load()
        let job = try await GatewayJobs(credentials: credentials)
            .startApproved(scopeID: scope.id, requestID: requestID)
        return .result(value: job.summary)
    }
}

struct CancelCohesixJobIntent: AppIntent {
    static let title: LocalizedStringResource = "Request Cohesix Job Cancellation"
    static let description = IntentDescription(
        "Request cancellation of an admitted job; verify its later outcome separately."
    )

    @Parameter(title: "Admission ID") var admissionID: String

    func perform() async throws -> some IntentResult & ReturnsValue<String> {
        try GatewayJobs.validateAdmissionID(admissionID)
        try await requestConfirmation(
            dialog: "Request cancellation for Cohesix job \(admissionID)?"
        )
        let credentials = try GatewayKeychain.load()
        let job = try await GatewayJobs(credentials: credentials).requestCancellation(admissionID)
        return .result(value: job.summary)
    }
}

struct ExplainCohesixJobIntent: AppIntent {
    static let title: LocalizedStringResource = "Explain Cohesix Job"
    static let description = IntentDescription(
        "Interpret a scoped gateway job with on-device Apple Intelligence when available."
    )

    @Parameter(title: "Admission ID") var admissionID: String

    func perform() async throws -> some IntentResult & ReturnsValue<String> {
        let credentials = try GatewayKeychain.load()
        let job = try await GatewayJobs(credentials: credentials).inspect(admissionID)
        let assistance = await JobAssistance.explain(job: job, endpoint: credentials.endpoint)
        return .result(value: assistance.summary)
    }
}
