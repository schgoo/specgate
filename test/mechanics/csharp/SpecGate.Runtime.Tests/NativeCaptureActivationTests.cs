using System.Text;
using System.Text.Json;
using SpecGate.TestSupport;
using Xunit;

namespace SpecGate.Runtime.Tests;

/// <summary>
/// Environment-driven capture: the session the command-line tool starts through
/// <c>SPECGATE_NATIVE_CAPTURE</c> and the sidecar it keeps current.
/// </summary>
[Collection(nameof(CaptureSessionSerialization))]
public sealed class NativeCaptureActivationTests
{
    private const string Sidecar = "sidecar.json";

    private const string Document = $$"""
        {"capture":{"scenario_name":"env_scenario","trace_id":"{{CaptureScenario.TraceIdText}}",
        "run_span_id":"{{CaptureScenario.RunSpanIdText}}","scenario_span_id":"{{CaptureScenario.ScenarioSpanIdText}}",
        "operation_span_ids":[],"start_time_unix_nano":0,"clock_step_unix_nano":1},"sidecar_path":"sidecar.json"}
        """;

    [Theory]
    [InlineData(null)]
    [InlineData("")]
    public void AnAbsentOrEmptyVariableStartsNothing(string? value)
    {
        FakeSidecarFileSystem files = new();

        Assert.Null(NativeCaptureActivation.Start(value, files));

        Assert.False(SpecGateRuntime.IsRecording);
        Assert.Empty(files.Writes);
    }

    [Theory]
    [InlineData("not json")]
    [InlineData("[]")]
    [InlineData("""{"sidecar_path":"sidecar.json"}""")]
    [InlineData("""{"capture":{},"sidecar_path":"sidecar.json"}""")]
    [InlineData("""{"capture":"x","sidecar_path":"sidecar.json"}""")]
    public void APresentButMalformedVariableFailsLoudly(string value)
    {
        InvalidOperationException error = Assert.Throws<InvalidOperationException>(
            () => NativeCaptureActivation.Start(value, new FakeSidecarFileSystem()));

        Assert.Contains("invalid SPECGATE_NATIVE_CAPTURE configuration", error.Message, StringComparison.Ordinal);
        Assert.False(SpecGateRuntime.IsRecording);
    }

    [Theory]
    [InlineData("\"trace_id\":\"" + CaptureScenario.TraceIdText + "\"", "\"trace_id\":\"nothex\"")]
    [InlineData("\"run_span_id\":\"" + CaptureScenario.RunSpanIdText + "\"", "\"run_span_id\":7")]
    [InlineData("\"start_time_unix_nano\":0", "\"start_time_unix_nano\":\"0\"")]
    [InlineData("\"clock_step_unix_nano\":1", "\"clock_step_unix_nano\":0")]
    [InlineData("\"sidecar_path\":\"sidecar.json\"", "\"sidecar_path\":\"\"")]
    [InlineData("\"operation_span_ids\":[]", "\"operation_span_ids\":[\"short\"]")]
    public void AnInvalidFieldFailsLoudly(string valid, string invalid)
    {
        string value = Document.Replace(valid, invalid, StringComparison.Ordinal);
        Assert.NotEqual(Document, value);

        InvalidOperationException error = Assert.Throws<InvalidOperationException>(
            () => NativeCaptureActivation.Start(value, new FakeSidecarFileSystem()));

        Assert.Contains("invalid SPECGATE_NATIVE_CAPTURE configuration", error.Message, StringComparison.Ordinal);
        Assert.False(SpecGateRuntime.IsRecording);
    }

    [Fact]
    public void ADocumentStartsASessionThatRecordsEveryComponent()
    {
        FakeSidecarFileSystem files = new();
        using CaptureSession session = Started(files);

        Run("fixture.one", [2, 3], 5);
        Run("fixture.two", [4, 5], 9);

        Capture capture = session.Finish();
        Assert.Equal("env_scenario", capture.ScenarioName);
        Assert.Equal(["fixture.one", "fixture.two"], capture.Operations.Select(span => span.ComponentId));
    }

    [Fact]
    public void AStartedSessionIgnoresASecondActivation()
    {
        FakeSidecarFileSystem files = new();
        using CaptureSession session = Started(files);

        Assert.Null(NativeCaptureActivation.Start(Document, files));

        Run("fixture.one", [2, 3], 5);
        Assert.Single(session.Finish().Operations);
    }

    [Fact]
    public void TheSidecarIsRewrittenAfterInputsAndAfterEveryOperation()
    {
        FakeSidecarFileSystem files = new();
        using CaptureSession session = Started(files);

        OperationScope scope = SpecGateRuntime.EnterOperation(Boundaries.Of("Add"), "add", "fixture.one");
        Assert.Empty(files.Writes);

        SpecGateRuntime.RecordInputs(scope, [2, 3]);
        Assert.Single(files.Writes);
        Assert.Equal("incomplete_capture", FaultTypeOf(files.Last(), 0));

        SpecGateRuntime.ExitOk(scope, 5);
        Assert.Equal(2, files.Writes.Count);
        Assert.Null(FaultTypeOf(files.Last(), 0));

        Run("fixture.one", [4, 5], 9);
        Assert.Equal(4, files.Writes.Count);
        Assert.Equal(2, Operations(files.Last()).GetArrayLength());
        Assert.Equal(Sidecar, files.Paths.Distinct().Single());
        session.Finish();
    }

    [Fact]
    public void ProjectingAnOutstandingOperationDoesNotShiftTheClock()
    {
        FakeSidecarFileSystem files = new();
        string withSidecar;
        using (CaptureSession session = Started(files))
        {
            Run("fixture.one", [1, 1], 2);
            withSidecar = CaptureJson.SerializeToString(session.Finish());
        }

        string withoutSidecar;
        using (CaptureSession session = SpecGateRuntime.BeginSession("fixture.one", CaptureScenario.Config("env_scenario")))
        {
            Run("fixture.one", [1, 1], 2);
            withoutSidecar = CaptureJson.SerializeToString(session.Finish());
        }

        Assert.Equal(withoutSidecar, withSidecar);
    }

    [Fact]
    public void AnExitFlushRecordsAnOutstandingOperationAsIncomplete()
    {
        FakeSidecarFileSystem files = new();
        using CaptureSession session = Started(files);
        OperationScope scope = SpecGateRuntime.EnterOperation(Boundaries.Of("Add"), "add", "fixture.one");
        SpecGateRuntime.RecordInputs(scope, [2, 3]);
        int before = files.Writes.Count;

        session.PersistAtExit(TextWriter.Null);

        Assert.Equal(before + 1, files.Writes.Count);
        Assert.Equal("incomplete_capture", FaultTypeOf(files.Last(), 0));
    }

    [Fact]
    public void AnExitFlushWritesNothingWhenEveryOperationClosed()
    {
        FakeSidecarFileSystem files = new();
        using CaptureSession session = Started(files);
        Run("fixture.one", [2, 3], 5);
        int before = files.Writes.Count;

        session.PersistAtExit(TextWriter.Null);

        Assert.Equal(before, files.Writes.Count);
    }

    [Fact]
    public void AnExitFlushNeverThrowsWhenPersistenceFails()
    {
        FakeSidecarFileSystem files = new();
        using CaptureSession session = Started(files);
        OperationScope scope = SpecGateRuntime.EnterOperation(Boundaries.Of("Add"), "add", "fixture.one");
        SpecGateRuntime.RecordInputs(scope, [2, 3]);
        int before = files.Attempts;
        files.FailuresRemaining = int.MaxValue;

        session.PersistAtExit(TextWriter.Null);

        Assert.Equal(SidecarWriter.Attempts, files.Attempts - before);
    }

    [Fact]
    public void ATransientReplacementFailureIsRetriedWithLinearBackoff()
    {
        FakeSidecarFileSystem files = new() { FailuresRemaining = 2 };
        using CaptureSession session = Started(files);

        Run("fixture.one", [2, 3], 5);

        Assert.Equal([TimeSpan.FromMilliseconds(10), TimeSpan.FromMilliseconds(20)], files.Delays);
        Assert.Equal(2, files.Writes.Count);
    }

    [Fact]
    public void APersistenceFailureFailsTheInstrumentedOperation()
    {
        FakeSidecarFileSystem files = new();
        using CaptureSession session = Started(files);
        OperationScope scope = SpecGateRuntime.EnterOperation(Boundaries.Of("Add"), "add", "fixture.one");
        files.FailuresRemaining = int.MaxValue;

        SpecGateCaptureException error = Assert.Throws<SpecGateCaptureException>(
            () => SpecGateRuntime.RecordInputs(scope, [2, 3]));

        Assert.Equal(CaptureRejection.PersistenceFailed, error.Rejection);
        Assert.Contains("__SPECGATE_NATIVE_CAPTURE_PERSISTENCE_FAILURE__", error.Message, StringComparison.Ordinal);
        Assert.Equal(SidecarWriter.Attempts, files.Attempts);
        Assert.IsType<IOException>(error.InnerException);
    }

    [Fact]
    public void ACompletionPersistenceFailureSurfacesThroughTheExit()
    {
        FakeSidecarFileSystem files = new();
        using CaptureSession session = Started(files);
        OperationScope scope = SpecGateRuntime.EnterOperation(Boundaries.Of("Add"), "add", "fixture.one");
        SpecGateRuntime.RecordInputs(scope, [2, 3]);
        files.FailuresRemaining = int.MaxValue;

        SpecGateCaptureException error = Assert.Throws<SpecGateCaptureException>(
            () => SpecGateRuntime.ExitOk(scope, 5));

        Assert.Equal(CaptureRejection.PersistenceFailed, error.Rejection);
        Assert.Null(SpecGateRuntime.Current);
    }

    [Fact]
    public void APersistenceFailureThePrecedingCodeCaughtPoisonsTheSession()
    {
        FakeSidecarFileSystem files = new();
        using CaptureSession session = Started(files);
        OperationScope first = SpecGateRuntime.EnterOperation(Boundaries.Of("Add"), "add", "fixture.one");
        SpecGateRuntime.RecordInputs(first, [2, 3]);
        files.FailuresRemaining = SidecarWriter.Attempts;
        Assert.Throws<SpecGateCaptureException>(() => SpecGateRuntime.ExitOk(first, 5));
        int writesBefore = files.Writes.Count;

        // Application code swallowed the failure and carried on; the disk is healthy again.
        SpecGateCaptureException later = Assert.Throws<SpecGateCaptureException>(
            () => Run("fixture.one", [4, 5], 9));

        Assert.Equal(CaptureRejection.PersistenceFailed, later.Rejection);
        Assert.Contains("__SPECGATE_NATIVE_CAPTURE_PERSISTENCE_FAILURE__", later.Message, StringComparison.Ordinal);
        Assert.Equal(writesBefore, files.Writes.Count);
    }

    [Fact]
    public void APoisonedSessionIsReportedAtExitWithNoOperationLeftLookingSuccessful()
    {
        FakeSidecarFileSystem files = new();
        using CaptureSession session = Started(files);
        OperationScope first = SpecGateRuntime.EnterOperation(Boundaries.Of("Add"), "add", "fixture.one");
        SpecGateRuntime.RecordInputs(first, [2, 3]);
        files.FailuresRemaining = SidecarWriter.Attempts;
        Assert.Throws<SpecGateCaptureException>(() => SpecGateRuntime.ExitOk(first, 5));
        Assert.Throws<SpecGateCaptureException>(() => Run("fixture.one", [4, 5], 9));
        using StringWriter error = new();

        session.PersistAtExit(error);

        Assert.Contains("__SPECGATE_NATIVE_CAPTURE_PERSISTENCE_FAILURE__", error.ToString(), StringComparison.Ordinal);
        byte[] last = files.Last();
        Assert.Equal(2, Operations(last).GetArrayLength());
        Assert.Equal("incomplete_capture", FaultTypeOf(last, 0));
        Assert.Equal("incomplete_capture", FaultTypeOf(last, 1));
    }

    [Fact]
    public void APoisonedExitStillNeverThrowsWhenTheDiskStaysBroken()
    {
        FakeSidecarFileSystem files = new();
        using CaptureSession session = Started(files);
        OperationScope scope = SpecGateRuntime.EnterOperation(Boundaries.Of("Add"), "add", "fixture.one");
        files.FailuresRemaining = int.MaxValue;
        Assert.Throws<SpecGateCaptureException>(() => SpecGateRuntime.RecordInputs(scope, [2, 3]));
        using StringWriter error = new();

        session.PersistAtExit(error);

        Assert.Contains("__SPECGATE_NATIVE_CAPTURE_PERSISTENCE_FAILURE__", error.ToString(), StringComparison.Ordinal);
    }

    [Fact]
    public void TheDiskFileSystemReplacesTheSidecarAndLeavesNoTemporaryFile()
    {
        string directory = Path.Combine(Path.GetTempPath(), "specgate-sidecar-" + Guid.NewGuid().ToString("N"));
        try
        {
            string path = Path.Combine(directory, "nested", "0001.json");
            SidecarWriter writer = new(path, DiskSidecarFileSystem.Instance);

            writer.Write(Encoding.UTF8.GetBytes("first"));
            writer.Write(Encoding.UTF8.GetBytes("second"));

            Assert.Equal("second", File.ReadAllText(path));
            Assert.Equal([path], Directory.GetFiles(Path.GetDirectoryName(path)!));
        }
        finally
        {
            Directory.Delete(directory, recursive: true);
        }
    }

    private static CaptureSession Started(FakeSidecarFileSystem files) =>
        NativeCaptureActivation.Start(Document, files)
        ?? throw new InvalidOperationException("the activation document did not start a session");

    private static void Run(string component, object?[] arguments, object result)
    {
        OperationScope scope = SpecGateRuntime.EnterOperation(Boundaries.Of("Add"), "add", component);
        SpecGateRuntime.RecordInputs(scope, arguments);
        SpecGateRuntime.ExitOk(scope, result);
    }

    private static JsonElement Operations(byte[] sidecar) =>
        JsonDocument.Parse(sidecar).RootElement.GetProperty("operations");

    private static string? FaultTypeOf(byte[] sidecar, int index)
    {
        JsonElement completion = Operations(sidecar)[index].GetProperty("completion");
        return completion.ValueKind == JsonValueKind.Object
            && completion.TryGetProperty("fault_type", out JsonElement faultType)
            ? faultType.GetString()
            : null;
    }

    private sealed class FakeSidecarFileSystem : ISidecarFileSystem
    {
        internal List<byte[]> Writes { get; } = [];

        internal List<string> Paths { get; } = [];

        internal List<TimeSpan> Delays { get; } = [];

        internal int FailuresRemaining { get; set; }

        internal int Attempts { get; private set; }

        internal byte[] Last() => Writes[^1];

        public void ReplaceAtomically(string path, byte[] bytes)
        {
            Attempts++;
            if (FailuresRemaining > 0)
            {
                FailuresRemaining--;
                throw new IOException("injected replacement failure");
            }

            Paths.Add(path);
            Writes.Add(bytes);
        }

        public void Delay(TimeSpan duration) => Delays.Add(duration);
    }
}
