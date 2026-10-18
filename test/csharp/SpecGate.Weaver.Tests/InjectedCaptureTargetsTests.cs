using System;
using System.Diagnostics;
using System.IO;
using System.Text;
using Xunit;

namespace SpecGate.Weaver.Tests;

/// <summary>
/// The MSBuild targets the command-line tool injects into a build of someone
/// else's tests project, run through a real <c>dotnet build</c>.
/// </summary>
/// <remarks>
/// The test output directory is laid out like the shipped weaver directory:
/// the weaver, Cecil, the runtime, and both targets files side by side.
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

    private string WriteProject(string name, bool withOperation, string? reference)
    {
        string root = Path.Combine(_directory, name);
        Directory.CreateDirectory(root);
        string annotations = Path.Combine(RepositoryRoot(), "csharp", "SpecGate.Annotations", "SpecGate.Annotations.csproj");
        string references = $"<ProjectReference Include=\"{annotations}\" />"
            + (reference is null ? string.Empty : $"<ProjectReference Include=\"{reference}\" />");
        File.WriteAllText(
            Path.Combine(root, name + ".csproj"),
            $"<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><TargetFramework>net10.0</TargetFramework>"
            + $"<Nullable>enable</Nullable></PropertyGroup><ItemGroup>{references}</ItemGroup></Project>");
        File.WriteAllText(
            Path.Combine(root, "Code.cs"),
            withOperation
                ? $"using SpecGate.Annotations;\npublic static class {name}Code {{ [SpecOperation(\"add\", Spec = \"targets.basic\")] "
                    + "public static int Add([SpecInput(\"a\")] int a, [SpecInput(\"b\")] int b) => a + b; }\n"
                : $"public static class {name}Code {{ }}\n");
        return Path.Combine(root, name + ".csproj");
    }

    private void Build(string project, string weaveProject, string report)
    {
        ProcessStartInfo start = new("dotnet")
        {
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            WorkingDirectory = _directory,
        };
        start.ArgumentList.Add("build");
        start.ArgumentList.Add(project);
        start.ArgumentList.Add("-c");
        start.ArgumentList.Add("Debug");
        start.ArgumentList.Add("--artifacts-path");
        start.ArgumentList.Add(Path.Combine(_directory, "artifacts"));
        start.ArgumentList.Add("-nodeReuse:false");
        start.ArgumentList.Add("-p:UseSharedCompilation=false");
        start.ArgumentList.Add("-p:CustomAfterMicrosoftCommonTargets=" + Path.Combine(AppContext.BaseDirectory, "SpecGateCapture.targets"));
        start.ArgumentList.Add("-p:SpecGateWeaveProject=" + weaveProject);
        start.ArgumentList.Add("-p:SpecGateWeaveReportPath=" + report);

        using Process process = Process.Start(start) ?? throw new InvalidOperationException("dotnet did not start");
        string output = process.StandardOutput.ReadToEnd() + process.StandardError.ReadToEnd();
        process.WaitForExit();
        Assert.True(process.ExitCode == 0, output);
    }
}
