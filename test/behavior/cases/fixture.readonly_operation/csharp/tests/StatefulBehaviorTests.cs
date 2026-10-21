using Xunit;
using Readonly = SpecGate.CtscFixtures.Conformance.Stateful.ReadonlyOperation;

namespace SpecGate.CtscFixtures.Tests;

public sealed partial class StatefulBehaviorTests
{
    private static void ParameterizedReadonlyStateMachineAndVoidSurfacesRemainBehavioralStep1()
    {
        Readonly.Counter readOnly = Readonly.Counter.Make();
        Assert.Equal(42, readOnly.GetCount());
        Assert.Equal(42, readOnly.Count);
    }
}
