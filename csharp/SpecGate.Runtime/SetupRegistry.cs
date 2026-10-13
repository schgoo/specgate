using System;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.Linq;
using System.Reflection;
using SpecGate.Annotations;

namespace SpecGate.Runtime;

/// <summary>
/// Identity of one <see cref="SpecSetupAttribute"/> declaration inside a
/// capture.
/// </summary>
/// <remarks>
/// Mirrors <c>SetupKey</c> in
/// <c>rust/crates/specgate-runtime/src/capture/setup.rs:11-20</c>: component,
/// operation, module path, function name, and the filled parameter. The C#
/// analogue of a Rust module path is the declaring type's source-style name,
/// which is the <c>cs_class</c> that C# discovery publishes
/// (<c>rust/crates/specgate-discovery/src/csharp_discovery/program.rs:297</c>).
/// </remarks>
internal readonly struct SetupKey : IEquatable<SetupKey>
{
    internal SetupKey(string component, string operation, string declaringType, string method, string? fills)
    {
        Component = component;
        Operation = operation;
        DeclaringType = declaringType;
        Method = method;
        Fills = fills;
    }

    internal string Component { get; }

    internal string Operation { get; }

    /// <summary>The declaring type's source-style name, nested types spelled with '.'.</summary>
    internal string DeclaringType { get; }

    internal string Method { get; }

    /// <summary>The operation parameter this declaration fills, or null when it infers one.</summary>
    internal string? Fills { get; }

    /// <summary>The producer as it is named in a rejection message.</summary>
    internal string Producer => $"{DeclaringType}.{Method}";

    public bool Equals(SetupKey other) =>
        string.Equals(Component, other.Component, StringComparison.Ordinal)
        && string.Equals(Operation, other.Operation, StringComparison.Ordinal)
        && string.Equals(DeclaringType, other.DeclaringType, StringComparison.Ordinal)
        && string.Equals(Method, other.Method, StringComparison.Ordinal)
        && string.Equals(Fills, other.Fills, StringComparison.Ordinal);

    public override bool Equals(object? obj) => obj is SetupKey other && Equals(other);

    public override int GetHashCode()
    {
        unchecked
        {
            int hash = 17;
            hash = (hash * 31) + Component.GetHashCode();
            hash = (hash * 31) + Operation.GetHashCode();
            hash = (hash * 31) + DeclaringType.GetHashCode();
            hash = (hash * 31) + Method.GetHashCode();
            return (hash * 31) + (Fills?.GetHashCode() ?? 0);
        }
    }
}

/// <summary>One setup declaration, resolved to the component it prepares for.</summary>
internal sealed class SetupDeclaration
{
    internal SetupDeclaration(MethodInfo method, SetupKey key)
    {
        Method = method;
        Key = key;
    }

    internal MethodInfo Method { get; }

    internal SetupKey Key { get; }

    /// <summary>
    /// What the setup produces, or <see langword="null"/> when it returns void.
    /// A void setup matches no parameter, so it can only construct the receiver.
    /// </summary>
    internal SemanticType? Produces => OperationMetadata.For(Method.MethodHandle).Result;
}

/// <summary>
/// The setup declarations an assembly carries, as capture sees them.
/// </summary>
/// <remarks>
/// <para>
/// Rust capture reads link-time registry metadata. C# has no such table, so the
/// equivalent is reflection over the assembly that declares the operation, which
/// is exactly what C# discovery does. The selection here is
/// <c>rust/crates/specgate-discovery/src/csharp_discovery/program.rs:355-365</c>
/// - an explicitly scoped setup belongs to its own component, an unscoped setup
/// belongs to whichever component declares the operation it prepares - composed
/// with <c>registry.setups_for(component, op.name)</c>.
/// </para>
/// <para>
/// Declaration order is reflection order, the same order discovery emits, so
/// capture folds the setups of one operation in the order the published registry
/// lists them.
/// </para>
/// </remarks>
internal sealed class SetupRegistry
{
    private const BindingFlags Selection = BindingFlags.Public | BindingFlags.NonPublic
        | BindingFlags.Static | BindingFlags.Instance | BindingFlags.DeclaredOnly;

    private static readonly ConcurrentDictionary<Assembly, SetupRegistry> Cache = new();
    private static readonly SetupDeclaration[] NoSetups = [];

    private readonly SetupDeclaration[] _setups;

    private SetupRegistry(SetupDeclaration[] setups) => _setups = setups;

    internal static SetupRegistry For(Assembly assembly) => Cache.GetOrAdd(assembly, Describe);

    /// <summary>The setups folded into one operation, in declaration order.</summary>
    internal SetupDeclaration[] SetupsFor(string component, string operation)
    {
        if (_setups.Length == 0)
        {
            return NoSetups;
        }

        return
        [
            .. _setups.Where(setup =>
                string.Equals(setup.Key.Component, component, StringComparison.Ordinal)
                && string.Equals(setup.Key.Operation, operation, StringComparison.Ordinal)),
        ];
    }

    /// <summary>
    /// The declarations one setup method carries for <paramref name="component"/>.
    /// A single method may declare several, so one invocation records provenance
    /// for each of them.
    /// </summary>
    internal SetupDeclaration[] DeclarationsOf(RuntimeMethodHandle method, string component)
    {
        if (_setups.Length == 0)
        {
            return NoSetups;
        }

        return
        [
            .. _setups.Where(setup =>
                setup.Method.MethodHandle.Equals(method)
                && string.Equals(setup.Key.Component, component, StringComparison.Ordinal)),
        ];
    }

    private static SetupRegistry Describe(Assembly assembly)
    {
        List<MethodInfo> annotated = [];
        Dictionary<string, List<string>> operationOwners = new(StringComparer.Ordinal);

        foreach (Type type in assembly.GetTypes())
        {
            foreach (MethodInfo method in type.GetMethods(Selection))
            {
                foreach (SpecOperationAttribute operation in
                    method.GetCustomAttributes<SpecOperationAttribute>(inherit: false))
                {
                    if (operation.Spec is null)
                    {
                        continue;
                    }

                    if (!operationOwners.TryGetValue(operation.Name, out List<string>? owners))
                    {
                        owners = [];
                        operationOwners.Add(operation.Name, owners);
                    }

                    if (!owners.Contains(operation.Spec, StringComparer.Ordinal))
                    {
                        owners.Add(operation.Spec);
                    }
                }

                if (method.GetCustomAttributes<SpecSetupAttribute>(inherit: false).Any())
                {
                    annotated.Add(method);
                }
            }
        }

        List<SetupDeclaration> setups = [];
        foreach (MethodInfo method in annotated)
        {
            foreach (SpecSetupAttribute setup in method.GetCustomAttributes<SpecSetupAttribute>(inherit: false))
            {
                if (setup.Spec is not null)
                {
                    setups.Add(Declare(method, setup, setup.Spec));
                }
                else if (operationOwners.TryGetValue(setup.Name, out List<string>? owners))
                {
                    foreach (string owner in owners)
                    {
                        setups.Add(Declare(method, setup, owner));
                    }
                }
            }
        }

        return new SetupRegistry(setups.Count == 0 ? NoSetups : [.. setups]);
    }

    private static SetupDeclaration Declare(MethodInfo method, SpecSetupAttribute setup, string component) =>
        new(
            method,
            new SetupKey(
                component,
                setup.Name,
                SourceTypeName(method.DeclaringType),
                method.Name,
                string.IsNullOrEmpty(setup.Fills) ? null : setup.Fills));

    /// <summary>
    /// Reflection spells a nested type with '+', while the registry replays it as
    /// a C# source name. This is discovery's <c>SourceTypeName</c>
    /// (<c>csharp_discovery/program.rs:297</c>).
    /// </summary>
    private static string SourceTypeName(Type? type) =>
        (type?.FullName ?? type?.Name ?? string.Empty).Replace('+', '.');
}
