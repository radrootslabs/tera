package tera.smoke

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertTrue
import kotlinx.coroutines.runBlocking
import uniffi.tera_core.FfiLocalNetworkRecord
import uniffi.tera_core.FfiTodayProjectionUpdate
import uniffi.tera_core.TeraAppException

class SearchAdmissionTests {
    @Test
    fun installedSearchPreservesTypedByteAdmissionAndClosedPrecedence(): Unit = runBlocking {
        SmokeFixture().use { fixture ->
            val runtime = fixture.runtime()
            val context = FfiLocalNetworkRecord(1u, "nearby", "Nearby", listOf("wss://relay.example"), null, emptyList(), 1uL)
            try {
                runtime.phase1RefreshToday(context, UNIX_S, FfiTodayProjectionUpdate.REBUILD)
                for (query in listOf(
                    "x".repeat(257), "é".repeat(128) + "a",
                    " ".repeat(1_048_576) + "carrots", "x".repeat(1_048_576),
                    "   ", "\ncarrots", "carrots\u0000", "İ".repeat(86),
                )) {
                    val failure = assertFailsWith<TeraAppException.Failure> {
                        runtime.phase1Search(context, query, 20u, UNIX_S, "UTC")
                    }
                    assertEquals("today_invalid_request", failure.report.code)
                    assertFalse(failure.report.retryable)
                    assertEquals(listOf("correct_input"), failure.report.recoveryActions)
                }
                for (query in listOf("x".repeat(256), "é".repeat(128), "İ".repeat(85) + "a", "  CARROT  ")) {
                    assertTrue(runtime.phase1Search(context, query, 20u, UNIX_S, "UTC").isEmpty())
                }
                runtime.shutdown()
                val failure = assertFailsWith<TeraAppException.Failure> {
                    runtime.phase1Search(context, "x".repeat(257), 20u, UNIX_S, "UTC")
                }
                assertEquals("client_closed", failure.report.code)
            } finally {
                runtime.shutdown()
                runtime.close()
            }
        }
    }
}
