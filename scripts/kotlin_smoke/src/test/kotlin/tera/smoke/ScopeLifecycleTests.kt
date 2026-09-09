package tera.smoke

import java.nio.file.Files
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertNotEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.async
import kotlinx.coroutines.cancelAndJoin
import kotlinx.coroutines.delay
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import uniffi.tera_core.FfiInvalidationRevision
import uniffi.tera_core.FfiLocalNetworkRecord
import uniffi.tera_core.FfiMediaOperation
import uniffi.tera_core.FfiRecoveryDisposition
import uniffi.tera_core.FfiRetryDisposition
import uniffi.tera_core.FfiRuntimeChangeKind
import uniffi.tera_core.FfiTodayProjectionUpdate
import uniffi.tera_core.ProtectedDataAvailability
import uniffi.tera_core.TeraAppException
import uniffi.tera_core.classifyErrorRecovery

class ScopeLifecycleTests {
    @Test
    fun scopeAndUnsignedContextsCrossRustWithoutSignedNarrowing(): Unit = runBlocking {
        SmokeFixture().use { fixture ->
            val runtime = fixture.runtime()
            var firstEpoch = ""
            try {
                val observer = SmokeObserver()
                runtime.subscribeChanges(observer).use { subscription ->
                    val initial = observer.next()
                    assertEquals(FfiRuntimeChangeKind.INITIAL, initial.kind)
                    assertEquals(PUBLIC_KEY, initial.scope.publicKey)
                    assertEquals(GENERATION, initial.scope.sourceGeneration)
                    assertNull(initial.scope.context)
                    assertEquals(32, initial.epoch.length)
                    firstEpoch = initial.epoch
                    for ((index, generation) in listOf(0uL, 1uL, Long.MAX_VALUE.toULong() + 1uL, ULong.MAX_VALUE).withIndex()) {
                        val context = FfiLocalNetworkRecord(1u, "nearby", "Nearby", listOf("wss://relay.example"), null, emptyList(), generation)
                        runtime.phase1RefreshToday(context, UNIX_S, FfiTodayProjectionUpdate.INCREMENTAL)
                        val changed = observer.next()
                        assertEquals(context, changed.scope.context)
                        assertEquals(initial.epoch, changed.epoch)
                        assertEquals(FfiInvalidationRevision.Current((index + 1).toULong()), changed.revision)
                    }
                    val valid = FfiLocalNetworkRecord(1u, "nearby", "Nearby", listOf("wss://relay.example"), null, emptyList(), 1uL)
                    for ((invalid, code) in listOf(
                        valid.copy(id = "") to "invalid_local_network",
                        valid.copy(schemaVersion = UShort.MAX_VALUE) to "unsupported_schema_version",
                    )) {
                        val failure = assertFailsWith<TeraAppException.Failure> {
                            runtime.phase1RefreshToday(invalid, UNIX_S, FfiTodayProjectionUpdate.INCREMENTAL)
                        }
                        assertEquals(code, failure.report.code)
                    }
                    runtime.configurePublicRelays(listOf("wss://write.example"))
                    assertEquals(FfiRuntimeChangeKind.RELAY, observer.next().kind)
                    assertTrue(subscription.isActive())
                    subscription.unsubscribe()
                    subscription.unsubscribe()
                    assertFalse(subscription.isActive())
                }
            } finally {
                runtime.shutdown()
                runtime.close()
            }
            val reopened = fixture.runtime()
            try {
                assertEquals(PUBLIC_KEY, reopened.identityStatus().publicKey)
                assertTrue(reopened.identityStatus().hostSignerConfigured)
                val observer = SmokeObserver()
                reopened.subscribeChanges(observer).use {
                    assertNotEquals(firstEpoch, observer.next().epoch)
                    assertTrue(it.isActive())
                }
            } finally {
                reopened.shutdown()
                reopened.close()
            }
        }
    }

    @Test
    fun cancelledCloseWaitRetainsNativeCallbackDrainAndClosedAdmission(): Unit = runBlocking {
        SmokeFixture().use { fixture ->
            val runtime = fixture.runtime()
            val observer = SmokeObserver(paused = true)
            val subscription = runtime.subscribeChanges(observer)
            try {
                observer.next()
                val closing = async(start = CoroutineStart.UNDISPATCHED) { runtime.shutdown() }
                withTimeout(5_000) { while (!runtime.info().sdkClosed) delay(1) }
                assertFalse(closing.isCompleted)
                closing.cancelAndJoin()
                assertFalse(subscription.isActive())
                val failure = assertFailsWith<TeraAppException.Failure> { runtime.sdkStorageStatus() }
                assertEquals("client_closed", failure.report.code)
                val repeated = async(start = CoroutineStart.UNDISPATCHED) { runtime.shutdown() }
                assertFalse(repeated.isCompleted)
                observer.released.countDown()
                val receipt = withTimeout(5_000) { repeated.await() }
                assertEquals("closed", receipt.state)
                assertTrue(receipt.alreadyClosed)
                assertTrue(runtime.shutdown().alreadyClosed)
            } finally {
                observer.released.countDown()
                subscription.close()
                runtime.shutdown()
                runtime.close()
            }
        }
    }

    @Test
    fun independentSubscriptionDisposalStopsOnlyItsObserver(): Unit = runBlocking {
        SmokeFixture().use { fixture ->
            val runtime = fixture.runtime()
            val first = SmokeObserver()
            val second = SmokeObserver()
            val a = runtime.subscribeChanges(first)
            val b = runtime.subscribeChanges(second)
            try {
                assertEquals(first.next(), second.next())
                // Closing the live generated handle must dispose its Rust subscription.
                a.close()
                a.close()
                runtime.configurePublicRelays(listOf("wss://write.example"))
                assertEquals(FfiRuntimeChangeKind.RELAY, second.next().kind)
                assertEquals(0, first.pending())
                assertTrue(b.isActive())
                runtime.shutdown()
                assertEquals(FfiRuntimeChangeKind.LIFECYCLE, second.next().kind)
                assertFalse(b.isActive())
            } finally {
                a.close()
                b.close()
                runtime.shutdown()
                runtime.close()
            }
        }
    }

    @Test
    fun errorRecoveryAndProtectedDataFailureRemainTyped(): Unit = runBlocking {
        for ((code, disposition, retry) in listOf(
            Triple("draft_revision_conflict", FfiRecoveryDisposition.STALE_REVISION, FfiRetryDisposition.AFTER_RECOVERY),
            Triple("ios.runtime.cancelled", FfiRecoveryDisposition.OUTCOME_UNKNOWN, FfiRetryDisposition.RECONCILE_EXISTING_OPERATION),
            Triple("deadline_exceeded", FfiRecoveryDisposition.OUTCOME_UNKNOWN, FfiRetryDisposition.RECONCILE_EXISTING_OPERATION),
            Triple("unknown_future_retryable_network", FfiRecoveryDisposition.UNKNOWN, FfiRetryDisposition.NOT_ALLOWED),
        )) {
            val decision = classifyErrorRecovery(1u, code)
            assertEquals(disposition, decision.disposition)
            assertEquals(retry, decision.retry)
        }
        assertEquals(FfiRecoveryDisposition.UNSUPPORTED_VERSION, classifyErrorRecovery(UShort.MAX_VALUE, "client_closed").disposition)
        SmokeFixture().use { fixture ->
            val failure = assertFailsWith<TeraAppException.Failure> { fixture.runtime(ProtectedDataAvailability.UNAVAILABLE) }
            assertEquals("protected_data_unavailable", failure.report.code)
            assertEquals(1u.toUShort(), failure.report.schemaVersion)
            assertFalse(Files.exists(fixture.owner.resolve("runtime.sqlite")))
            assertFalse(failure.report.safeMessage.contains(fixture.root.toString()))
        }
    }

    @Test
    fun cancellationHandleKeepsIdentityAndDisposesIdempotently() {
        val operation = FfiMediaOperation()
        val identity = operation.operationId()
        assertEquals(32, identity.length)
        assertFalse(operation.isCancelled())
        operation.cancel()
        operation.cancel()
        assertTrue(operation.isCancelled())
        assertEquals(identity, operation.operationId())
        FfiMediaOperation().use { assertNotEquals(identity, it.operationId()) }
        operation.close()
        operation.close()
        assertFailsWith<IllegalStateException> { operation.isCancelled() }
    }
}
