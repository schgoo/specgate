using System;
using System.IO;
using System.Linq;
using Mono.Cecil;

namespace SpecGate.Weaver;

/// <summary>
/// Resolves the <c>SpecGate.Runtime</c> members that woven code calls, imported
/// into the module being instrumented.
/// </summary>
internal sealed class RuntimeReferences
{
    private const string RuntimeAssembly = "SpecGate.Runtime";
    private const string RuntimeTypeName = "SpecGate.Runtime.SpecGateRuntime";
    private const string ScopeTypeName = "SpecGate.Runtime.OperationScope";
    private const string StagingTypeName = "SpecGate.Runtime.SetupStaging";

    private RuntimeReferences(
        TypeReference scopeType,
        TypeReference stagingType,
        MethodReference enterOperation,
        MethodReference enterOperationMulti,
        MethodReference recordInputs,
        MethodReference exitOk,
        MethodReference exitVoid,
        MethodReference exitError,
        MethodReference stageSetup,
        MethodReference commitSetup)
    {
        ScopeType = scopeType;
        StagingType = stagingType;
        EnterOperation = enterOperation;
        EnterOperationMulti = enterOperationMulti;
        RecordInputs = recordInputs;
        ExitOk = exitOk;
        ExitVoid = exitVoid;
        ExitError = exitError;
        StageSetup = stageSetup;
        CommitSetup = commitSetup;
    }

    internal TypeReference ScopeType { get; }

    internal TypeReference StagingType { get; }

    internal MethodReference StageSetup { get; }

    internal MethodReference CommitSetup { get; }

    internal MethodReference EnterOperation { get; }

    internal MethodReference EnterOperationMulti { get; }

    internal MethodReference RecordInputs { get; }

    internal MethodReference ExitOk { get; }

    internal MethodReference ExitVoid { get; }

    internal MethodReference ExitError { get; }

    /// <summary>
    /// Imports the runtime members into <paramref name="module"/>, loading
    /// <c>SpecGate.Runtime</c> from <paramref name="assemblyDirectory"/> when the
    /// module does not already reference it.
    /// </summary>
    /// <exception cref="WeaveException">The runtime is missing or unusable.</exception>
    internal static RuntimeReferences Resolve(ModuleDefinition module, string assemblyDirectory)
    {
        AssemblyDefinition runtime = LoadRuntime(module, assemblyDirectory);
        TypeDefinition runtimeType = FindType(runtime, RuntimeTypeName);
        TypeDefinition scopeType = FindType(runtime, ScopeTypeName);


        MethodDefinition[] entries =
        [
            .. runtimeType.Methods.Where(
                method => string.Equals(method.Name, "EnterOperation", StringComparison.Ordinal)),
        ];

        return new RuntimeReferences(
            module.ImportReference(scopeType),
            module.ImportReference(FindType(runtime, StagingTypeName)),
            module.ImportReference(SelectEnter(entries, isMulti: false)),
            module.ImportReference(SelectEnter(entries, isMulti: true)),
            module.ImportReference(FindMethod(runtimeType, "RecordInputs")),
            module.ImportReference(FindMethod(runtimeType, "ExitOk")),
            module.ImportReference(FindMethod(runtimeType, "ExitVoid")),
            module.ImportReference(FindMethod(runtimeType, "ExitError")),
            module.ImportReference(FindMethod(runtimeType, "StageSetup")),
            module.ImportReference(FindMethod(runtimeType, "CommitSetup")));
    }

    /// <summary>
    /// Loads <c>SpecGate.Runtime</c>. The C# compiler omits a reference the source
    /// never mentions, so an annotated assembly that only reaches the runtime
    /// through woven IL has no metadata reference yet. Importing members from the
    /// loaded assembly adds the reference.
    /// </summary>
    private static AssemblyDefinition LoadRuntime(ModuleDefinition module, string assemblyDirectory)
    {
        AssemblyNameReference? name = module.AssemblyReferences
            .FirstOrDefault(reference => string.Equals(reference.Name, RuntimeAssembly, StringComparison.Ordinal));
        if (name is not null)
        {
            return module.AssemblyResolver.Resolve(name)
                ?? throw new WeaveException($"Could not resolve {RuntimeAssembly} for '{module.Name}'.");
        }

        string path = Path.Combine(assemblyDirectory, RuntimeAssembly + ".dll");
        if (!File.Exists(path))
        {
            throw new WeaveException(
                $"'{module.Assembly.Name.Name}' declares SpecGate operations but {RuntimeAssembly}.dll is not "
                + $"in '{assemblyDirectory}'. Add a ProjectReference to csharp/SpecGate.Runtime.");
        }

        return AssemblyDefinition.ReadAssembly(
            path,
            new ReaderParameters { AssemblyResolver = module.AssemblyResolver });
    }

    private static MethodDefinition SelectEnter(MethodDefinition[] candidates, bool isMulti)
    {
        MethodDefinition? match = candidates.FirstOrDefault(
            method => method.Parameters.Count == 3 && method.Parameters[2].ParameterType.IsArray == isMulti);
        return match ?? throw new WeaveException(
            $"{RuntimeTypeName} has no EnterOperation overload taking "
            + (isMulti ? "a component array." : "a single component."));
    }

    private static TypeDefinition FindType(AssemblyDefinition assembly, string fullName)
    {
        return assembly.MainModule.GetType(fullName)
            ?? throw new WeaveException($"{RuntimeAssembly} does not define {fullName}.");
    }

    private static MethodDefinition FindMethod(TypeDefinition type, string name)
    {
        return type.Methods.FirstOrDefault(method => string.Equals(method.Name, name, StringComparison.Ordinal))
            ?? throw new WeaveException($"{type.FullName} does not define {name}.");
    }
}
