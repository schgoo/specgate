using Xunit;
using VoidOperation = SpecGate.CtscFixtures.Conformance.Stateful.VoidOperation;

namespace SpecGate.CtscFixtures.Tests;

public sealed partial class StatefulBehaviorTests
{
    private static void ParameterizedReadonlyStateMachineAndVoidSurfacesRemainBehavioralStep3()
    {
        VoidOperation.Logger logger = VoidOperation.Logger.Make();
        logger.Log("captured");
        Assert.Equal(1, logger.Count);
    }
}
