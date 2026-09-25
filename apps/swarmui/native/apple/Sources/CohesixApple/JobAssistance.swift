// Author: Lukas Bower
// Purpose: Explain one scoped gateway job with on-device Foundation Models while keeping all actions under operator control.
// Copyright 2026 Lukas Bower

import Foundation
import FoundationModels

@Generable
enum SuggestedAction {
    case inspectAgain
    case requestCancellation
    case noAction
}

@Generable
private struct GeneratedAdvice {
    @Guide(description: "Brief interpretation of only the supplied gateway job facts. Do not claim verified completion.")
    var explanation: String

    @Guide(description: "A possible next operator action. This value never executes an action.")
    var suggestedAction: SuggestedAction
}

public struct JobAssistanceResult: Sendable {
    public let recordSummary: String
    public let interpretation: String
    public let proposedAction: String
    public let sourceURL: URL
    public let usedFoundationModel: Bool

    public var summary: String {
        let label = usedFoundationModel ? "Model interpretation" : "Gateway status"
        return "Gateway record: \(recordSummary) \(label): \(interpretation) " +
            "Proposed action: \(proposedAction). Source: \(sourceURL.absoluteString)"
    }
}

public enum JobAssistance {
    private static let maximumExplanation = 600

    public static func explain(job: GatewayJob, endpoint: URL) async -> JobAssistanceResult {
        let source = endpoint.appending(path: "v1/jobs/\(job.binding.admissionID)")
        let manual = JobAssistanceResult(
            recordSummary: job.summary,
            interpretation: "On-device assistance is unavailable; use the observed gateway fields.",
            proposedAction: "inspect again in SwarmUI or coh",
            sourceURL: source,
            usedFoundationModel: false
        )
        let model = SystemLanguageModel.default
        guard model.isAvailable && model.supportsLocale(.current) else { return manual }
        let session = LanguageModelSession(
            model: model,
            instructions: "Interpret the supplied scoped Cohesix gateway record only. " +
                "It is an observation, not a signed terminal receipt. Never claim that cancellation " +
                "stopped work or that delivery pending means success. Propose an action only; do not execute it."
        )
        let prompt = """
        Admission ID: \(job.binding.admissionID)
        Ticket ID: \(job.binding.ticketID)
        Selected action: \(job.binding.action)
        Selected target: \(job.binding.target)
        Execution: \(job.execution)
        Result delivery: \(job.delivery)
        Cancellation requested: \(job.cancelRequested)
        Result digest: \(job.resultSHA256 ?? "none")
        Explain what these exact fields establish and choose one possible next action.
        """
        do {
            let generated = try await session.respond(to: prompt, generating: GeneratedAdvice.self).content
            let bounded = String(generated.explanation.prefix(maximumExplanation))
                .trimmingCharacters(in: .whitespacesAndNewlines)
            guard !bounded.isEmpty else { return manual }
            let action = permittedAction(generated.suggestedAction, for: job)
            return JobAssistanceResult(
                recordSummary: job.summary,
                interpretation: bounded,
                proposedAction: action,
                sourceURL: source,
                usedFoundationModel: true
            )
        } catch {
            return manual
        }
    }

    static func permittedAction(_ proposed: SuggestedAction, for job: GatewayJob) -> String {
        switch proposed {
        case .requestCancellation
            where ["reserved", "dispatching", "uncertain"].contains(job.execution)
                && !job.cancelRequested:
            return "request cancellation with confirmation for \(job.binding.admissionID)"
        case .inspectAgain:
            return "inspect \(job.binding.admissionID) again"
        default:
            return "no action"
        }
    }
}
