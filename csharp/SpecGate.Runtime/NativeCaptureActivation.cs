using System;
using System.Collections.Generic;
using System.Diagnostics.CodeAnalysis;
using System.Runtime.CompilerServices;
using System.Text.Json;

namespace SpecGate.Runtime;

/// <summary>
/// Starts a capture session when the command-line tool asks for one through the
/// process environment.
/// </summary>
/// <remarks>
/// <para>
/// The tool sets <c>SPECGATE_NATIVE_CAPTURE</c> to the same JSON document the
/// Rust runtime reads (<c>EnvConfig</c> in
/// <c>rust/crates/specgate-runtime/src/capture/model.rs</c>): the session
/// configuration under <c>capture</c> and the sidecar destination under
/// <c>sidecar_path</c>. A woven assembly needs no initializer of its own,
/// because this module initializer runs when the runtime assembly is first
/// loaded, before any woven operation can call it.
/// </para>
/// <para>
/// An absent or empty variable is the ordinary case and does nothing. A present
/// variable that cannot be used fails loudly: a capture that silently did not
/// start would be indistinguishable from a test that never ran an operation.
/// </para>
/// </remarks>
internal static class NativeCaptureActivation
{
    /// <summary>The environment variable carrying the activation document.</summary>
    internal const string EnvironmentVariable = "SPECGATE_NATIVE_CAPTURE";

    [ModuleInitializer]
    [SuppressMessage(
        "Usage",
        "CA2255:The 'ModuleInitializer' attribute should not be used in libraries",
        Justification = "Environment-driven capture must start before any woven operation runs, and the "
            + "runtime assembly is the one place every woven assembly already depends on. Weaving "
            + "initializers into user assemblies was rejected in docs/decisions/csharp-capture-cli.md.")]
    internal static void Initialize()
    {
        CaptureSession? session = Start(
            Environment.GetEnvironmentVariable(EnvironmentVariable), DiskSidecarFileSystem.Instance);
        if (session is not null)
        {
            AppDomain.CurrentDomain.ProcessExit += (_, _) => session.PersistAtExit();
        }
    }

    /// <summary>Starts the session an activation document describes.</summary>
    /// <param name="document">The variable's value, or <see langword="null"/> when it is unset.</param>
    /// <param name="fileSystem">Where the sidecar is written.</param>
    /// <returns>
    /// The started session, or <see langword="null"/> when there is nothing to
    /// start: the variable is absent or empty, or a session is already active.
    /// </returns>
    /// <exception cref="InvalidOperationException">The document is present but invalid.</exception>
    internal static CaptureSession? Start(string? document, ISidecarFileSystem fileSystem)
    {
        if (string.IsNullOrEmpty(document))
        {
            return null;
        }

        (CaptureConfig config, string sidecarPath) = Parse(document);
        return SpecGateRuntime.BeginEnvironmentSession(config, new SidecarWriter(sidecarPath, fileSystem));
    }

    private static (CaptureConfig Config, string SidecarPath) Parse(string document)
    {
        try
        {
            using JsonDocument parsed = JsonDocument.Parse(document);
            JsonElement root = RequireObject(parsed.RootElement, "the document");
            JsonElement capture = RequireObject(Require(root, "capture"), "capture");

            List<SpanId> operations = [];
            JsonElement operationIds = Require(capture, "operation_span_ids");
            if (operationIds.ValueKind != JsonValueKind.Array)
            {
                throw new FormatException("operation_span_ids must be an array");
            }

            foreach (JsonElement id in operationIds.EnumerateArray())
            {
                operations.Add(SpanId.Parse(ReadString(id, "operation_span_ids")));
            }

            CaptureConfig config = CaptureConfig
                .Builder(
                    ReadString(Require(capture, "scenario_name"), "scenario_name"),
                    TraceId.Parse(ReadString(Require(capture, "trace_id"), "trace_id")),
                    SpanId.Parse(ReadString(Require(capture, "run_span_id"), "run_span_id")),
                    SpanId.Parse(ReadString(Require(capture, "scenario_span_id"), "scenario_span_id")))
                .OperationSpanIds(operations)
                .StartTime(ReadLong(Require(capture, "start_time_unix_nano"), "start_time_unix_nano"))
                .ClockStep(ReadLong(Require(capture, "clock_step_unix_nano"), "clock_step_unix_nano"))
                .Build();

            string sidecarPath = ReadString(Require(root, "sidecar_path"), "sidecar_path");
            if (sidecarPath.Length == 0)
            {
                throw new FormatException("sidecar_path must not be empty");
            }

            return (config, sidecarPath);
        }
        catch (Exception error) when (error is JsonException or FormatException or ArgumentException)
        {
            throw new InvalidOperationException(
                $"specgate: invalid {EnvironmentVariable} configuration: {error.Message}", error);
        }
    }

    private static JsonElement Require(JsonElement owner, string name) =>
        owner.TryGetProperty(name, out JsonElement value)
            ? value
            : throw new FormatException($"missing field `{name}`");

    private static JsonElement RequireObject(JsonElement element, string name) =>
        element.ValueKind == JsonValueKind.Object
            ? element
            : throw new FormatException($"{name} must be an object");

    private static string ReadString(JsonElement element, string name) =>
        element.ValueKind == JsonValueKind.String
            ? element.GetString()!
            : throw new FormatException($"{name} must be a string");

    private static long ReadLong(JsonElement element, string name) =>
        element.ValueKind == JsonValueKind.Number && element.TryGetInt64(out long value)
            ? value
            : throw new FormatException($"{name} must be an integer");
}
