package tera.smoke

import java.nio.ByteBuffer
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import uniffi.tera_core.FfiCalendarTiming
import uniffi.tera_core.FfiCivilDate
import uniffi.tera_core.FfiConverterTypeFfiCalendarTiming
import uniffi.tera_core.FfiConverterTypeFfiTodayPageRecord
import uniffi.tera_core.FfiTodayPageRecord
import uniffi.tera_core.FfiViewerCalendarContext

class CalendarTimingTests {
    @Test
    fun generatedPageBufferRetainsFrozenViewerCalendarContext() {
        val value = FfiTodayPageRecord(
            calendar = FfiViewerCalendarContext(
                schemaVersion = 1u, asOfUnixS = 1788569400uL,
                timeZone = "America/Vancouver", civilDate = FfiCivilDate(2026u, 9u, 4u),
            ),
            projectionGeneration = 9uL, schemaVersion = 2u,
            asOfUnixS = 1788569400uL, items = emptyList(), nextCursor = "opaque",
        )
        assertEquals(value, FfiConverterTypeFfiTodayPageRecord.lift(FfiConverterTypeFfiTodayPageRecord.lower(value)))
    }

    @Test
    fun generatedNativeBuffersPreserveCivilDatesAndExclusiveEnds() {
        for (value in listOf(
            FfiCalendarTiming.DateBased(FfiCivilDate(1u, 1u, 1u), null),
            FfiCalendarTiming.DateBased(FfiCivilDate(2024u, 2u, 29u), FfiCivilDate(2024u, 3u, 1u)),
            FfiCalendarTiming.DateBased(FfiCivilDate(2026u, 9u, 5u), FfiCivilDate(2026u, 9u, 7u)),
            FfiCalendarTiming.DateBased(FfiCivilDate(9999u, 12u, 31u), null),
        )) {
            assertEquals(value, FfiConverterTypeFfiCalendarTiming.lift(FfiConverterTypeFfiCalendarTiming.lower(value)))
        }
    }

    @Test
    fun generatedNativeBuffersPreserveEveryUnsignedInstantBitAndSourceZone() {
        for (start in listOf(0uL, (1uL shl 53) + 1uL, ULong.MAX_VALUE - 1uL, ULong.MAX_VALUE)) {
            for (zones in listOf(false, true)) {
                val value = FfiCalendarTiming.TimeBased(
                    start, if (start == ULong.MAX_VALUE) null else start + 1uL,
                    if (zones) "America/Vancouver" else null,
                    if (zones) "Europe/Paris" else null,
                )
                assertEquals(value, FfiConverterTypeFfiCalendarTiming.lift(FfiConverterTypeFfiCalendarTiming.lower(value)))
            }
        }
    }

    @Test
    fun unknownVariantsAndTruncatedGeneratedValuesFailWithoutFabricatingDates() {
        for (bytes in listOf(byteArrayOf(0, 0, 0, 99), byteArrayOf(0, 0, 0, 1), byteArrayOf())) {
            assertFailsWith<RuntimeException> { FfiConverterTypeFfiCalendarTiming.read(ByteBuffer.wrap(bytes)) }
        }
    }
}
