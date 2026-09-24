// Author: Lukas Bower
// Purpose: Keep model suggestions typed, scoped to the observed job and distinct from verified outcomes.
// Copyright 2026 Lukas Bower

@testable import CohesixApple
import Foundation
import Testing

private func job(_ execution: String, cancelled: Bool = false) throws -> GatewayJob {
    let bytes = Data("""
    {"binding":{"admission_id":"job_123"},"execution":"\(execution)",\
    "delivery":"pending","cancel_requested":\(cancelled),"result_sha256":null}
    """.utf8)
    return try JSONDecoder().decode(GatewayJob.self, from: bytes)
}

@Test func modelProposalCannotCancelTerminalOrAlreadyCancelledWork() throws {
    #expect(JobAssistance.permittedAction(.requestCancellation, for: try job("confirmed")) == "no action")
    #expect(JobAssistance.permittedAction(.requestCancellation, for: try job("uncertain", cancelled: true)) == "no action")
    #expect(JobAssistance.permittedAction(.requestCancellation, for: try job("uncertain")) ==
            "request cancellation with confirmation for job_123")
}

@Test func assistanceLabelsInterpretationAndSource() throws {
    let result = JobAssistanceResult(
        recordSummary: "Job job_123: execution uncertain; delivery pending.",
        interpretation: "Outcome remains uncertain",
        proposedAction: "inspect job_123 again",
        sourceURL: URL(string: "https://hive.example.invalid/v1/jobs/job_123")!,
        usedFoundationModel: true
    )
    #expect(result.summary.contains("Model interpretation:"))
    #expect(result.summary.contains("Gateway record: Job job_123: execution uncertain"))
    #expect(result.summary.contains("Source: https://hive.example.invalid/v1/jobs/job_123"))
}
