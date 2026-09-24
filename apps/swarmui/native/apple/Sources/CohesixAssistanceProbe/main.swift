// Author: Lukas Bower
// Purpose: Exercise actual on-device Foundation Models generation against a clearly synthetic scoped job fixture.
// Copyright 2026 Lukas Bower

import CohesixApple
import Foundation

@main
struct CohesixAssistanceProbe {
    static func main() async throws {
        let fixture = Data("""
        {"binding":{"admission_id":"synthetic-job-1"},"execution":"uncertain",\
        "delivery":"pending","cancel_requested":false,"result_sha256":null}
        """.utf8)
        let job = try JSONDecoder().decode(GatewayJob.self, from: fixture)
        let endpoint = URL(string: "https://synthetic.invalid")!
        let result = await JobAssistance.explain(job: job, endpoint: endpoint)
        let output: [String: Any] = [
            "schema": "cohesix-apple-assistance-probe/v1",
            "fixture": "synthetic",
            "foundation_model_used": result.usedFoundationModel,
            "interpretation": result.interpretation,
            "proposed_action": result.proposedAction,
            "source": result.sourceURL.absoluteString,
        ]
        let data = try JSONSerialization.data(withJSONObject: output, options: [.sortedKeys])
        FileHandle.standardOutput.write(data)
        FileHandle.standardOutput.write(Data([0x0a]))
    }
}
