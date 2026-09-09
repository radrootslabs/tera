package tera.smoke

import com.sun.jna.Library
import com.sun.jna.Native
import java.nio.file.Files
import java.nio.file.Path
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertNotNull
import kotlin.test.assertTrue
import kotlinx.coroutines.runBlocking
import uniffi.tera_core.FfiConverterTypeFfiMediaFile
import uniffi.tera_core.FfiMediaFile
import uniffi.tera_core.TeraAppException

internal interface SmokeLibC : Library {
    fun open(path: String, flags: Int): Int
    fun close(descriptor: Int): Int
    fun dup2(source: Int, target: Int): Int
    fun read(descriptor: Int, bytes: ByteArray, size: Long): Long
}

internal val LIBC = Native.load("c", SmokeLibC::class.java)

internal class SmokeFile(path: Path) : AutoCloseable {
    val descriptor: Int = LIBC.open(path.toString(), 0).also { check(it >= 0) }
    private var open = true

    override fun close() {
        if (open) {
            open = false
            check(LIBC.close(descriptor) == 0)
        }
    }
}

class MediaOwnershipTests {
    @Test
    fun callerCloseBeforeGeneratedConversionPreservesTheAdmittedBytes(): Unit = runBlocking {
        SmokeFixture().use { fixture ->
            val runtime = fixture.runtime()
            try {
                SmokeFile(fixture.original).use { original ->
                    FfiMediaFile(original.descriptor.toULong(), fixture.bytes.size.toULong()).use { file ->
                        original.close()
                        val saved = runtime.phase1SaveDraft(
                            "31".repeat(16), fixture.draft(listOf(fixture.media(file))), UNIX_S, null, UNIX_MS,
                        )
                        assertEquals(fixture.sha256, assertNotNull(saved.form).media.single().sha256)
                    }
                }
            } finally {
                runtime.shutdown()
                runtime.close()
            }
        }
    }

    @Test
    fun recycledCallerSlotAndDisposedSourceWrapperCannotSubstituteBytes(): Unit = runBlocking {
        SmokeFixture().use { fixture ->
            val runtime = fixture.runtime()
            val replacementPath = fixture.root.resolve("replacement.png")
            Files.write(replacementPath, ByteArray(fixture.bytes.size))
            try {
                SmokeFile(fixture.original).use { original ->
                    SmokeFile(replacementPath).use { replacement ->
                        val source = FfiMediaFile(original.descriptor.toULong(), fixture.bytes.size.toULong())
                        // Clone while the source wrapper is live. Raw generated pointer
                        // converters must never be called on an already disposed wrapper.
                        val retained = FfiConverterTypeFfiMediaFile.lift(FfiConverterTypeFfiMediaFile.lower(source))
                        source.close()
                        source.close()
                        retained.use { file ->
                            assertEquals(original.descriptor, LIBC.dup2(replacement.descriptor, original.descriptor))
                            val substituted = ByteArray(fixture.bytes.size)
                            assertEquals(substituted.size.toLong(), LIBC.read(original.descriptor, substituted, substituted.size.toLong()))
                            assertTrue(substituted.all { it == 0.toByte() })
                            val saved = runtime.phase1SaveDraft(
                                "32".repeat(16), fixture.draft(listOf(fixture.media(file))), UNIX_S, null, UNIX_MS,
                            )
                            assertEquals(fixture.sha256, assertNotNull(saved.form).media.single().sha256)
                        }
                    }
                }
            } finally {
                runtime.shutdown()
                runtime.close()
            }
        }
    }

    @Test
    fun admissionRejectsInvalidDescriptorsAndWrongTypesSynchronously() {
        assertFailsWith<TeraAppException.Failure> { FfiMediaFile(ULong.MAX_VALUE, 24uL) }
        SmokeFixture().use { fixture ->
            SmokeFile(fixture.root).use { directory ->
                assertFailsWith<TeraAppException.Failure> { FfiMediaFile(directory.descriptor.toULong(), 24uL) }
            }
            SmokeFile(fixture.original).use { original ->
                val failure = assertFailsWith<TeraAppException.Failure> {
                    FfiMediaFile(original.descriptor.toULong(), fixture.bytes.size.toULong() + 1uL)
                }
                assertEquals("media_size_mismatch", failure.report.code)
            }
        }
    }
}
