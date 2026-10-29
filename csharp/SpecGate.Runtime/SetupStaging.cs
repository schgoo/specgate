using System;
using System.Collections.Generic;
using System.Reflection;

namespace SpecGate.Runtime;

/// <summary>
/// The construction inputs a setup producer projected at entry, waiting for the
/// producer to return successfully. Woven code treats this as an opaque token.
/// </summary>
/// <remarks>
/// Mirrors <c>DeferredSetup</c> in
/// <c>rust/crates/specgate-runtime/src/capture/setup.rs:22-93</c>. The inputs
/// are projected when the producer is entered, so a body that mutates its own
/// arguments cannot change what provenance records, and they are committed only
/// once the producer has returned.
/// </remarks>
public sealed class SetupStaging
{
    private readonly CaptureSession _session;
    private readonly SetupDeclaration[] _declarations;
    private readonly SortedDictionary<string, Value> _inputs;

    private SetupStaging(
        CaptureSession session,
        SetupDeclaration[] declarations,
        SortedDictionary<string, Value> inputs)
    {
        _session = session;
        _declarations = declarations;
        _inputs = inputs;
    }

    /// <summary>
    /// Projects the construction inputs of one setup invocation, or returns
    /// <see langword="null"/> when the method declares no setup for the session's
    /// component.
    /// </summary>
    internal static SetupStaging? Stage(
        CaptureSession session, RuntimeMethodHandle method, object?[] arguments)
    {
        MethodBase producer = MethodBase.GetMethodFromHandle(method)
            ?? throw new SpecGateCaptureException(
                CaptureRejection.UnsupportedType,
                "specgate: a woven setup's method could not be resolved.");

        SetupDeclaration[] declarations = SetupRegistry
            .For(producer.DeclaringType!.Assembly)
            .DeclarationsOf(method, session.RecordsAllComponents ? null : session.Component);
        if (declarations.Length == 0)
        {
            return null;
        }

        SemanticInput[] declared = OperationMetadata.For(method).Inputs;
        if (declared.Length != arguments.Length)
        {
            throw new SpecGateCaptureException(
                CaptureRejection.TypeMismatch,
                $"specgate: setup '{declarations[0].Key.Producer}' received {arguments.Length} "
                + $"argument(s) but declares {declared.Length}.");
        }

        SortedDictionary<string, Value> inputs = new(Utf8KeyComparer.Instance);
        for (int index = 0; index < declared.Length; index++)
        {
            foreach (KeyValuePair<string, Value> leaf in InputFolding.Project(declared[index], arguments[index]))
            {
                if (!inputs.TryAdd(leaf.Key, leaf.Value))
                {
                    throw new SpecGateCaptureException(
                        CaptureRejection.AmbiguousDeclaration,
                        $"specgate: setup '{declarations[0].Key.Producer}' produces duplicate input name '{leaf.Key}'.");
                }
            }
        }

        return new SetupStaging(session, declarations, inputs);
    }

    /// <summary>
    /// Records the staged provenance. One method may carry several
    /// <c>[SpecSetup]</c> declarations, and a single invocation satisfies every
    /// one of them.
    /// </summary>
    internal void Commit()
    {
        foreach (SetupDeclaration declaration in _declarations)
        {
            _session.RecordSetup(declaration.Key, _inputs);
        }
    }
}
