using Xunit;

namespace SpecGate.Runtime.Tests;

/// <summary>
/// Protocol identities and deterministic configuration are validated at
/// construction, so an invalid capture cannot reach a sidecar.
/// </summary>
/// <remarks>
/// A consumer that rejects a malformed artifact cannot say which producer wrote
/// it, so both producers refuse to build one. These rules mirror Rust's
/// <c>validate_hex_id</c> and <c>validate_config</c>.
/// </remarks>
public class CaptureConfigTests
{
    private const string Trace = "0af7651916cd43dd8448eb211c80319c";
    private const string Run = "aaaaaaaaaaaa0001";
    private const string Scenario = "aaaaaaaaaaaa0002";

    [Theory]
    [InlineData("")]
    [InlineData("0af7651916cd43dd8448eb211c80319")]
    [InlineData("0af7651916cd43dd8448eb211c80319cc")]
    [InlineData("0AF7651916CD43DD8448EB211C80319C")]
    [InlineData("0af7651916cd43dd8448eb211c80319g")]
    [InlineData("00000000000000000000000000000000")]
    public void AMalformedTraceIdIsRejected(string text) =>
        Assert.Throws<ArgumentException>(() => TraceId.Parse(text));

    [Theory]
    [InlineData("aaaaaaaaaaaa000")]
    [InlineData("aaaaaaaaaaaa00011")]
    [InlineData("AAAAAAAAAAAA0001")]
    [InlineData("zaaaaaaaaaaa0001")]
    [InlineData("0000000000000000")]
    public void AMalformedSpanIdIsRejected(string text) =>
        Assert.Throws<ArgumentException>(() => SpanId.Parse(text));

    [Fact]
    public void ANullIdentityIsRejected()
    {
        Assert.Throws<ArgumentNullException>(() => TraceId.Parse(null!));
        Assert.Throws<ArgumentNullException>(() => SpanId.Parse(null!));
    }

    [Fact]
    public void TwoIdentitiesOfTheSameKindAndTextAreEqual()
    {
        Assert.Equal(SpanId.Parse(Run), SpanId.Parse(Run));
        Assert.NotEqual(SpanId.Parse(Run), SpanId.Parse(Scenario));
        Assert.Equal(Run, SpanId.Parse(Run).Text);
    }

    [Fact]
    public void TheDefaultClockStartsAtZeroAndAdvancesByOne()
    {
        CaptureConfig config = Builder().Build();

        Assert.Equal(0, config.StartTimeUnixNano);
        Assert.Equal(1, config.ClockStepUnixNano);
        Assert.Empty(config.OperationSpanIds);
    }

    [Fact]
    public void ANegativeStartTimeIsRejected() =>
        Assert.Throws<ArgumentException>(() => Builder().StartTime(-1).Build());

    [Theory]
    [InlineData(0)]
    [InlineData(-1)]
    public void ANonPositiveClockStepIsRejected(long step) =>
        Assert.Throws<ArgumentException>(() => Builder().ClockStep(step).Build());

    [Fact]
    public void ARunAndScenarioSharingOneSpanIdAreRejected() =>
        Assert.Throws<ArgumentException>(
            () => CaptureConfig.Builder("scenario", TraceId.Parse(Trace), SpanId.Parse(Run), SpanId.Parse(Run)).Build());

    [Fact]
    public void ADuplicatedOperationSpanIdIsRejected() =>
        Assert.Throws<ArgumentException>(
            () => Builder().OperationSpanIds([SpanId.Parse("aaaaaaaaaaaa0010"), SpanId.Parse("aaaaaaaaaaaa0010")]).Build());

    [Fact]
    public void AnOperationSpanIdThatCollidesWithARootIsRejected() =>
        Assert.Throws<ArgumentException>(() => Builder().OperationSpanIds([SpanId.Parse(Scenario)]).Build());

    [Fact]
    public void ANullOperationSpanIdIsRejected() =>
        Assert.Throws<ArgumentException>(() => Builder().OperationSpanIds([null!]).Build());

    [Fact]
    public void SuppliedOperationSpanIdsAreKeptInOrder()
    {
        SpanId first = SpanId.Parse("aaaaaaaaaaaa0010");
        SpanId second = SpanId.Parse("aaaaaaaaaaaa0011");

        CaptureConfig config = Builder().OperationSpanIds([first, second]).Build();

        Assert.Equal([first, second], config.OperationSpanIds);
    }

    private static CaptureConfigBuilder Builder() =>
        CaptureConfig.Builder("scenario", TraceId.Parse(Trace), SpanId.Parse(Run), SpanId.Parse(Scenario));
}
