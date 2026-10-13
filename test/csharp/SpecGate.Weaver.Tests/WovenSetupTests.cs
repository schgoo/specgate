using System;
using System.IO;
using System.Linq;
using SpecGate.Runtime;
using SpecGate.WeaverSetupFixture;
using Xunit;

namespace SpecGate.Weaver.Tests;

/// <summary>
/// Executes woven setups. Running them at all proves the setup IL shape
/// verifies: the epilogue carries no exception handler, so a bad stack shape or
/// a stray branch surfaces as <see cref="InvalidProgramException"/> on first jit.
/// </summary>
[Collection(WovenCaptureSerialization.Name)]
public sealed class WovenSetupTests
{
    [Fact]
    public void ASetupContributesItsConstructionInputsAndFoldsAwayTheReceiver()
    {
        using CaptureSession session = SpecGateRuntime.BeginSession("weaver.setup");

        Counter counter = Counter.Make(5);
        int total = counter.Increment(2);

        Assert.Equal(7, total);
        RecordedSpan span = Assert.Single(session.Spans);
        Assert.Equal("increment", span.Operation);

        // The receiver itself is never an input; the setup's own declared input
        // is, and inputs are emitted sorted by name.
        Assert.Equal(["by", "start"], span.Inputs.Select(input => input.Name));
        Assert.Equal([Value.Integer(2), Value.Integer(5)], span.Inputs.Select(input => input.Value));
    }

    [Fact]
    public void ASetupThatThrowsCommitsNothing()
    {
        using CaptureSession session = SpecGateRuntime.BeginSession("weaver.setup");

        Assert.Throws<InvalidOperationException>(() => Counter.MakeBroken(5));

        // Provenance is committed on the success path only, so the operation that
        // needed it now fails closed rather than recording an unfounded surface.
        Counter counter = Counter.Unrecorded(5);
        SpecGateCaptureException rejection =
            Assert.Throws<SpecGateCaptureException>(() => counter.Broken(2));

        Assert.Equal(CaptureRejection.SetupMissing, rejection.Rejection);
        Assert.Contains("MakeBroken", rejection.Message, StringComparison.Ordinal);
        Assert.Empty(session.Spans);
    }

    [Fact]
    public void AnAlreadyWovenAssemblyIsNotWovenAgain()
    {
        // The fixture assembly in the test output has already been woven by its
        // own build, and committing its provenance twice would be an ambiguity
        // the user never caused.
        string source = Path.Combine(AppContext.BaseDirectory, "SpecGate.WeaverSetupFixture.dll");
        string workingDirectory =
            Path.Combine(Path.GetTempPath(), "specgate-weave-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(workingDirectory);
        try
        {
            string target = Path.Combine(workingDirectory, Path.GetFileName(source));
            File.Copy(source, target);
            foreach (string dependency in new[] { "SpecGate.Annotations.dll", "SpecGate.Runtime.dll" })
            {
                File.Copy(
                    Path.Combine(AppContext.BaseDirectory, dependency),
                    Path.Combine(workingDirectory, dependency));
            }

            Assert.Empty(AssemblyWeaver.Weave(target));
        }
        finally
        {
            Directory.Delete(workingDirectory, recursive: true);
        }
    }
}
