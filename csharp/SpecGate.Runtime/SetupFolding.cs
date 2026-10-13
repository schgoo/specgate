using System;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.Linq;
using System.Reflection;

namespace SpecGate.Runtime;

/// <summary>One setup an operation's recorded inputs must be folded from.</summary>
internal sealed class FoldedSetup(SetupKey key, string description)
{
    internal SetupKey Key { get; } = key;

    /// <summary>
    /// How the setup is named in a rejection, mirroring <c>describe_setup</c> in
    /// <c>rust/crates/specgate-runtime/src/capture/setup.rs:381-391</c>.
    /// </summary>
    internal string Description { get; } = description;
}

/// <summary>
/// Which setups contribute to one operation's recorded inputs, and which of its
/// declared parameters they replace.
/// </summary>
/// <remarks>
/// <para>
/// Mirrors <c>folded_inputs</c> in
/// <c>rust/crates/specgate-runtime/src/capture/setup.rs:295-375</c>, cross-checked
/// against the discovery-time authority <c>fold_operation</c> in
/// <c>rust/crates/specgate-discovery/src/discovery/setup.rs:79-173</c>, whose
/// wording the rejections below reuse.
/// </para>
/// <para>
/// Two deliberate differences from Rust capture. First, a setup whose
/// <c>Fills</c> names no declared parameter is rejected here; Rust capture drops
/// it silently while discovery errors, and a capture that silently diverged from
/// the published registry would record a surface nobody declared. Second, an
/// ambiguous or duplicated contribution is rejected here for the same reason.
/// Discovery's "an instance operation must have a receiver setup" rule is
/// deliberately <em>not</em> enforced: Rust capture does not enforce it either,
/// and a second enforcement point could only diverge from the first.
/// </para>
/// </remarks>
internal sealed class SetupFolding
{
    private static readonly ConcurrentDictionary<FoldingKey, SetupFolding> Cache = new();

    private SetupFolding(FoldedSetup[] required, HashSet<string> filled)
    {
        Required = required;
        Filled = filled;
    }

    /// <summary>
    /// The setups whose provenance this operation requires, receiver first and
    /// then parameter contributions in declaration order, which is the order
    /// their inputs merge in.
    /// </summary>
    internal FoldedSetup[] Required { get; }

    /// <summary>The declared parameters a setup contributes in place of.</summary>
    internal HashSet<string> Filled { get; }

    internal static SetupFolding None { get; } = new([], new HashSet<string>(StringComparer.Ordinal));

    internal static SetupFolding For(
        RuntimeMethodHandle method, string component, string operation, SemanticInput[] declared)
    {
        FoldingKey key = new(method, component, operation);
        if (Cache.TryGetValue(key, out SetupFolding? folded))
        {
            return folded;
        }

        // A rejected folding is not cached: it is raised again on the next
        // invocation rather than being remembered as a result.
        folded = Fold(key, declared);
        Cache[key] = folded;
        return folded;
    }

    private static SetupFolding Fold(FoldingKey key, SemanticInput[] declared)
    {
        MethodBase operationMethod = MethodBase.GetMethodFromHandle(key.Method)
            ?? throw new SpecGateCaptureException(
                CaptureRejection.UnsupportedType,
                "specgate: a woven operation's method could not be resolved.");

        // Rust capture reads the link-time registry. The C# equivalent is the
        // assembly that declares the operation, which is the same assembly C#
        // discovery reflects over.
        SetupDeclaration[] setups = SetupRegistry
            .For(operationMethod.DeclaringType!.Assembly)
            .SetupsFor(key.Component, key.Operation);
        if (setups.Length == 0)
        {
            return None;
        }

        HashSet<string> filled = new(StringComparer.Ordinal);
        List<FoldedSetup> parameters = [];
        FoldedSetup? receiver = null;

        foreach (SetupDeclaration setup in setups)
        {
            string? target = SelectTarget(key, declared, filled, setup);
            if (target is null)
            {
                if (receiver is not null)
                {
                    throw Ambiguous(
                        $"multiple receiver setups are registered for '{key.Component}::{key.Operation}'.");
                }

                receiver = new FoldedSetup(
                    setup.Key, $"setup '{setup.Key.Producer}' constructing the receiver");
                continue;
            }

            if (!filled.Add(target))
            {
                throw Ambiguous(
                    $"multiple setups for '{key.Component}::{key.Operation}' fill parameter '{target}'.");
            }

            parameters.Add(new FoldedSetup(
                setup.Key,
                setup.Key.Fills is null
                    ? $"setup '{setup.Key.Producer}' inferring folded parameter '{target}'"
                    : $"setup '{setup.Key.Producer}' filling parameter '{target}'"));
        }

        FoldedSetup[] required = receiver is null ? [.. parameters] : [receiver, .. parameters];
        return new SetupFolding(required, filled);
    }

    /// <summary>
    /// The parameter a setup contributes to, or <see langword="null"/> when it
    /// constructs the receiver.
    /// </summary>
    private static string? SelectTarget(
        FoldingKey key, SemanticInput[] declared, HashSet<string> filled, SetupDeclaration setup)
    {
        if (setup.Key.Fills is string fills)
        {
            if (!declared.Any(input => string.Equals(input.Name, fills, StringComparison.Ordinal)))
            {
                throw Ambiguous(
                    $"setup '{setup.Key.Producer}' for '{key.Component}::{key.Operation}' fills unknown "
                    + $"parameter '{fills}'.");
            }

            return fills;
        }

        // Rust matches whitespace-stripped declared type strings. The C#
        // equivalent is the mapped CTSC type, because two CLR types can map onto
        // one registry-visible type and discovery folds on what the registry
        // publishes, not on CLR identity.
        SemanticType? produces = setup.Produces;
        string[] candidates = produces is null
            ? []
            : [
                .. declared
                    .Where(input => !filled.Contains(input.Name) && produces.Equals(input.Type))
                    .Select(input => input.Name),
            ];

        if (candidates.Length == 1)
        {
            return candidates[0];
        }

        if (candidates.Length == 0)
        {
            return null;
        }

        throw Ambiguous(
            $"setup '{setup.Key.Producer}' for '{key.Component}::{key.Operation}' ambiguously matches "
            + $"parameters {string.Join(", ", candidates)} by its return type; set Fills explicitly.");
    }

    private static SpecGateCaptureException Ambiguous(string message) =>
        new(CaptureRejection.SetupAmbiguity, "specgate: " + message);

    /// <summary>
    /// A folding is decided by the woven method together with the component and
    /// operation it was entered for: one method may declare the same operation
    /// for several components, and each component folds its own setups.
    /// </summary>
    private readonly struct FoldingKey : IEquatable<FoldingKey>
    {
        internal FoldingKey(RuntimeMethodHandle method, string component, string operation)
        {
            Method = method;
            Component = component;
            Operation = operation;
        }

        internal RuntimeMethodHandle Method { get; }

        internal string Component { get; }

        internal string Operation { get; }

        public bool Equals(FoldingKey other) =>
            Method.Equals(other.Method)
            && string.Equals(Component, other.Component, StringComparison.Ordinal)
            && string.Equals(Operation, other.Operation, StringComparison.Ordinal);

        public override bool Equals(object? obj) => obj is FoldingKey other && Equals(other);

        public override int GetHashCode()
        {
            unchecked
            {
                int hash = (17 * 31) + Method.GetHashCode();
                hash = (hash * 31) + Component.GetHashCode();
                return (hash * 31) + Operation.GetHashCode();
            }
        }
    }
}
