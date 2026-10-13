using Xunit;

namespace SpecGate.Weaver.Tests;

/// <summary>
/// The capture session is process-wide, so woven fixture tests must not overlap.
/// </summary>
[CollectionDefinition(Name, DisableParallelization = true)]
public sealed class WovenCaptureSerialization
{
    /// <summary>The collection name shared by every woven fixture test class.</summary>
    public const string Name = "woven-capture";
}
