using SpecGate.CtscFixtures.Conformance.SumTypes;
using Xunit;

namespace SpecGate.CtscFixtures.Tests;

public sealed partial class SumTypeBehaviorTests
{
    [Fact]
    public void TaggedUnionSupportsUnitTupleLikeAndNamedVariants()
    {
        Assert.IsType<Point>(EnumEvent.Classify(0));
        Assert.Equal(5.0, Assert.IsType<Circle>(EnumEvent.Classify(1)).Radius);
        Assert.Equal("triangle", Assert.IsType<Tag>(EnumEvent.Classify(3)).Value);

        Rectangle rectangle = Assert.IsType<Rectangle>(EnumEvent.Classify(4));
        Assert.Equal(3.0, rectangle.Width);
        Assert.Equal(4.0, rectangle.Height);
    }
}
