using System;
using System.IO;
using Xunit;

namespace SpecGate.Weaver.Tests;

/// <summary>
/// Constructs the weaver must reject loudly rather than instrument incorrectly.
/// </summary>
public sealed class WeaveRejectionTests
{
    [Fact]
    public void RejectsAnnotatedAsyncOperation()
    {
        string source = Path.Combine(AppContext.BaseDirectory, "SpecGate.WeaverNegativeFixtures.dll");
        Assert.True(File.Exists(source), $"Negative fixture assembly not found at {source}.");

        string workingDirectory = Path.Combine(Path.GetTempPath(), "specgate-weave-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(workingDirectory);
        try
        {
            string target = Path.Combine(workingDirectory, Path.GetFileName(source));
            File.Copy(source, target);
            File.Copy(
                Path.Combine(AppContext.BaseDirectory, "SpecGate.Annotations.dll"),
                Path.Combine(workingDirectory, "SpecGate.Annotations.dll"));

            WeaveException failure = Assert.Throws<WeaveException>(() => AssemblyWeaver.Weave(target));

            Assert.Contains("DelayedAddAsync", failure.Message, StringComparison.Ordinal);
            Assert.Contains("async", failure.Message, StringComparison.Ordinal);
            Assert.Contains("#71", failure.Message, StringComparison.Ordinal);
        }
        finally
        {
            Directory.Delete(workingDirectory, recursive: true);
        }
    }

    [Fact]
    public void RejectsMissingAssembly()
    {
        string missing = Path.Combine(AppContext.BaseDirectory, "does-not-exist.dll");

        Assert.Throws<WeaveException>(() => AssemblyWeaver.Weave(missing));
    }
}
