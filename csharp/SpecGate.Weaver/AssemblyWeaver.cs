using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using Mono.Cecil;
using Mono.Cecil.Cil;

namespace SpecGate.Weaver;

/// <summary>
/// Instruments every annotated method in one compiled assembly, in place.
/// </summary>
public sealed class AssemblyWeaver
{
    private const string RuntimeTypeName = "SpecGate.Runtime.SpecGateRuntime";

    /// <summary>
    /// Weaves <paramref name="assemblyPath"/> in place.
    /// </summary>
    /// <param name="assemblyPath">The compiled assembly to instrument.</param>
    /// <returns>The names of the operations woven, empty when nothing changed.</returns>
    /// <exception cref="WeaveException">The assembly declares an operation that cannot be woven.</exception>
    public static IReadOnlyList<string> Weave(string assemblyPath)
    {
        if (!File.Exists(assemblyPath))
        {
            throw new WeaveException($"Assembly to weave does not exist: {assemblyPath}");
        }

        string pdbPath = Path.ChangeExtension(assemblyPath, ".pdb");
        DefaultAssemblyResolver resolver = new();
        string? directory = Path.GetDirectoryName(assemblyPath);
        if (!string.IsNullOrEmpty(directory))
        {
            resolver.AddSearchDirectory(directory);
        }

        ReaderParameters readerParameters = new()
        {
            AssemblyResolver = resolver,
            ReadSymbols = File.Exists(pdbPath),
            ReadWrite = false,
        };

        string tempAssembly = assemblyPath + ".specgate-weave";
        string tempPdb = Path.ChangeExtension(tempAssembly, ".pdb");
        List<string> woven = [];

        using (AssemblyDefinition assembly = AssemblyDefinition.ReadAssembly(assemblyPath, readerParameters))
        {
            ModuleDefinition module = assembly.MainModule;
            List<WeaveTarget> targets = FindOperations(module);
            if (targets.Count == 0)
            {
                return [];
            }

            RuntimeReferences runtime = RuntimeReferences.Resolve(module, directory ?? ".");
            MethodWeaver weaver = new(module, runtime);
            foreach (WeaveTarget target in targets)
            {
                if (target.Operation is not null)
                {
                    weaver.Weave(target.Method, target.Operation);
                    woven.Add($"{target.Operation.Operation} ({target.Method.FullName})");
                }
                else
                {
                    weaver.WeaveSetup(target.Method, target.Setup!);
                    woven.Add($"setup ({target.Method.FullName})");
                }
            }

            assembly.Write(tempAssembly, new WriterParameters { WriteSymbols = readerParameters.ReadSymbols });
        }

        File.Copy(tempAssembly, assemblyPath, overwrite: true);
        File.Delete(tempAssembly);
        if (readerParameters.ReadSymbols && File.Exists(tempPdb))
        {
            File.Copy(tempPdb, pdbPath, overwrite: true);
            File.Delete(tempPdb);
        }

        woven.Sort(StringComparer.Ordinal);
        return woven;
    }

    private static List<WeaveTarget> FindOperations(ModuleDefinition module)
    {
        List<WeaveTarget> targets = [];
        foreach (TypeDefinition type in module.Types.SelectMany(FlattenTypes))
        {
            foreach (MethodDefinition method in type.Methods)
            {
                OperationDeclaration? operation = OperationDeclaration.TryRead(method);
                SetupDeclaration? setup = SetupDeclaration.TryRead(method);
                if (operation is not null && setup is not null)
                {
                    // The woven shapes differ - an operation opens a scope inside
                    // an exception handler, a setup commits provenance on its
                    // success path only - and no combined shape is designed. A
                    // method that is both is a declaration defect, so it is
                    // refused rather than woven one way and silently not the
                    // other.
                    throw new WeaveException(
                        $"{method.FullName} is annotated as both a SpecGate operation and a SpecGate setup. "
                        + "Declare the setup on a separate producer method.");
                }

                if (operation is null && setup is null)
                {
                    continue;
                }

                if (!IsWoven(method))
                {
                    targets.Add(new WeaveTarget(method, operation, setup));
                }
            }
        }

        return targets;
    }

    private static IEnumerable<TypeDefinition> FlattenTypes(TypeDefinition type)
    {
        yield return type;
        foreach (TypeDefinition nested in type.NestedTypes.SelectMany(FlattenTypes))
        {
            yield return nested;
        }
    }

    /// <summary>
    /// Weaving runs after the compiled assembly is copied to the output directory,
    /// so an incremental build can hand the weaver an already-instrumented
    /// assembly. A method whose body already calls into the capture runtime has
    /// been woven, and weaving it again would record the operation, or commit the
    /// setup provenance, twice.
    /// </summary>
    private static bool IsWoven(MethodDefinition method)
    {
        foreach (Instruction instruction in method.Body.Instructions)
        {
            if (instruction.Operand is MethodReference callee
                && (string.Equals(callee.Name, "EnterOperation", StringComparison.Ordinal)
                    || string.Equals(callee.Name, "StageSetup", StringComparison.Ordinal))
                && string.Equals(callee.DeclaringType.FullName, RuntimeTypeName, StringComparison.Ordinal))
            {
                return true;
            }
        }

        return false;
    }

    /// <summary>One annotated method, with the declaration the weaver read off it.</summary>
    private sealed class WeaveTarget(
        MethodDefinition method, OperationDeclaration? operation, SetupDeclaration? setup)
    {
        internal MethodDefinition Method { get; } = method;

        internal OperationDeclaration? Operation { get; } = operation;

        internal SetupDeclaration? Setup { get; } = setup;
    }
}
