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
import uniffi.tera_core.FfiConverterTypeFfiLegacyDraftListEntry
import uniffi.tera_core.FfiConverterTypeFfiLegacyDraftPageRecord
import uniffi.tera_core.FfiLegacyDraftListEntry
import uniffi.tera_core.FfiLegacyDraftRepairReason
import uniffi.tera_core.TeraAppException

class LegacyInventoryTests {
    @Test
    fun nativeLegacyPagesPreserveOperationsAcrossRestartAndRejectMalformedCursors(): Unit = runBlocking {
        SmokeFixture().use { fixture ->
            val runtime = fixture.runtime()
            val input = fixture.draft(emptyList()).copy(commandType = FfiAddCommandType.CREATE_UPDATE)
            val saved = (1..2).map { runtime.phase1SaveAddIntent(input, null, null) }
            val first = try {
                val page = runtime.legacyDraftPage(1u, 1u, null)
                assertEquals(PUBLIC_KEY, page.authorPublicKey)
                assertEquals(1u.toUShort(), page.schemaVersion)
                assertEquals(1, page.entries.size)
                assertNotNull(page.nextCursor)
                val summary = (page.entries.single() as FfiLegacyDraftListEntry.Draft).summary
                assertTrue(summary.hasForm)
                assertFalse(summary.isRevision)
                assertEquals(0uL, summary.mediaCount)
                assertEquals(0uL, summary.verifiedMediaCount)
                assertEquals(0uL, summary.possibleOrphanCount)
                assertNull(summary.settlement)
                assertEquals(page, FfiConverterTypeFfiLegacyDraftPageRecord.lift(FfiConverterTypeFfiLegacyDraftPageRecord.lower(page)))
                assertEquals("unsupported_schema_version", assertFailsWith<TeraAppException.Failure> {
                    runtime.legacyDraftPage(2u, 1u, null)
                }.report.code)
                assertEquals("draft_inventory_invalid", assertFailsWith<TeraAppException.Failure> {
                    runtime.legacyDraftPage(1u, 0u, null)
                }.report.code)
                for (cursor in listOf("invalid", page.nextCursor!!.uppercase(), page.nextCursor!! + "0")) {
                    assertEquals("draft_inventory_cursor_invalid", assertFailsWith<TeraAppException.Failure> {
                        runtime.legacyDraftPage(1u, 1u, cursor)
                    }.report.code)
                }
                page
            } finally {
                runtime.shutdown()
                runtime.close()
            }
            val reopened = fixture.runtime()
            try {
                val last = reopened.legacyDraftPage(1u, 1u, first.nextCursor)
                assertNull(last.nextCursor)
                assertEquals(saved.map { it.draftId }.toSet(), (first.entries + last.entries).map {
                    (it as FfiLegacyDraftListEntry.Draft).summary.draftId
                }.toSet())
                for (draft in saved) assertEquals(draft, reopened.phase1DraftStatus(draft.draftId))
                for (reason in FfiLegacyDraftRepairReason.entries) {
                    val repair = FfiLegacyDraftListEntry.Repair("00".repeat(16), ULong.MAX_VALUE, reason)
                    assertEquals(repair, FfiConverterTypeFfiLegacyDraftListEntry.lift(FfiConverterTypeFfiLegacyDraftListEntry.lower(repair)))
                }
                for (bytes in listOf(byteArrayOf(), byteArrayOf(0, 0, 0, 99), byteArrayOf(0, 0, 0, 1))) {
                    assertFailsWith<RuntimeException> { FfiConverterTypeFfiLegacyDraftListEntry.read(ByteBuffer.wrap(bytes)) }
                }
            } finally {
                reopened.shutdown()
                reopened.close()
            }
        }
    }
}
