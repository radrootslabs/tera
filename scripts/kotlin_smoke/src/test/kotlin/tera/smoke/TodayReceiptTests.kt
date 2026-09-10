package tera.smoke

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import uniffi.tera_core.FfiConverterTypeFfiTodaySyncRecord
import uniffi.tera_core.FfiTodayProjectionUpdate
import uniffi.tera_core.FfiTodayRefreshRecord
import uniffi.tera_core.FfiTodayRelaySyncState
import uniffi.tera_core.FfiTodaySyncRecord
import uniffi.tera_core.FfiTodaySyncTermination
import uniffi.tera_core.FfiTodayTargetPageSummary
import uniffi.tera_core.FfiTodayTargetSyncRecord
import uniffi.tera_core.FfiTodayTargetSyncState

class TodayReceiptTests {
    @Test
    fun generatedNativeBufferRoundTripPreservesIncompleteAndUnknownTargetEvidence() {
        for (state in FfiTodayTargetSyncState.entries) {
            for (termination in FfiTodaySyncTermination.entries) {
                val receipt = FfiTodaySyncRecord(
                    schemaVersion = 1u, relayState = FfiTodayRelaySyncState.PARTIAL,
                    termination = termination,
                    targets = listOf(
                        FfiTodayTargetSyncRecord(
                            "opaque", FfiTodayTargetSyncState.COMPLETE,
                            FfiTodayTargetPageSummary(8u, 1u, 2u, state),
                        ),
                        FfiTodayTargetSyncRecord("unknown", null, null),
                    ),
                    pagesFetched = 8u, eventsObserved = ULong.MAX_VALUE,
                    eventsAdmitted = ULong.MAX_VALUE - 1uL, eventsRejected = 1uL,
                    projection = FfiTodayRefreshRecord(
                        1u, FfiTodayProjectionUpdate.REBUILD, 501uL, 400uL,
                        20uL, 70uL, ULong.MAX_VALUE, true,
                    ),
                )
                val lifted = FfiConverterTypeFfiTodaySyncRecord.lift(
                    FfiConverterTypeFfiTodaySyncRecord.lower(receipt),
                )
                assertEquals(receipt, lifted)
                assertEquals(state, lifted.targets.first().summary?.lastIncomplete)
                assertNull(lifted.targets.last().summary)
                assertNull(lifted.targets.last().finalState)
            }
        }
    }
}
