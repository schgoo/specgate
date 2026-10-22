using System;
using System.IO;
using System.Linq;
using Mono.Cecil;
using Mono.Cecil.Cil;
using Xunit;

namespace SpecGate.Weaver.Tests;

/// <summary>
/// Constructs the weaver must reject loudly rather than instrument incorrectly.
/// </summary>
public sealed class WeaveRejectionTests
{
    [Fact]
    public void RejectsAsyncVoidOperation()
    {
        WeaveException failure = WeaveFixture("SpecGate.WeaverNegativeFixtures");

        Assert.Contains("FireAndForget", failure.Message, StringComparison.Ordinal);
        Assert.Contains("async operation returning", failure.Message, StringComparison.Ordinal);
        Assert.Contains("Task, Task<T>, ValueTask, or", failure.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void RejectsGenericMethodWithoutConcreteSpecialization()
    {
        WeaveException failure = WeaveFixture("SpecGate.WeaverGenericMethodFixture");

        Assert.Contains("Echo", failure.Message, StringComparison.Ordinal);
        Assert.Contains("concrete TypeArguments", failure.Message, StringComparison.Ordinal);
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
    public void RejectsMethodDeclaringSeveralComponents()
    {
        string directory = Path.Combine(Path.GetTempPath(), "specgate-weave-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(directory);
        try
        {
            string annotations = Path.Combine(AppContext.BaseDirectory, "SpecGate.Annotations.dll");
            File.Copy(annotations, Path.Combine(directory, "SpecGate.Annotations.dll"));
            string target = Path.Combine(directory, "MultiDeclared.dll");
            WriteMultiDeclared(annotations, target);

            WeaveException failure = Assert.Throws<WeaveException>(() => AssemblyWeaver.Weave(target));

            Assert.Contains("Holder::Shared", failure.Message, StringComparison.Ordinal);
            Assert.Contains("'fixture.one', 'fixture.two'", failure.Message, StringComparison.Ordinal);
            Assert.Contains("exactly one component", failure.Message, StringComparison.Ordinal);
        }
        finally
        {
            Directory.Delete(directory, recursive: true);
        }
    }

    private static void WriteMultiDeclared(string annotationsPath, string targetPath)
    {
        using AssemblyDefinition annotations = AssemblyDefinition.ReadAssembly(annotationsPath);
        TypeDefinition attribute = annotations.MainModule.GetType("SpecGate.Annotations.SpecOperationAttribute");

        using AssemblyDefinition assembly = AssemblyDefinition.CreateAssembly(
            new AssemblyNameDefinition("MultiDeclared", new Version(1, 0)), "MultiDeclared", ModuleKind.Dll);
        ModuleDefinition module = assembly.MainModule;
        MethodReference constructor = module.ImportReference(
            attribute.Methods.Single(method => method.IsConstructor && !method.IsStatic));

        TypeDefinition holder = new(
            "Fixtures",
            "Holder",
            TypeAttributes.Public | TypeAttributes.Abstract | TypeAttributes.Sealed,
            module.TypeSystem.Object);
        module.Types.Add(holder);
        MethodDefinition shared = new(
            "Shared",
            MethodAttributes.Public | MethodAttributes.Static,
            module.TypeSystem.Void);
        shared.Body.GetILProcessor().Emit(OpCodes.Ret);
        holder.Methods.Add(shared);

        foreach (string component in new[] { "fixture.one", "fixture.two" })
        {
            CustomAttribute annotation = new(constructor);
            annotation.ConstructorArguments.Add(new CustomAttributeArgument(module.TypeSystem.String, "shared"));
            annotation.Properties.Add(new CustomAttributeNamedArgument(
                "Spec", new CustomAttributeArgument(module.TypeSystem.String, component)));
            shared.CustomAttributes.Add(annotation);
        }

        assembly.Write(targetPath);
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
