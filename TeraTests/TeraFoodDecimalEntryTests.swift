import Foundation
@testable import TeraApp
import XCTest

final class TeraFoodDecimalEntryTests: XCTestCase {
  func testExactLocaleTranslationNeverRoundsOrInfersUnits() {
    for (locale, raw, expected) in [
      ("fr_FR", "0004,500", "4.5"), ("de_DE", "0,000000000000000000000000001", "0.000000000000000000000000001"),
      ("en_US", "12345678901234567890.12345678", "12345678901234567890.12345678"),
      ("en_US", "000.000", "0"), ("ar_EG", "٤٫٥٠", "4.5"), ("fa_IR", "۱۲٫۵۰", "12.5"),
      ("fr_FR", "4.5", "4.5"), ("en_US", "9.9900", "9.99"),
    ] {
      XCTAssertEqual(TeraFoodDecimalEntry.canonicalOrRaw(raw, locale: Locale(identifier: locale)), expected, raw)
    }
  }

  func testIncompleteOrAmbiguousEntriesStayUnchangedForSharedValidation() {
    for raw in ["", "-", "1,", ",5", "1.000,5", "1,000.5", "1,2,3", "1 000", "1\u{202F}000", "1e3", "+1", " 1 ", "⅕", "²"] {
      XCTAssertEqual(TeraFoodDecimalEntry.canonicalOrRaw(raw, locale: Locale(identifier: "fr_FR")), raw)
    }
    XCTAssertNil(TeraFoodDecimalEntry.canonicalOrRaw(nil, locale: Locale(identifier: "en_US")))
    XCTAssertEqual(TeraFoodDecimalEntry.canonicalOrRaw("1,5", locale: Locale(identifier: "en_US")), "1,5")
  }

  func testOnlyFoodAmountsAreTranslatedAndContractFieldsArePreserved() {
    var form = TeraAddForm.empty(.createFoodAvailability)
    form.priceAmount = "4,50"
    form.quantity = "0002,00"
    form.unit = "bag"
    form.currency = "ZZZ"
    form.foodStatus = "sold"
    var expected = form
    expected.priceAmount = "4.5"
    expected.quantity = "2"
    XCTAssertEqual(TeraFoodDecimalEntry.form(form, locale: Locale(identifier: "fr_FR")), expected)
    form.commandType = .createUpdate
    XCTAssertEqual(TeraFoodDecimalEntry.form(form, locale: Locale(identifier: "fr_FR")), form)
  }
}

extension TeraAddStoreTests {
  @MainActor
  func testAutomaticRevisionPreservationKeepsRawEntryAndItsRetryInterlock() async throws {
    let backend = AddBackend()
    let client = try await Self.startedClient(backend)
    let store = TeraAddStore(runtimeClient: client, initialType: .createFoodAvailability)
    await store.configure(snapshot: backend.snapshot())
    await store.start()
    configure(store, type: .createFoodAvailability)
    let original = try await backend.saveAddIntent(input: TeraAddRuntimeInput(form: store.form, media: []),
                                                   existingDraftID: nil, expectedRevision: nil)
    await store.retractAndRevise(Self.card(localOperationID: original.id))
    store.updateForm(\.priceAmount, "4.50")
    await backend.failNextRevisionReceipt()
    store.newDraft(type: .createUpdate)
    await TeraScopeFixtures.eventually { !store.protection.isWorking }
    XCTAssertTrue(store.protection.failed)
    XCTAssertTrue(store.revisionPreparation.isCaptured)
    XCTAssertEqual(store.form.priceAmount, "4.50")
    store.protection.retry()
    await TeraScopeFixtures.eventually { !store.protection.isWorking }
    XCTAssertFalse(store.protection.failed)
    XCTAssertEqual(store.form.commandType, .createUpdate)
    let retained = await backend.values
    XCTAssertEqual(retained.values.first(where: \.isRevision)?.form?.priceAmount, "4.50")
    let count = await backend.revisionPlanCount()
    XCTAssertEqual(count, 1)
    store.stop()
    _ = try await client.stop()
  }

  @MainActor
  func testFoodRevisionSaveTranslatesOnceAndLostReceiptRetainsExactAmounts() async throws {
    let backend = AddBackend()
    let client = try await Self.startedClient(backend)
    let store = TeraAddStore(runtimeClient: client, initialType: .createFoodAvailability)
    await store.configure(snapshot: backend.snapshot())
    await store.start()
    configure(store, type: .createFoodAvailability)
    let original = try await backend.saveAddIntent(input: TeraAddRuntimeInput(form: store.form, media: []),
                                                   existingDraftID: nil, expectedRevision: nil)
    await store.retractAndRevise(Self.card(localOperationID: original.id))
    store.updateForm(\.priceAmount, "4,50")
    store.updateForm(\.quantity, "002,00")
    await backend.failNextRevisionReceipt()
    await store.save(locale: Locale(identifier: "fr_FR"))
    XCTAssertTrue(store.revisionPreparation.isCaptured)
    XCTAssertEqual(store.form.priceAmount, "4.5")
    XCTAssertEqual(store.form.quantity, "2")
    store.updateForm(\.priceAmount, "9,00")
    await store.save(locale: Locale(identifier: "en_US"))
    XCTAssertTrue(store.activeDraft?.isRevision == true)
    XCTAssertEqual(store.activeDraft?.form?.priceAmount, "4.5")
    XCTAssertEqual(store.activeDraft?.form?.quantity, "2")
    let count = await backend.revisionPlanCount()
    XCTAssertEqual(count, 1)
    store.stop()
    _ = try await client.stop()
  }

  @MainActor
  func testFoodDraftStaysRawUntilNewSubmitAndRetryRetainsCapturedAmounts() async throws {
    let backend = AddBackend()
    let client = try await Self.startedClient(backend)
    let store = TeraAddStore(runtimeClient: client, initialType: .createFoodAvailability)
    await store.configure(snapshot: backend.snapshot())
    await store.start()
    configure(store, type: .createFoodAvailability)
    store.updateForm(\.priceAmount, "4,50")
    store.updateForm(\.quantity, "002,00")
    await store.save()
    XCTAssertEqual(store.savedComposer?.form.priceAmount, "4,50")
    XCTAssertEqual(store.form.quantity, "002,00")
    await store.submit(locale: Locale(identifier: "fr_FR"))
    let captured = try XCTUnwrap(store.submissions.status?.captured)
    XCTAssertEqual(captured.form.priceAmount, "4.5")
    XCTAssertEqual(captured.form.quantity, "2")
    XCTAssertEqual(captured.form.editingValue, store.form)
    store.updateForm(\.priceAmount, "9,00")
    await store.submit(locale: Locale(identifier: "en_US"))
    XCTAssertEqual(store.submissions.status?.captured, captured)
    XCTAssertEqual(store.form.priceAmount, "9,00")
    store.newDraft(type: .createFoodAvailability)
    await TeraScopeFixtures.eventually { !store.protection.isWorking }
    configure(store, type: .createFoodAvailability)
    store.updateForm(\.priceAmount, "4,50")
    store.updateForm(\.quantity, "002,00")
    await store.submit(locale: Locale(identifier: "fr_FR"))
    let next = try XCTUnwrap(store.submissions.status?.captured)
    XCTAssertNotEqual(next.id, captured.id)
    XCTAssertNotEqual(next.form.identifier, captured.form.identifier)
    XCTAssertEqual(next.form.priceAmount, captured.form.priceAmount)
    store.stop()
    _ = try await client.stop()
  }
}
