import RadrootsKit
import SwiftUI
import UIKit

struct RuntimeStatusView: View {
    let phase: RadrootsAppModel.Phase
    let retry: () -> Void
    let createIdentity: () -> Void
    let importIdentity: (RadrootsIdentitySecretMaterial) -> Void
    let unlockIdentity: () -> Void
    let recoverIdentity: () -> Void
    let applyConfigurationReconfiguration: () -> Void
    @State private var showsIdentityImport = false

    var body: some View {
        NavigationStack {
            VStack(spacing: 20) {
                Spacer()
                Image(systemName: symbolName)
                    .font(.system(size: 48, weight: .medium))
                    .foregroundStyle(symbolColor)
                    .accessibilityHidden(true)
                Text(title)
                    .font(.title2.weight(.semibold))
                Text(detail)
                    .font(.body)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
                if case let .failed(failure) = phase {
                    Text("Error code \(failure.code)")
                        .font(.caption.monospaced())
                        .foregroundStyle(.secondary)
                        .accessibilityIdentifier("radroots.runtime.failure_code")
                }
                if case .identityRequired = phase {
                    Button("Create identity", action: createIdentity)
                        .buttonStyle(.borderedProminent)
                        .accessibilityIdentifier("radroots.identity.create")
                    Button("Import identity") { showsIdentityImport = true }
                        .buttonStyle(.bordered)
                        .accessibilityIdentifier("radroots.identity.import")
                } else if case .identityLocked = phase {
                    Button("Unlock identity", action: unlockIdentity)
                        .buttonStyle(.borderedProminent)
                        .accessibilityIdentifier("radroots.identity.unlock")
                } else if case .recoveryRequired = phase {
                    Button("Recover identity", action: recoverIdentity)
                        .buttonStyle(.borderedProminent)
                        .accessibilityIdentifier("radroots.identity.recover")
                } else if case .configurationReconfigurationRequired = phase {
                    Button("Apply network configuration", action: applyConfigurationReconfiguration)
                        .buttonStyle(.borderedProminent)
                        .accessibilityIdentifier("radroots.configuration.reconfigure")
                } else if case .failed = phase {
                    Button("Retry", action: retry)
                        .buttonStyle(.borderedProminent)
                        .accessibilityIdentifier("radroots.runtime.retry")
                }
                Spacer()
            }
            .padding(24)
            .navigationTitle("Radroots")
        }
        .sheet(isPresented: $showsIdentityImport) {
            NavigationStack {
                VStack(alignment: .leading, spacing: 16) {
                    Text("Enter an nsec or 64-character secret key. It is transferred directly to Apple custody and is never stored in view state.")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                    RadrootsSecureIdentityImportField { material in
                        showsIdentityImport = false
                        importIdentity(material)
                    }
                    Spacer()
                }
                .padding()
                .navigationTitle("Import identity")
                .toolbar {
                    ToolbarItem(placement: .cancellationAction) {
                        Button("Cancel") { showsIdentityImport = false }
                    }
                }
            }
            .presentationDetents([.medium])
            .interactiveDismissDisabled()
        }
        .accessibilityIdentifier("radroots.runtime.status")
    }

    private var symbolName: String {
        switch phase {
        case .starting: "leaf"
        case .identityRequired: "person.badge.key"
        case .identityLocked: "lock"
        case .protectedDataUnavailable: "lock.iphone"
        case .recoveryRequired: "wrench.and.screwdriver"
        case .corruptIdentity: "exclamationmark.shield"
        case .configurationReconfigurationRequired: "arrow.triangle.2.circlepath"
        case .running: "checkmark.circle"
        case .failed: "exclamationmark.triangle"
        case .stopped: "pause.circle"
        }
    }

    private var symbolColor: Color {
        switch phase {
        case .failed, .corruptIdentity: .red
        case .configurationReconfigurationRequired: .orange
        case .running: .green
        default: .accentColor
        }
    }

    private var title: String {
        switch phase {
        case .starting: "Starting Radroots"
        case .identityRequired: "Set up your identity"
        case .identityLocked: "Unlock your identity"
        case .protectedDataUnavailable: "Unlock this device"
        case .recoveryRequired: "Recover your identity"
        case .corruptIdentity: "Identity data needs repair"
        case .configurationReconfigurationRequired: "Network configuration changed"
        case .running: "Radroots is ready"
        case .failed: "Radroots needs attention"
        case .stopped: "Radroots is paused"
        }
    }

    private var detail: String {
        switch phase {
        case .starting:
            "Preparing your local Radroots data."
        case .identityRequired:
            "Create or import an identity to connect your local food network."
        case .identityLocked:
            "Your local Nostr secret remains protected until you explicitly unlock it."
        case .protectedDataUnavailable:
            "Protected local data is unavailable while this device is locked."
        case let .recoveryRequired(identity):
            identity.recoveryCode ?? "A previous identity operation needs recovery."
        case let .corruptIdentity(identity):
            identity.recoveryCode ?? "Stored identity state is corrupt; it was not treated as absent."
        case let .configurationReconfigurationRequired(requirement):
            if let fingerprint = requirement.previousBlossomConfigFingerprint {
                "Review and apply the new network configuration. Existing uploads remain bound to configuration \(fingerprint.prefix(12))…."
            } else {
                "Review and apply the new network configuration. Existing local identity and drafts are preserved."
            }
        case let .running(snapshot):
            "Runtime \(snapshot.crateVersion) is connected to your local data."
        case let .failed(failure):
            failure.safeMessage
        case .stopped:
            "Your durable local work is safe."
        }
    }
}

private struct RadrootsSecureIdentityImportField: UIViewRepresentable {
    let submit: @MainActor (RadrootsIdentitySecretMaterial) -> Void

    func makeCoordinator() -> Coordinator {
        Coordinator(submit: submit)
    }

    func makeUIView(context: Context) -> UIView {
        let field = UITextField()
        field.borderStyle = .roundedRect
        field.isSecureTextEntry = true
        field.textContentType = .password
        field.autocapitalizationType = .none
        field.autocorrectionType = .no
        field.spellCheckingType = .no
        field.returnKeyType = .done
        field.placeholder = "nsec1… or secret hex"
        field.accessibilityIdentifier = "radroots.identity.import.secret"
        field.delegate = context.coordinator

        let button = UIButton(type: .system)
        var configuration = UIButton.Configuration.filled()
        configuration.title = "Import securely"
        button.configuration = configuration
        button.accessibilityIdentifier = "radroots.identity.import.submit"
        button.addTarget(context.coordinator, action: #selector(Coordinator.submitIdentity), for: .touchUpInside)

        let error = UILabel()
        error.font = .preferredFont(forTextStyle: .footnote)
        error.textColor = .secondaryLabel
        error.numberOfLines = 0
        error.accessibilityIdentifier = "radroots.identity.import.error"

        let stack = UIStackView(arrangedSubviews: [field, button, error])
        stack.axis = .vertical
        stack.spacing = 12
        stack.translatesAutoresizingMaskIntoConstraints = false
        let container = UIView()
        container.addSubview(stack)
        NSLayoutConstraint.activate([
          stack.leadingAnchor.constraint(equalTo: container.leadingAnchor),
          stack.trailingAnchor.constraint(equalTo: container.trailingAnchor),
          stack.topAnchor.constraint(equalTo: container.topAnchor),
        ])
        context.coordinator.field = field
        context.coordinator.errorLabel = error
        return container
    }

    func updateUIView(_: UIView, context _: Context) {}

    @MainActor
    final class Coordinator: NSObject, UITextFieldDelegate {
        weak var field: UITextField?
        weak var errorLabel: UILabel?
        private let submit: @MainActor (RadrootsIdentitySecretMaterial) -> Void

        init(submit: @escaping @MainActor (RadrootsIdentitySecretMaterial) -> Void) {
            self.submit = submit
        }

        func textFieldShouldReturn(_: UITextField) -> Bool {
            submitIdentity()
            return false
        }

        @objc func submitIdentity() {
            guard let field else { return }
            let input = field.text ?? ""
            field.text = nil
            do {
                let material = try RadrootsIdentitySecretMaterial(importText: input)
                errorLabel?.text = nil
                submit(material)
            } catch {
                errorLabel?.text = "Enter a valid Nostr secret key."
                field.becomeFirstResponder()
            }
        }
    }
}
