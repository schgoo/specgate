using Xunit;

namespace SpecGate.CliCaptureFixtures.FailingTests;

// Deliberately fails after the operation completed. Kept apart from the passing tests so
// the packaged-install smoke captures a clean project; the CLI integration test uses this
// one to prove a failed test is reported and strict capture refuses the batch.
public sealed class FailingCaptureTests
{
    [Fact]
    public void DeliberatelyFailsAfterRecording()
    {
        Assert.Equal(0, Arithmetic.Add(1, 1));
    }
}
