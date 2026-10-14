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
        WeaveException failure = WeaveFixture("SpecGate.WeaverNegativeFixtures");

        Assert.Contains("DelayedAddAsync", failure.Message, StringComparison.Ordinal);
        Assert.Contains("async", failure.Message, StringComparison.Ordinal);
        Assert.Contains("#71", failure.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void RejectsAnnotatedGenericMethod()
    {
        WeaveException failure = WeaveFixture("SpecGate.WeaverGenericMethodFixture");

        Assert.Contains("Echo", failure.Message, StringComparison.Ordinal);
        Assert.Contains("generic method", failure.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void RejectsOperationOnGenericDeclaringType()
    {
        WeaveException failure = WeaveFixture("SpecGate.WeaverGenericTypeFixture");

        Assert.Contains("Count", failure.Message, StringComparison.Ordinal);
        Assert.Contains("generic type", failure.Message, StringComparison.Ordinal);
        Assert.Contains("GenericHolder", failure.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void RejectsMethodDeclaredAsBothOperationAndSetup()
    {
        WeaveException failure = WeaveFixture("SpecGate.WeaverSetupConflictFixture");

        Assert.Contains("Make", failure.Message, StringComparison.Ordinal);
        Assert.Contains("both a SpecGate operation and a SpecGate setup", failure.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void RejectsMissingAssembly()
    {
        string missing = Path.Combine(AppContext.BaseDirectory, "does-not-exist.dll");

        Assert.Throws<WeaveException>(() => AssemblyWeaver.Weave(missing));
    }

    /// <summary>
    /// Weaves a copy of a negative fixture assembly and returns the failure it
    /// must raise. Weaving a copy leaves the fixture assembly in the test
    /// output untouched for the other facts.
    /// </summary>
    private static WeaveException WeaveFixture(string assemblyName)
    {
        string source = Path.Combine(AppContext.BaseDirectory, assemblyName + ".dll");
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

            return Assert.Throws<WeaveException>(() => AssemblyWeaver.Weave(target));
        }
        finally
        {
            Directory.Delete(workingDirectory, recursive: true);
        }
    }
}
