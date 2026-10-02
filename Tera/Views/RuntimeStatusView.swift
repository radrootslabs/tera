import RadrootsKit
import SwiftUI
import UIKit

struct RuntimeStatusView: View {
    let phase: TeraAppModel.Phase
    let retry: () -> Void
    let createIdentity: () -> Void
    let importIdentity: (RadrootsIdentitySecretMaterial) -> Void
    let unlockIdentity: () -> Void
    let recoverIdentity: () -> Void
    let applyConfigurationReconfiguration: () -> Void
    @State private var showsIdentityImport = false
    @Environment(\.dynamicTypeSize) private var dynamicTypeSize

    var body: some View {
        NavigationStack {
            ScrollView {
              VStack(spacing: 20) {
                Image(systemName: symbolName)
                    .font(.system(size: 48, weight: .medium))
                    .foregroundStyle(symbolColor)
                    .accessibilityHidden(true)
                Text(title)
                    .font(.title2.weight(.semibold))
                    .accessibilityAddTraits(.isHeader)
                Text(detail)
                    .font(.body)
                    .foregroundStyle(.primary)
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
                } else if canRecheckLocalState {
                    Button("Check local state again", action: retry)
                        .buttonStyle(.borderedProminent)
                        .accessibilityIdentifier("tera.runtime.recheck")
                }
              }
              .frame(maxWidth: .infinity)
              .padding(24)
            }
            .navigationTitle("Tera")
            .navigationBarTitleDisplayMode(dynamicTypeSize.isAccessibilitySize ? .inline : .automatic)
            .teraReadableScrollEdges(dynamicTypeSize.isAccessibilitySize)
            .tint(.primary)
            .buttonBorderShape(.roundedRectangle(radius: 12))
        }
        .sheet(isPresented: $showsIdentityImport) {
            NavigationStack {
                Form {
                  Section {
                    importInstruction("Enter an nsec or 64-character secret key.")
                    importInstruction("It is transferred directly to Apple custody.")
                    importInstruction("Secret input is never stored in view state.")
                  }
                  Section {
                    TeraSecureIdentityImportField { material in
                        showsIdentityImport = false
                        importIdentity(material)
                    }
                  }
                }
                .navigationTitle("Import identity")
                .navigationBarTitleDisplayMode(.inline)
                .teraReadableScrollEdges(dynamicTypeSize.isAccessibilitySize)
                .toolbar {
                    ToolbarItem(placement: .cancellationAction) {
                        Button { showsIdentityImport = false } label: {
                            Label("Cancel", systemImage: "xmark")
                                .labelStyle(.iconOnly)
                                .frame(minWidth: 44, minHeight: 44)
                        }
                    }
                }
            }
            .tint(.primary)
            .presentationDetents(dynamicTypeSize.isAccessibilitySize ? [.large] : [.medium, .large])
            .interactiveDismissDisabled()
        }
        .accessibilityIdentifier("radroots.runtime.status")
    }

    private func importInstruction(_ text: String) -> some View {
        TeraIdentityImportInstruction(text: text)
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

    var canRecheckLocalState: Bool {
        switch phase {
        case .protectedDataUnavailable, .corruptIdentity, .stopped: true
        default: false
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
        case .starting: "Starting Tera"
        case .identityRequired: "Set up your identity"
        case .identityLocked: "Unlock your identity"
        case .protectedDataUnavailable: "Unlock this device"
        case .recoveryRequired: "Recover your identity"
        case .corruptIdentity: "Identity data needs repair"
        case .configurationReconfigurationRequired: "Network configuration changed"
        case .running: "Tera is ready"
        case .failed: "Tera needs attention"
        case .stopped: "Tera is paused"
        }
    }

    private var detail: String {
        switch phase {
        case .starting:
            "Preparing your local Tera data."
        case .identityRequired:
            "Create or import an identity to connect your local food network."
        case .identityLocked:
            "Your local Nostr secret remains protected until you explicitly unlock it."
        case .protectedDataUnavailable:
            "Protected local data is unavailable while this device is locked."
        case .recoveryRequired:
            TeraUserMessages.text(.secureStateUnavailable)
        case .corruptIdentity:
            TeraUserMessages.text(.secureStateUnavailable)
        case .configurationReconfigurationRequired:
            "Review and apply the new network configuration. Existing local identity and drafts are preserved."
        case let .running(snapshot):
            "Runtime \(snapshot.crateVersion) is connected to your local data."
        case let .failed(failure):
            TeraUserMessages.text(for: failure, fallback: .runtimeOperationFailed)
        case .stopped:
            "Your durable local work is safe."
        }
    }
}

struct TeraSecureIdentityImportField: UIViewRepresentable {
    let submit: @MainActor (RadrootsIdentitySecretMaterial) -> Void
    @State private var errorMessage: String?

    func makeCoordinator() -> Coordinator {
        Coordinator(errorChanged: { errorMessage = $0 }, submit: submit)
    }

    func makeUIView(context: Context) -> UIView {
        let field = UITextField()
        field.borderStyle = .roundedRect
        field.font = .preferredFont(forTextStyle: .body)
        field.adjustsFontForContentSizeCategory = true
        field.isSecureTextEntry = true
        field.textContentType = .password
        field.autocapitalizationType = .none
        field.autocorrectionType = .no
        field.spellCheckingType = .no
        field.returnKeyType = .done
        field.placeholder = "Secret key"
        field.accessibilityLabel = "Secret identity key"
        field.accessibilityIdentifier = "radroots.identity.import.secret"
        field.delegate = context.coordinator

        let button = UIButton(type: .system)
        var configuration = UIButton.Configuration.filled()
        configuration.title = "Import securely"
        button.configuration = configuration
        button.titleLabel?.numberOfLines = 0
        button.accessibilityIdentifier = "radroots.identity.import.submit"
        button.addTarget(context.coordinator, action: #selector(Coordinator.submitIdentity), for: .touchUpInside)

        let error = UILabel()
        error.font = .preferredFont(forTextStyle: .footnote)
        error.adjustsFontForContentSizeCategory = true
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
          stack.bottomAnchor.constraint(equalTo: container.bottomAnchor),
          field.heightAnchor.constraint(greaterThanOrEqualToConstant: 44),
          button.heightAnchor.constraint(greaterThanOrEqualToConstant: 44),
        ])
        context.coordinator.field = field
        context.coordinator.errorLabel = error
        return container
    }

    func updateUIView(_ uiView: UIView, context: Context) {
        context.coordinator.errorLabel?.text = errorMessage
        uiView.invalidateIntrinsicContentSize()
        uiView.setNeedsLayout()
    }

    func sizeThatFits(_ proposal: ProposedViewSize, uiView: UIView, context _: Context) -> CGSize? {
        uiView.systemLayoutSizeFitting(
          CGSize(width: proposal.width ?? 320, height: 0),
          withHorizontalFittingPriority: .required,
          verticalFittingPriority: .fittingSizeLevel
        )
    }

    static func dismantleUIView(_: UIView, coordinator: Coordinator) {
        coordinator.field?.text = nil
        coordinator.field?.resignFirstResponder()
        coordinator.field?.delegate = nil
        coordinator.errorLabel?.text = nil
        coordinator.field = nil
        coordinator.errorLabel = nil
    }

    @MainActor
    final class Coordinator: NSObject, UITextFieldDelegate {
        weak var field: UITextField?
        weak var errorLabel: UILabel?
        private let submit: @MainActor (RadrootsIdentitySecretMaterial) -> Void
        private let announce: @MainActor (String) -> Void
        private let errorChanged: @MainActor (String?) -> Void

        init(
          announce: @escaping @MainActor (String) -> Void = {
                UIAccessibility.post(notification: .announcement, argument: $0)
            },
          errorChanged: @escaping @MainActor (String?) -> Void = { _ in },
          submit: @escaping @MainActor (RadrootsIdentitySecretMaterial) -> Void
        ) {
            self.announce = announce
            self.errorChanged = errorChanged
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
                errorChanged(nil)
                submit(material)
            } catch {
                let message = "Enter a valid Nostr secret key."
                errorLabel?.text = message
                errorChanged(message)
                field.becomeFirstResponder()
                announce(message)
            }
        }
    }
}
