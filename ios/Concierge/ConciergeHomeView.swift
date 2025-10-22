// Author: Lukas Bower
import SwiftUI
import UserNotifications

/// Minimal concierge surface limited to the home view and user notifications.
struct ConciergeHomeView: View {
    @State private var suggestion: ConciergeSuggestion?
    @State private var errorMessage: String?
    @ObservedObject var notifications: ConciergeNotificationCoordinator
    let suggestionSource: ConciergeSuggestionSource

    var body: some View {
        VStack(spacing: 16) {
            Text("Concierge")
                .font(.largeTitle)
                .bold()
                .accessibilityIdentifier("concierge-title")

            if let suggestion {
                suggestionCard(for: suggestion)
            } else if let errorMessage {
                Text(errorMessage)
                    .font(.footnote)
                    .foregroundColor(.secondary)
                    .multilineTextAlignment(.center)
                    .accessibilityIdentifier("concierge-error")
            } else {
                Text("Ask Apple Intelligence for a personalised media pick.")
                    .font(.footnote)
                    .foregroundColor(.secondary)
                    .multilineTextAlignment(.center)
                    .accessibilityIdentifier("concierge-empty")
            }

            Button(action: refreshSuggestion) {
                Label("Refresh Suggestion", systemImage: "sparkles")
                    .padding(.horizontal, 20)
            }
            .buttonStyle(.borderedProminent)
            .accessibilityIdentifier("concierge-refresh")

            Button(action: notifications.schedulePreviewNotification) {
                Label("Send Preview Notification", systemImage: "bell")
            }
            .buttonStyle(.bordered)
            .accessibilityIdentifier("concierge-notification")
        }
        .padding()
        .onAppear(perform: notifications.registerDelegate)
        .task(refreshSuggestion)
    }

    @ViewBuilder
    private func suggestionCard(for suggestion: ConciergeSuggestion) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(suggestion.title)
                .font(.title3)
                .bold()
            Text(suggestion.subtitle)
                .font(.subheadline)
                .foregroundColor(.secondary)
            if !suggestion.rationale.isEmpty {
                Divider()
                ForEach(Array(suggestion.rationale.enumerated()), id: \.offset) { index, line in
                    Text("\(index + 1). \(line)")
                        .font(.caption)
                        .accessibilityIdentifier("concierge-rationale-\(index)")
                }
            }
        }
        .padding()
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: 12).fill(Color(.secondarySystemBackground)))
        .accessibilityIdentifier("concierge-card")
    }

    private func refreshSuggestion() {
        do {
            suggestion = try suggestionSource.fetchSuggestion()
            errorMessage = nil
        } catch {
            suggestion = nil
            errorMessage = error.localizedDescription
        }
    }
}

/// Representation of a concierge suggestion surfaced by Apple Intelligence.
struct ConciergeSuggestion: Identifiable {
    let id: UUID
    let title: String
    let subtitle: String
    let rationale: [String]

    init(id: UUID = UUID(), title: String, subtitle: String, rationale: [String]) {
        self.id = id
        self.title = title
        self.subtitle = subtitle
        self.rationale = rationale
    }
}

/// Abstraction that supplies suggestions for the home view.
protocol ConciergeSuggestionSource {
    func fetchSuggestion() throws -> ConciergeSuggestion
}

/// Notification helper dedicated to concierge previews.
final class ConciergeNotificationCoordinator: NSObject, ObservableObject, UNUserNotificationCenterDelegate {
    private lazy var center: UNUserNotificationCenter = UNUserNotificationCenter.current()

    func registerDelegate() {
        center.delegate = self
        center.requestAuthorization(options: [.alert, .sound]) { granted, _ in
            if !granted {
                NSLog("Concierge notifications disabled by user preference.")
            }
        }
    }

    func schedulePreviewNotification() {
        let content = UNMutableNotificationContent()
        content.title = "Concierge Preview"
        content.body = "Ask Ralph for a fresh recommendation tailored to tonight."
        content.sound = .default

        let trigger = UNTimeIntervalNotificationTrigger(timeInterval: 2, repeats: false)
        let request = UNNotificationRequest(
            identifier: "cohesix.concierge.preview",
            content: content,
            trigger: trigger
        )
        center.add(request) { error in
            if let error {
                NSLog("Failed to schedule concierge preview: \(error)")
            }
        }
    }
}

@available(*, deprecated, message: "Replaced by RalphConcierge")
final class LegacyConciergeExperience: ObservableObject {
    @Published var isActive = false

    func presentLegacyFlow() {
        isActive = true
    }

    func tearDown() {
        isActive = false
    }
}
