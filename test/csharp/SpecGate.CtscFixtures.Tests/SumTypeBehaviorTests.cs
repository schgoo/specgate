using SpecGate.Annotations;
using SpecGate.CtscFixtures.Conformance.SumTypes;
using Xunit;

namespace SpecGate.CtscFixtures.Tests;

public sealed class SumTypeBehaviorTests
{
    [Fact]
    public void OptionAndResultSurfacesCoverBothArms()
    {
        Option<int> found = OptionResult.Find([4, 7, 9], 7);
        Assert.True(found.HasValue);
        Assert.Equal(1, found.Value);
        Assert.False(OptionResult.Find([4, 7, 9], 8).HasValue);

        Result<int, string> success = OptionResult.TryDivide(12, 4);
        Assert.True(success.IsOk);
        Assert.Equal(3, success.OkValue);

        Result<int, string> failure = OptionResult.TryDivide(12, 0);
        Assert.False(failure.IsOk);
        Assert.Equal("division by zero", failure.ErrValue);
    }

    [Fact]
    public void DeclaredErrorsRemainDistinctFromUndeclaredFaults()
    {
        Assert.Equal(4, DeclaredErrors.CheckedDivide(12, 3));
        Assert.Equal(
            "division by zero",
            Assert.Throws<DivideByZeroException>(
                () => DeclaredErrors.CheckedDivide(12, 0)).Message);
        Assert.Equal(
            "negative divisor",
            Assert.Throws<InvalidOperationException>(
                () => DeclaredErrors.CheckedDivide(12, -3)).Message);

        Assert.Equal(40, DeclaredErrors.RequireInRange(40));
        Assert.IsType<InvalidOperationException>(
            Assert.ThrowsAny<Exception>(() => DeclaredErrors.RequireInRange(-1)));
        Assert.IsType<FormatException>(
            Assert.ThrowsAny<Exception>(() => DeclaredErrors.RequireInRange(101)));
        Assert.Equal(
            "attempt to divide by zero",
            Assert.Throws<InvalidOperationException>(
                () => DeclaredErrors.Divide(1, 0)).Message);
    }

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
