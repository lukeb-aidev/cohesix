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

struct StartApprovedCohesixJobIntent: AppIntent {
    static let title: LocalizedStringResource = "Start Approved Cohesix Job"
    static let description = IntentDescription(
        "Start one selected standing-scope service recipe using the original Hive Gateway authority."
    )

    @Parameter(title: "Approved Scope ID") var scopeID: String
    @Parameter(title: "Stable Request ID") var requestID: String

    func perform() async throws -> some IntentResult & ReturnsValue<String> {
        try GatewayJobs.validateAdmissionID(scopeID)
        try GatewayJobs.validateRequestID(requestID)
        try await requestConfirmation(
            dialog: "Start approved Cohesix scope \(scopeID) for request \(requestID)?"
        )
        let credentials = try GatewayKeychain.load()
        let job = try await GatewayJobs(credentials: credentials)
            .startApproved(scopeID: scopeID, requestID: requestID)
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
