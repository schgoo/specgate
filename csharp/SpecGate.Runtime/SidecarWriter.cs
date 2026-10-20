using System;
using System.IO;
using System.Threading;

namespace SpecGate.Runtime;

/// <summary>The file operations a sidecar rewrite needs, separated so tests can fail each one.</summary>
internal interface ISidecarFileSystem
{
    /// <summary>
    /// Replaces <paramref name="path"/> with <paramref name="bytes"/> so a reader
    /// sees either the old or the new document and never a partial one.
    /// </summary>
    void ReplaceAtomically(string path, byte[] bytes);

    /// <summary>Waits before retrying a failed replacement.</summary>
    void Delay(TimeSpan duration);
}

/// <summary>The real file system.</summary>
internal sealed class DiskSidecarFileSystem : ISidecarFileSystem
{
    internal static DiskSidecarFileSystem Instance { get; } = new();

    public void ReplaceAtomically(string path, byte[] bytes)
    {
        string? parent = Path.GetDirectoryName(Path.GetFullPath(path));
        if (!string.IsNullOrEmpty(parent))
        {
            Directory.CreateDirectory(parent);
        }

        string temporary = $"{path}.{Guid.NewGuid():N}.tmp";
        try
        {
            using (FileStream stream = new(temporary, FileMode.CreateNew, FileAccess.Write, FileShare.None))
            {
                stream.Write(bytes, 0, bytes.Length);
                stream.Flush(flushToDisk: true);
            }

            File.Move(temporary, path, overwrite: true);
        }
        catch
        {
            try
            {
                File.Delete(temporary);
            }
            catch (Exception cleanup) when (cleanup is IOException or UnauthorizedAccessException)
            {
                // The temporary is best effort; the original failure is what the caller must see.
            }

            throw;
        }
    }

    public void Delay(TimeSpan duration) => Thread.Sleep(duration);
}

/// <summary>
/// Rewrites one capture sidecar atomically, retrying transient replacement
/// failures.
/// </summary>
/// <remarks>
/// A scenario rewrites its sidecar after every operation, so one destination is
/// replaced many times. On Windows a replacement transiently fails with "access
/// is denied" while a virus scanner or indexer still holds the file it just saw
/// appear. The serialized bytes are already complete, so only the replacement is
/// retried; the schedule (twelve attempts, a ten millisecond linear step) is the
/// Rust runtime's, in <c>capture/io.rs</c>.
/// </remarks>
internal sealed class SidecarWriter
{
    /// <summary>Attempts before a replacement failure is reported.</summary>
    internal const int Attempts = 12;

    private const int RetryStepMilliseconds = 10;

    private readonly string _path;
    private readonly ISidecarFileSystem _fileSystem;

    internal SidecarWriter(string path, ISidecarFileSystem fileSystem)
    {
        _path = path;
        _fileSystem = fileSystem;
    }

    /// <summary>
    /// The text every persistence failure carries. The command-line tool looks
    /// for it in a failed test's output to tell a lost capture from an ordinary
    /// test failure; it is the same marker the Rust runtime emits.
    /// </summary>
    internal const string FailureMarker = "__SPECGATE_NATIVE_CAPTURE_PERSISTENCE_FAILURE__";

    /// <summary>Replaces the sidecar with <paramref name="bytes"/>.</summary>
    /// <exception cref="SpecGateCaptureException">Every attempt failed.</exception>
    internal void Write(byte[] bytes)
    {
        for (int attempt = 1; ; attempt++)
        {
            try
            {
                _fileSystem.ReplaceAtomically(_path, bytes);
                return;
            }
            catch (Exception error) when (error is IOException or UnauthorizedAccessException)
            {
                if (attempt >= Attempts)
                {
                    throw new SpecGateCaptureException(
                        CaptureRejection.PersistenceFailed,
                        $"{FailureMarker} specgate: failed to atomically persist native capture sidecar "
                        + $"'{_path}' after {Attempts} attempts: {error.Message}",
                        error);
                }

                _fileSystem.Delay(TimeSpan.FromMilliseconds(attempt * RetryStepMilliseconds));
            }
        }
    }
}
