package tera.smoke

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertNotEquals
import kotlin.test.assertTrue
import kotlinx.coroutines.async
import kotlinx.coroutines.runBlocking
import uniffi.tera_core.FfiAddCommandType
import uniffi.tera_core.FfiComposerFormRecord
import uniffi.tera_core.FfiComposerSaveRequest
import uniffi.tera_core.FfiComposerScopeRecord
import uniffi.tera_core.FfiConverterTypeFfiSubmissionReservationReceipt
import uniffi.tera_core.FfiRecoveryDisposition
import uniffi.tera_core.FfiSubmissionReservationRequest
import uniffi.tera_core.TeraAppException
import uniffi.tera_core.classifyErrorRecovery
import uniffi.tera_core.composerReserveId
import uniffi.tera_core.submissionReserveId

class SubmissionReservationTests {
    private val scope = FfiComposerScopeRecord(1u, PUBLIC_KEY, "nearby")
    private fun savedRequest() = FfiComposerSaveRequest(
        schemaVersion = 1u, scope = scope, id = composerReserveId().id,
        expectedRevision = null, editSequence = ULong.MAX_VALUE - 1uL,
        form = FfiComposerFormRecord(
            schemaVersion = 1u, commandType = FfiAddCommandType.CREATE_EVENT,
            content = "PRIVATE incomplete\n\u0000é", identifier = null, title = null, summary = null,
            location = null, eventTiming = null, eventStartDate = "2026-09-", eventEndDate = null,
            eventStartUnixS = null, eventEndUnixS = null, eventTimezone = null, priceAmount = "-",
            currency = null, unit = null, quantity = null, foodPublishedAtUnixS = null,
            foodStatus = null, media = emptyList(),
        ),
    )

    @Test
    fun reservationRecoversOneHistoricalSourceAcrossConcurrencyAndRestart(): Unit = runBlocking {
        SmokeFixture().use { fixture ->
            val input = savedRequest()
            val request = FfiSubmissionReservationRequest(1u, submissionReserveId().id, scope, input.id, 1uL)
            val runtime = fixture.runtime()
            val first = try {
                val saved = runtime.composerSave(input)
                val left = async { runtime.submissionReserve(request) }
                val right = async { runtime.submissionReserve(request) }
                val a = left.await()
                val b = right.await()
                assertEquals(a.reservationId, b.reservationId)
                assertNotEquals(a.replayed, b.replayed)
                assertEquals(saved.draft, a.captured)
                assertEquals(a, FfiConverterTypeFfiSubmissionReservationReceipt.lift(
                    FfiConverterTypeFfiSubmissionReservationReceipt.lower(a)))
                runtime.composerSave(input.copy(expectedRevision = 1uL, editSequence = ULong.MAX_VALUE,
                    form = input.form.copy(content = "later edit")))
                a
            } finally {
                runtime.shutdown()
                runtime.close()
            }
            val reopened = fixture.runtime()
            try {
                val replay = reopened.submissionReserve(request)
                assertTrue(replay.replayed)
                assertEquals(first.reservationId, replay.reservationId)
                assertEquals(first.reservedAtUnixMs, replay.reservedAtUnixMs)
                assertEquals(first.captured, replay.captured)
                assertEquals(2uL, reopened.composerLoad(scope, input.id).revision)
                val changed = request.copy(expectedRevision = 2uL)
                val failure = assertFailsWith<TeraAppException.Failure> { reopened.submissionReserve(changed) }
                assertEquals("idempotency_conflict", failure.report.code)
                assertEquals(FfiRecoveryDisposition.IDEMPOTENCY_CONFLICT,
                    classifyErrorRecovery(1u, failure.report.code).disposition)
                val second = reopened.submissionReserve(changed.copy(commandId = submissionReserveId().id))
                val third = reopened.submissionReserve(changed.copy(commandId = submissionReserveId().id))
                assertEquals(second.captured, third.captured)
                assertNotEquals(second.reservationId, third.reservationId)
                assertTrue(reopened.phase1DraftHeads(100u).isEmpty())
            } finally {
                reopened.shutdown()
                reopened.close()
            }
        }
    }

    @Test
    fun invalidReservationVersionsAndUnsignedWidthsFailClosed(): Unit = runBlocking {
        SmokeFixture().use { fixture ->
            val runtime = fixture.runtime()
            try {
                val input = savedRequest()
                runtime.composerSave(input)
                val request = FfiSubmissionReservationRequest(1u, submissionReserveId().id, scope, input.id, 1uL)
                for ((invalid, code) in listOf(
                    request.copy(schemaVersion = 2u) to "submission_schema_unsupported",
                    request.copy(scope = scope.copy(schemaVersion = 2u)) to "composer_schema_unsupported",
                    request.copy(commandId = "00".repeat(16)) to "submission_command_id_invalid",
                    request.copy(expectedRevision = ULong.MAX_VALUE) to "composer_revision_invalid",
                )) {
                    val failure = assertFailsWith<TeraAppException.Failure> { runtime.submissionReserve(invalid) }
                    assertEquals(code, failure.report.code)
                    assertFalse(failure.report.retryable)
                }
                assertFalse(runtime.submissionReserve(request).replayed)
            } finally {
                runtime.shutdown()
                runtime.close()
            }
        }
    }
}
