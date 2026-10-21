using System.Collections.Generic;
using Mono.Cecil;

namespace SpecGate.Weaver;

/// <summary>
/// The SpecGate setup annotations the weaver reads off a compiled method.
/// </summary>
/// <remarks>
/// One method may carry several <c>[SpecSetup]</c> declarations — see
/// <c>test/behavior/cases/fixture.shared_setup/csharp/SharedSetup.cs</c>,
/// where one producer fills both parameters of an operation — so a single
/// invocation commits provenance for every declaration it carries. The weaver
/// does not need the declarations themselves: capture resolves them by
/// reflection over the declaring assembly, mirroring
/// <c>rust/crates/specgate-discovery/src/csharp_discovery/program.rs:355-365</c>.
/// </remarks>
internal sealed class SetupDeclaration
{
    private const string Kind = "setup";

    private SetupDeclaration(ParameterDefinition[] inputs) => Inputs = inputs;

    /// <summary>
    /// Gets the parameters projected as construction inputs, in declaration
    /// order. Their recorded names come from reflection at capture time.
    /// </summary>
    internal ParameterDefinition[] Inputs { get; }

    /// <summary>
    /// Reads the setup declaration from <paramref name="method"/>, or returns
    /// <see langword="null"/> when the method carries no setup annotation.
    /// </summary>
    /// <exception cref="WeaveException">The method is annotated but cannot be woven.</exception>
    internal static SetupDeclaration? TryRead(MethodDefinition method)
    {
        List<CustomAttribute> annotations =
            AnnotatedMethod.Annotations(method, AnnotatedMethod.SpecSetupAttribute);
        if (annotations.Count == 0)
        {
            return null;
        }

        AnnotatedMethod.RejectUnsupported(method, Kind);

        return new SetupDeclaration(AnnotatedMethod.ReadInputs(method, Kind));
    }
}
