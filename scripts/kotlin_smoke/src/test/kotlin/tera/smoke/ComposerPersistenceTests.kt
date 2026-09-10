package tera.smoke

import java.nio.ByteBuffer
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue
import kotlinx.coroutines.runBlocking
import uniffi.tera_core.FfiAddCommandType
import uniffi.tera_core.FfiComposerFormRecord
import uniffi.tera_core.FfiComposerListEntry
import uniffi.tera_core.FfiComposerMediaRecord
import uniffi.tera_core.FfiComposerRepairReason
import uniffi.tera_core.FfiComposerSaveRequest
import uniffi.tera_core.FfiComposerScopeRecord
import uniffi.tera_core.FfiConverterTypeFfiComposerListEntry
import uniffi.tera_core.FfiConverterTypeFfiComposerSaveReceipt
import uniffi.tera_core.FfiEventTimingKind
import uniffi.tera_core.FfiRecoveryDisposition
import uniffi.tera_core.TeraAppException
import uniffi.tera_core.classifyErrorRecovery
import uniffi.tera_core.composerReserveId

class ComposerPersistenceTests {
    private val scope = FfiComposerScopeRecord(1u, PUBLIC_KEY, "nearby")

    private fun request() = FfiComposerSaveRequest(
        schemaVersion = 1u, scope = scope, id = composerReserveId().id,
        expectedRevision = null, editSequence = ULong.MAX_VALUE - 1uL,
        form = FfiComposerFormRecord(
            schemaVersion = 1u,
            commandType = FfiAddCommandType.CREATE_EVENT,
            content = "  private incomplete\n\u0000é  ",
            identifier = null,
            title = null,
            summary = null,
            location = null,
            eventTiming = FfiEventTimingKind.ALL_DAY,
            eventStartDate = "2026-09-",
            eventEndDate = "",
            eventStartUnixS = ULong.MAX_VALUE,
            eventEndUnixS = null,
            eventTimezone = "Mars/unfinished",
            priceAmount = "12.",
            currency = null,
            unit = null,
            quantity = "-",
            foodPublishedAtUnixS = null,
            foodStatus = null,
            media = listOf(FfiComposerMediaRecord(1u, "media:abc", "ab".repeat(32), "image/png", 24uL, 2u, 2u, "", UNIX_S)),
        ),
    )

    @Test
    fun actualNativeComposerPreservesPartialFormsExactUnsignedReceiptsAndRestart(): Unit = runBlocking {
        SmokeFixture().use { fixture ->
            val runtime = fixture.runtime()
            val input = request()
            val saved = try {
                val first = runtime.composerSave(input)
                assertEquals(1uL, first.draft.revision)
                assertEquals(scope, first.draft.scope)
                assertEquals(input.id, first.draft.id)
                assertEquals(input.form, first.draft.form)
                assertEquals(ULong.MAX_VALUE - 1uL, first.draft.editSequence)
                assertFalse(first.replayed)
                val next = input.copy(expectedRevision = 1uL, editSequence = ULong.MAX_VALUE,
                    form = input.form.copy(content = "newest incomplete"))
                val receipt = runtime.composerSave(next)
                assertEquals(2uL, receipt.draft.revision)
                assertEquals(ULong.MAX_VALUE, receipt.draft.editSequence)
                assertEquals(next.form, receipt.draft.form)
                assertEquals(receipt, FfiConverterTypeFfiComposerSaveReceipt.lift(
                    FfiConverterTypeFfiComposerSaveReceipt.lower(receipt)))
                val stale = assertFailsWith<TeraAppException.Failure> { runtime.composerSave(next) }
                assertEquals("composer_revision_conflict", stale.report.code)
                assertEquals(listOf("reload_composer"), stale.report.recoveryActions)
                assertEquals(FfiRecoveryDisposition.STALE_REVISION, classifyErrorRecovery(1u, stale.report.code).disposition)
                assertTrue(runtime.phase1DraftHeads(100u).isEmpty())
                receipt
            } finally {
                runtime.shutdown()
                runtime.close()
            }
            val reopened = fixture.runtime()
            try {
                assertEquals(saved.draft, reopened.composerLoad(scope, input.id))
                val second = reopened.composerSave(request())
                val page = reopened.composerList(1u, scope, 1u, null)
                assertEquals(scope, page.scope)
                assertEquals(1, page.entries.size)
                val cursor = assertNotNull(page.nextCursor)
                val last = reopened.composerList(1u, scope, 1u, cursor)
                assertNull(last.nextCursor)
                assertEquals(setOf(input.id, second.draft.id), (page.entries + last.entries).map {
                    (it as FfiComposerListEntry.Draft).summary.id
                }.toSet())
                val foreign = scope.copy(localNetworkId = "elsewhere")
                assertEquals("composer_scope_mismatch",
                    assertFailsWith<TeraAppException.Failure> { reopened.composerLoad(foreign, input.id) }.report.code)
                assertEquals("composer_scope_mismatch",
                    assertFailsWith<TeraAppException.Failure> { reopened.composerList(1u, foreign, 1u, cursor) }.report.code)
                assertTrue(reopened.composerList(1u, foreign, 10u, null).entries.isEmpty())
            } finally {
                reopened.shutdown()
                reopened.close()
            }
        }
    }

    @Test
    fun unsupportedNestedVersionsAndMalformedVariantsFailClosed(): Unit = runBlocking {
        SmokeFixture().use { fixture ->
            val runtime = fixture.runtime()
            try {
                val input = request()
                for (invalid in listOf(
                    input.copy(schemaVersion = 2u),
                    input.copy(scope = scope.copy(schemaVersion = 2u)),
                    input.copy(form = input.form.copy(schemaVersion = 2u)),
                    input.copy(form = input.form.copy(media = listOf(input.form.media[0].copy(schemaVersion = 2u)))),
                )) {
                    val failure = assertFailsWith<TeraAppException.Failure> { runtime.composerSave(invalid) }
                    assertEquals(1u.toUShort(), failure.report.schemaVersion)
                    assertEquals("composer_schema_unsupported", failure.report.code)
                    assertFalse(failure.report.retryable)
                }
                assertTrue(runtime.composerList(1u, scope, 10u, null).entries.isEmpty())
                val repair = FfiComposerListEntry.Repair("00".repeat(16), ULong.MAX_VALUE, FfiComposerRepairReason.CORRUPT_RECORD)
                assertEquals(repair, FfiConverterTypeFfiComposerListEntry.lift(FfiConverterTypeFfiComposerListEntry.lower(repair)))
                for (bytes in listOf(byteArrayOf(), byteArrayOf(0, 0, 0, 99), byteArrayOf(0, 0, 0, 1))) {
                    assertFailsWith<RuntimeException> { FfiConverterTypeFfiComposerListEntry.read(ByteBuffer.wrap(bytes)) }
                }
            } finally {
                runtime.shutdown()
                runtime.close()
            }
        }
    }
}
