using System;
using System.Diagnostics;
using System.IO;
using System.Text;
using System.Text.Json;
using SpecGate.TestSupport;
using Xunit;

namespace SpecGate.Weaver.Tests;

/// <summary>
/// The MSBuild targets the command-line tool injects into a build of someone
/// else's tests project, run through a real <c>dotnet build</c>.
/// </summary>
/// <remarks>
/// The test output supplies the weaver, Cecil, and targets. Runtime capture
/// tests build the shipped net8.0 runtime into an isolated tools directory.
/// </remarks>
public sealed class InjectedCaptureTargetsTests : IDisposable
{
    private readonly string _directory =
        Path.Combine(Path.GetTempPath(), "specgate-targets-" + Guid.NewGuid().ToString("N"));

    [Fact]
    public void WeavesOnlyTheNamedProjectAndReportsTheCount()
    {
        string sources = WriteProject("Sources", withOperation: true, reference: null);
        string bystander = WriteProject("Bystander", withOperation: true, reference: sources);
        string report = Path.Combine(_directory, "report.json");

        Build(bystander, sources, report);

        Assert.Equal("{\"woven_method_count\":1}\n", File.ReadAllText(report));
        string artifacts = Path.Combine(_directory, "artifacts", "bin");
        string wovenAssembly = Path.Combine(artifacts, "Sources", "debug", "Sources.dll");
        Assert.True(Contains(wovenAssembly, "SpecGate.Runtime"), "the named project was not woven");
        Assert.False(
            Contains(Path.Combine(artifacts, "Bystander", "debug", "Bystander.dll"), "SpecGate.Runtime"),
            "a project the command did not name was woven");
        Assert.True(
            File.Exists(Path.Combine(artifacts, "Sources", "debug", "SpecGate.Runtime.dll")),
            "the runtime is not beside the woven assembly");
    }

    [Fact]
    public void ReportsZeroWhenTheNamedProjectDeclaresNothing()
    {
        string sources = WriteProject("Sources", withOperation: false, reference: null);
        string report = Path.Combine(_directory, "report.json");

        Build(sources, sources, report);

        Assert.Equal("{\"woven_method_count\":0}\n", File.ReadAllText(report));
    }

    [Fact]
    public void WeavesNothingWhenNoProjectIsNamed()
    {
        string sources = WriteProject("Sources", withOperation: true, reference: null);

        Build(sources, weaveProject: string.Empty, report: Path.Combine(_directory, "report.json"));

        Assert.False(File.Exists(Path.Combine(_directory, "report.json")));
        Assert.False(
            Contains(Path.Combine(_directory, "artifacts", "bin", "Sources", "debug", "Sources.dll"), "SpecGate.Runtime"));
    }

    [Theory]
    [InlineData("net8.0", 8)]
    [InlineData("net10.0", 10)]
    public void CapturesAnOperationOnTheDeclaredRuntime(string framework, int runtimeMajor)
    {
        string sources = WriteProject("Sources", withOperation: true, reference: null, framework, executable: true);
        Build(sources, sources, Path.Combine(_directory, "report.json"), CreateShippedTools());
        string sidecar = Path.Combine(_directory, "capture.json");
        ProcessStartInfo start = new("dotnet")
        {
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            WorkingDirectory = _directory,
        };
        start.ArgumentList.Add(Path.Combine(_directory, "artifacts", "bin", "Sources", "debug", "Sources.dll"));
        start.Environment["SPECGATE_NATIVE_CAPTURE"] = JsonSerializer.Serialize(new
        {
            capture = new
            {
                scenario_name = "supported_runtime",
                trace_id = CaptureScenario.TraceIdText,
                run_span_id = CaptureScenario.RunSpanIdText,
                scenario_span_id = CaptureScenario.ScenarioSpanIdText,
                operation_span_ids = Array.Empty<string>(),
                start_time_unix_nano = 0,
                clock_step_unix_nano = 1,
            },
            sidecar_path = sidecar,
        });
        string output = Run(start);
        Assert.Equal($"{runtimeMajor}\n5\n", output.Replace("\r\n", "\n", StringComparison.Ordinal));
        using JsonDocument document = JsonDocument.Parse(File.ReadAllText(sidecar));
        JsonElement operation = Assert.Single(document.RootElement.GetProperty("operations").EnumerateArray());
        Assert.Equal("targets.basic", operation.GetProperty("component_id").GetString());
        Assert.Equal("add", operation.GetProperty("operation_name").GetString());
        Assert.Equal("ok", operation.GetProperty("status").GetString());
        Assert.Equal(2, operation.GetProperty("inputs").GetProperty("a").GetProperty("value").GetInt32());
        Assert.Equal(3, operation.GetProperty("inputs").GetProperty("b").GetProperty("value").GetInt32());
        Assert.Equal("result", operation.GetProperty("completion").GetProperty("kind").GetString());
        Assert.Equal(5, operation.GetProperty("completion").GetProperty("value").GetProperty("value").GetInt32());
    }

    public void Dispose()
    {
        if (Directory.Exists(_directory))
        {
            Directory.Delete(_directory, recursive: true);
        }
    }

    private static bool Contains(string assembly, string text)
    {
        Assert.True(File.Exists(assembly), $"{assembly} was not built");
        return Encoding.ASCII.GetString(File.ReadAllBytes(assembly)).Contains(text, StringComparison.Ordinal);
    }

    private static string RepositoryRoot()
    {
        DirectoryInfo? directory = new(AppContext.BaseDirectory);
        while (directory is not null && !File.Exists(Path.Combine(directory.FullName, "SpecGate.slnx")))
        {
            directory = directory.Parent;
        }

        return directory?.FullName ?? throw new InvalidOperationException("SpecGate.slnx was not found above the test output.");
    }

    private string WriteProject(
        string name, bool withOperation, string? reference, string framework = "net10.0", bool executable = false)
    {
        string root = Path.Combine(_directory, name);
        Directory.CreateDirectory(root);
        string annotations = Path.Combine(RepositoryRoot(), "csharp", "SpecGate.Annotations", "SpecGate.Annotations.csproj");
        string references = $"<ProjectReference Include=\"{annotations}\" />"
            + (reference is null ? string.Empty : $"<ProjectReference Include=\"{reference}\" />");
        File.WriteAllText(
            Path.Combine(root, name + ".csproj"),
            $"<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><TargetFramework>{framework}</TargetFramework>"
            + (executable ? "<OutputType>Exe</OutputType>" : string.Empty)
            + $"<Nullable>enable</Nullable></PropertyGroup><ItemGroup>{references}</ItemGroup></Project>");
        File.WriteAllText(
            Path.Combine(root, "Code.cs"),
            withOperation
                ? $"using SpecGate.Annotations;\npublic static class {name}Code {{ [SpecOperation(\"add\", Spec = \"targets.basic\")] "
                    + "public static int Add([SpecInput(\"a\")] int a, [SpecInput(\"b\")] int b) => a + b; }\n"
                : $"public static class {name}Code {{ }}\n");
        if (executable)
        {
            File.WriteAllText(Path.Combine(root, "Program.cs"),
                $"System.Console.WriteLine(System.Environment.Version.Major);\nSystem.Console.WriteLine({name}Code.Add(2, 3));\n");
        }

        return Path.Combine(root, name + ".csproj");
    }

    private string CreateShippedTools()
    {
        string tools = Path.Combine(_directory, "weaver");
        ProcessStartInfo start = new("dotnet")
        {
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            WorkingDirectory = RepositoryRoot(),
        };
        foreach (string argument in new[]
        {
            "build", Path.Combine(RepositoryRoot(), "csharp", "SpecGate.Runtime", "SpecGate.Runtime.csproj"),
            "--framework", "net8.0", "--output", tools, "--artifacts-path", Path.Combine(_directory, "tools-artifacts"),
            "-nodeReuse:false", "-p:UseSharedCompilation=false",
        })
        {
            start.ArgumentList.Add(argument);
        }

        Run(start);
        foreach (string name in new[]
        {
            "SpecGate.Weaver.dll", "Mono.Cecil.dll", "Mono.Cecil.Rocks.dll",
            "SpecGateCapture.targets", "SpecGateWeave.targets",
        })
        {
            File.Copy(Path.Combine(AppContext.BaseDirectory, name), Path.Combine(tools, name));
        }

        return tools;
    }

    private void Build(string project, string weaveProject, string report, string? tools = null)
    {
        ProcessStartInfo start = new("dotnet")
        {
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            WorkingDirectory = RepositoryRoot(),
        };
        start.ArgumentList.Add("build");
        start.ArgumentList.Add(project);
        start.ArgumentList.Add("-c");
        start.ArgumentList.Add("Debug");
        start.ArgumentList.Add("--artifacts-path");
        start.ArgumentList.Add(Path.Combine(_directory, "artifacts"));
        start.ArgumentList.Add("-nodeReuse:false");
        start.ArgumentList.Add("-p:UseSharedCompilation=false");
        start.ArgumentList.Add("-p:CustomAfterMicrosoftCommonTargets=" + Path.Combine(tools ?? AppContext.BaseDirectory, "SpecGateCapture.targets"));
        start.ArgumentList.Add("-p:SpecGateWeaveProject=" + weaveProject);
        start.ArgumentList.Add("-p:SpecGateWeaveReportPath=" + report);

        Run(start);
    }

    private static string Run(ProcessStartInfo start)
    {
        using Process process = Process.Start(start) ?? throw new InvalidOperationException("dotnet did not start");
        string output = process.StandardOutput.ReadToEnd() + process.StandardError.ReadToEnd();
        process.WaitForExit();
        Assert.True(process.ExitCode == 0, output);
        return output;
    }
}
