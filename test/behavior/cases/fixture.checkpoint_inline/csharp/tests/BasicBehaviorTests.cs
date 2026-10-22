using SpecGate.CtscFixtures.Conformance.Basic;
using Xunit;

namespace SpecGate.CtscFixtures.Tests;

public sealed partial class BasicBehaviorTests
{
    private static void NamedInputsAndCheckpointBehaviorRemainOrdinaryMethodsStep1()
    {
        Assert.Equal("  HELLO  ", CheckpointInline.AfterUpper("  HELLO  "));
        Assert.Equal("HELLO", CheckpointInline.Process("  hello  "));
    }
}
