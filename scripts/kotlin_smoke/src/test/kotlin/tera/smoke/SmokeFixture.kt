package tera.smoke

import java.nio.file.Files
import java.nio.file.Path
import java.security.MessageDigest
import java.util.HexFormat
import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import kotlin.test.assertNotNull
import uniffi.tera_core.FfiAddCommandType
import uniffi.tera_core.FfiAddDraftInput
import uniffi.tera_core.FfiBlossomEndpointAuthority
import uniffi.tera_core.FfiBlossomHostKind
import uniffi.tera_core.FfiMediaFile
import uniffi.tera_core.FfiPreparedMediaInput
import uniffi.tera_core.FfiRuntimeChangeDelivery
import uniffi.tera_core.FfiRuntimeChangeKind
import uniffi.tera_core.FfiRuntimeChangeRecord
import uniffi.tera_core.HostSigningRequest
import uniffi.tera_core.HostSigningResult
import uniffi.tera_core.ProtectedDataAvailability
import uniffi.tera_core.SignerAvailabilityRecord
import uniffi.tera_core.SignerStatusRecord
import uniffi.tera_core.TeraHostSigner
import uniffi.tera_core.TeraRuntime
import uniffi.tera_core.TeraRuntimeObserver

internal const val PUBLIC_KEY = "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798"
internal val GENERATION = "04".repeat(32)
internal const val UNIX_MS = 1_800_000_000_000uL
internal const val UNIX_S = 1_800_000_000uL

internal object UnavailableSigner : TeraHostSigner {
    override suspend fun signerStatus() = SignerStatusRecord(1u, SignerAvailabilityRecord.UNAVAILABLE)
    override suspend fun sign(request: HostSigningRequest): HostSigningResult =
        error("Binding smoke must never request a signature")
}

internal class SmokeFixture : AutoCloseable {
    val root: Path = Files.createTempDirectory(
        Files.createDirectories(Path.of(System.getProperty("tera.smoke.data"))), "binding-",
    )
    val owner: Path = Files.createDirectories(root.resolve("radroots/users").resolve(PUBLIC_KEY))
    val bytes = byteArrayOf(137.toByte(), 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 2, 0, 0, 0, 2)
    val original: Path = root.resolve("original.png").also { Files.write(it, bytes) }
    val sha256: String = HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(bytes))

    suspend fun runtime(protectedData: ProtectedDataAvailability = ProtectedDataAvailability.AVAILABLE): TeraRuntime =
        TeraRuntime.withHostSigner(root.toString(), PUBLIC_KEY, GENERATION, UNIX_MS, protectedData, UnavailableSigner)
            .also { runtime ->
                runtime.configureBlossom(
                    FfiBlossomHostKind.SIMULATOR, FfiBlossomEndpointAuthority.LOOPBACK_DEVELOPMENT,
                    "http://127.0.0.1:3000", emptyList(),
                )
            }

    fun media(file: FfiMediaFile) = FfiPreparedMediaInput(
        schemaVersion = 2u, opaqueReference = "media:$sha256", file = file,
        sha256 = sha256, mediaType = "image/png", byteSize = bytes.size.toULong(),
        width = 2u, height = 2u, alt = "Binding fixture", preparedAtUnixS = UNIX_S,
    )

    fun draft(media: List<FfiPreparedMediaInput>) = FfiAddDraftInput(
        schemaVersion = 1u, commandType = FfiAddCommandType.CREATE_PHOTO_UPDATE,
        content = "Binding smoke", identifier = null, title = null, summary = null,
        location = null, eventTiming = null, eventStartDate = null, eventEndDate = null,
        eventStartUnixS = null, eventEndUnixS = null, eventTimezone = null,
        priceAmount = null, currency = null, unit = null, quantity = null,
        foodPublishedAtUnixS = null, foodStatus = null, media = media,
    )

    override fun close() {
        Files.walk(root).use { paths ->
            paths.sorted(Comparator.reverseOrder()).forEach { Files.delete(it) }
        }
    }
}

internal class SmokeObserver(private val paused: Boolean = false) : TeraRuntimeObserver {
    private val events = ArrayBlockingQueue<FfiRuntimeChangeRecord>(64)
    val released = CountDownLatch(1)

    override fun onChange(change: FfiRuntimeChangeRecord) {
        check(events.offer(change)) { "The bounded smoke observer overflowed" }
        if (paused && change.kind == FfiRuntimeChangeKind.INITIAL && change.delivery == FfiRuntimeChangeDelivery.CHANGE) {
            check(released.await(10, TimeUnit.SECONDS)) { "The test did not release its callback" }
        }
    }

    fun next(): FfiRuntimeChangeRecord = assertNotNull(events.poll(5, TimeUnit.SECONDS), "Native callback deadline")
    fun pending(): Int = events.size
}
