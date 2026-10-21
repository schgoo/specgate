using Xunit;

namespace SpecGate.Runtime.Tests;

/// <summary>
/// Serializes every capture test. A capture session is process-wide, so tests
/// that attach one cannot run concurrently with each other.
/// </summary>
[CollectionDefinition(nameof(CaptureSessionSerialization), DisableParallelization = true)]
public sealed class CaptureSessionSerialization;
