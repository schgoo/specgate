using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Runtime.Loader;
using System.Text.Json;

namespace SpecGate.Discovery;

/// <summary>
/// Reflects over a compiled candidate assembly once and emits one raw registry
/// JSON document per requested component.
/// </summary>
internal static class DiscoveryRunner
{
    /// <summary>
    /// The Rust batch reader pairs each requested component with its zero-based
    /// position, so these positional filenames are part of the runner protocol.
    /// </summary>
    private const string ComponentOutputExtension = ".json";

    /// <summary>
    /// Runs discovery.
    /// </summary>
    /// <param name="args">
    /// <c>[0]</c> output directory, <c>[1]</c> candidate assembly path,
    /// <c>[2]</c> the candidate's build output directory holding its copy-local
    /// dependency assemblies, and <c>[3..]</c> the requested component names.
    /// With no component names, every inventoried component is published, in
    /// inventory order.
    /// </param>
    /// <param name="runnerSystem">Filesystem and assembly-loading adapter.</param>
    /// <returns>Zero on success.</returns>
    internal static int Run(string[] args, RunnerSystem runnerSystem)
    {
        ArgumentNullException.ThrowIfNull(args);
        ArgumentNullException.ThrowIfNull(runnerSystem);

        if (args.Length < 3)
        {
            Console.Error.WriteLine(
                "specgate-discovery: expected <output-directory> <assembly-path> <assembly-output-directory> [component...]");
            return 2;
        }

        string outputDirectory = args[0];
        string fixtureDll = args[1];
        string fixtureOut = args[2];
        string[] requestedComponents = [.. args.Skip(3)];

        // Resolve the candidate's dependency assemblies from the build output dir so
        // LoadFrom + GetTypes() + attribute reads + NullabilityInfoContext work
        // against the real assembly and its references.
        AssemblyDependencyResolver dependencyResolver = runnerSystem.CreateResolver(fixtureDll);
        using IDisposable resolverSubscription = runnerSystem.SubscribeResolver((context, name) =>
        {
            string candidate = Path.Combine(fixtureOut, name.Name + ".dll");
            if (runnerSystem.FileExists(candidate)) return runnerSystem.LoadFromPath(context, candidate);
            string? resolved = runnerSystem.ResolveAssemblyPath(dependencyResolver, name);
            return resolved is not null ? runnerSystem.LoadFromPath(context, resolved) : null;
        });

        var nullability = new NullabilityInfoContext();
        Assembly fixtureAssembly = runnerSystem.LoadAssembly(fixtureDll);

        Type[] allTypes;
        try
        {
            allTypes = fixtureAssembly.GetTypes();
        }
        catch (ReflectionTypeLoadException ex)
        {
            allTypes = [.. ex.Types.Where(type => type is not null).Select(type => type!)];
        }

        AnnotatedMethod[] annotatedMethods = FindAnnotatedMethods(allTypes);

        string? conflict = MultiComponentConflict(annotatedMethods);
        if (conflict is not null)
        {
            Console.Error.WriteLine("specgate-discovery: " + conflict);
            return 1;
        }

        runnerSystem.CreateDirectory(outputDirectory);

        string[] inventory =
        [
            .. annotatedMethods
                .SelectMany(entry => entry.Operations)
                .Select(operation => Annotations.Text(operation, "Spec"))
                .Where(spec => !string.IsNullOrEmpty(spec))
                .Select(spec => spec!)
                .Distinct(StringComparer.Ordinal)
                .OrderBy(spec => spec, StringComparer.Ordinal),
        ];
        runnerSystem.WriteAllText(
            Path.Combine(outputDirectory, "components.json"),
            JsonSerializer.Serialize(inventory));

        if (requestedComponents.Length == 0)
        {
            requestedComponents = inventory;
        }

        for (int componentIndex = 0; componentIndex < requestedComponents.Length; componentIndex++)
        {
            var document = new ComponentDocument(
                requestedComponents[componentIndex],
                allTypes,
                annotatedMethods,
                nullability);

            object payload;
            try
            {
                payload = document.Build();
            }
            catch (GenericSpecializationException exception)
            {
                Console.Error.WriteLine("specgate-discovery: " + exception.Message);
                return 1;
            }

            runnerSystem.WriteAllText(
                Path.Combine(outputDirectory, componentIndex + ComponentOutputExtension),
                JsonSerializer.Serialize(payload));
        }

        return 0;
    }

    /// <summary>
    /// Describes the first method that declares an operation for more than one
    /// component, or returns <see langword="null"/> when every operation belongs
    /// to exactly one.
    /// </summary>
    /// <remarks>
    /// An operation belongs to exactly one component. The weaver rejects the same
    /// declaration, so discovery fails closed with the same rule instead of
    /// publishing a registry capture could never honor.
    /// </remarks>
    internal static string? MultiComponentConflict(IEnumerable<AnnotatedMethod> methods)
    {
        foreach (AnnotatedMethod entry in methods)
        {
            if (entry.Operations.Length > 1
                && (!entry.Method.IsGenericMethodDefinition
                    || entry.Operations.Select(operation => Annotations.Text(operation, "Spec")).Distinct(StringComparer.Ordinal).Count() > 1))
            {
                string components = string.Join(
                    ", ",
                    entry.Operations.Select(operation => $"'{Annotations.Text(operation, "Spec")}'"));
                return $"{entry.Method.DeclaringType?.FullName}.{entry.Method.Name} declares an operation for "
                    + $"{entry.Operations.Length} components ({components}). An operation belongs to exactly one "
                    + "component; split it into one method per component.";
            }
        }

        return null;
    }

    /// <summary>
    /// Every method carrying a SpecGate operation or setup annotation.
    /// </summary>
    /// <remarks>
    /// Method reflection is deliberately <c>Public|NonPublic</c>: an annotation on
    /// a private method still declares a component, and hiding it here would let it
    /// escape both the inventory and the semantic validation that rejects a
    /// non-public operation. <c>is_public</c> preserves the real accessibility, so
    /// normalization still rejects it by name. <c>DeclaredOnly</c> stays, so an
    /// inherited or compiler-generated member of a base type is never attributed to
    /// a derived one.
    /// </remarks>
    private static AnnotatedMethod[] FindAnnotatedMethods(IEnumerable<Type> allTypes) =>
        [
            .. allTypes
                .SelectMany(type => type.GetMethods(
                    BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Static | BindingFlags.Instance | BindingFlags.DeclaredOnly)
                    .Cast<MethodBase>().Concat(type.GetConstructors(
                        BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance | BindingFlags.DeclaredOnly)))
                .Select(method => new AnnotatedMethod(
                    method,
                    Annotations.On(method, Annotations.Operation, inherit: true),
                    Annotations.On(method, Annotations.Setup, inherit: true)))
                .Where(entry => entry.Operations.Length != 0 || entry.Setups.Length != 0),
        ];
}
