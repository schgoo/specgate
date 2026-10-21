using System;
using System.IO;
using SpecGate.Runtime;
using SpecGate.TestSupport;
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
    public void ASetupThatThrowsCommitsNothing()
    {
        using CaptureSession session = CaptureScenario.Begin("weaver.setup");

        Assert.Throws<InvalidOperationException>(() => Counter.MakeBroken(5));

        // Provenance is committed on the success path only, so the operation that
        // needed it now fails closed rather than recording an unfounded surface.
        Counter counter = Counter.Unrecorded(5);
        SpecGateCaptureException rejection =
            Assert.Throws<SpecGateCaptureException>(() => counter.Broken(2));

        Assert.Equal(CaptureRejection.SetupMissing, rejection.Rejection);
        Assert.Contains("MakeBroken", rejection.Message, StringComparison.Ordinal);

        // The boundary the rejection opened never reached a terminal channel,
        // so the capture refuses to finish rather than publishing a partial run.
        InvalidOperationException failure = Assert.Throws<InvalidOperationException>(session.Finish);
        Assert.Contains("broken", failure.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void AnOperationWhoseSetupNeverRanIsRejected()
    {
        using CaptureSession session = CaptureScenario.Begin("weaver.setup");
        Counter counter = Counter.Unrecorded(5);

        SpecGateCaptureException rejection =
            Assert.Throws<SpecGateCaptureException>(() => counter.Increment(2));

        Assert.Equal(CaptureRejection.SetupMissing, rejection.Rejection);
        Assert.Null(SpecGateRuntime.Current);
        InvalidOperationException failure = Assert.Throws<InvalidOperationException>(session.Finish);
        Assert.Contains("increment", failure.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void ASetupMayRunTwiceWithIdenticalInputs()
    {
        using CaptureSession session = CaptureScenario.Begin("weaver.setup");

        _ = Counter.Make(5);
        Counter counter = Counter.Make(5);
        Assert.Equal(7, counter.Increment(2));

        OperationSpan span = Assert.Single(session.Finish().Operations);
        Assert.Equal(["by", "start"], span.Inputs.Keys);
        Assert.Equal([Value.Integer(2), Value.Integer(5)], span.Inputs.Values);
    }

    [Fact]
    public void ASetupThatRunsTwiceWithDifferentInputsIsRejected()
    {
        using CaptureSession session = CaptureScenario.Begin("weaver.setup");
        _ = Counter.Make(5);

        SpecGateCaptureException rejection =
            Assert.Throws<SpecGateCaptureException>(() => Counter.Make(6));

        Assert.Equal(CaptureRejection.SetupAmbiguity, rejection.Rejection);
        Assert.Contains("start=5 then start=6", rejection.Message, StringComparison.Ordinal);
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
