using SpecGate.Runtime;
using SpecGateFixtures.Conformance.Basic;
using SpecGateFixtures.Conformance.Mocks.MockField;
using SpecGateFixtures.Conformance.Stateful.NestedOperations;
using Xunit;

namespace SpecGateFixtures.Tests;

/// <summary>
/// Proves the compiled <c>SpecGateFixtures</c> assembly is instrumented at build
/// time by the SpecGate IL weaver — the C# analog of the Rust proc-macro. These
/// tests exercise the real built types directly (not a source-globbed surrogate),
/// so they only pass when the weaver has injected the same instrumentation that
/// <c>instrument_csharp_source</c> produces: <c>[SpecOperation]</c> entry traces,
/// <c>[SpecEvent]</c> property-setter emissions, and <c>[SpecMock]</c> call
/// redirection.
/// </summary>
public class WeaverTests
{
    /// <summary>
    /// A woven <c>[SpecOperation]</c> body must call
    /// <see cref="SpecGateRuntime.EnterOperation"/> on entry, so nested operation
    /// calls (<c>transfer</c> → <c>withdraw</c>/<c>deposit</c>) emit their own Run
    /// and input events.
    /// </summary>
    [Fact]
    public void SpecOperation_EntryEmitsNestedRunTraces()
    {
        SpecGateRuntime.Reset();
        var account = Account.Make();
        SpecGateRuntime.RegisterObject(account, null);

        account.Transfer(30);

        string json = SpecGateRuntime.GetTracesJson();
        Assert.Contains("{\"kind\":\"Run\",\"operation\":\"withdraw\"}", json);
        Assert.Contains("{\"kind\":\"Event\",\"name\":\"withdraw.amount\",\"value\":30}", json);
        Assert.Contains("{\"kind\":\"Run\",\"operation\":\"deposit\"}", json);
        Assert.Contains("{\"kind\":\"Event\",\"name\":\"deposit.amount\",\"value\":30}", json);
    }

    /// <summary>
    /// A woven static <c>[SpecOperation]</c> body must also call
    /// <see cref="SpecGateRuntime.EnterOperation"/> on entry, so a static
    /// operation invoked inside another operation self-reports its nested run.
    /// </summary>
    [Fact]
    public void SpecOperation_StaticEntryEmitsRunTrace()
    {
        SpecGateRuntime.Reset();

        int sum = StatelessAdd.Add(2, 3);

        Assert.Equal(5, sum);
        string json = SpecGateRuntime.GetTracesJson();
        Assert.Contains("{\"kind\":\"Run\",\"operation\":\"add\"}", json);
        Assert.Contains("{\"kind\":\"Event\",\"name\":\"add.a\",\"value\":2}", json);
        Assert.Contains("{\"kind\":\"Event\",\"name\":\"add.b\",\"value\":3}", json);
    }

    /// <summary>
    /// A woven <c>[SpecEvent]</c> auto-property setter must call
    /// <see cref="SpecGateRuntime.EmitMember"/> after assigning, so mutations are
    /// captured as state events.
    /// </summary>
    [Fact]
    public void SpecEvent_PropertySetterEmitsMember()
    {
        SpecGateRuntime.Reset();
        var account = new Account();
        SpecGateRuntime.RegisterObject(account, null);

        account.Balance = 42;

        string json = SpecGateRuntime.GetTracesJson();
        Assert.Contains("{\"kind\":\"Event\",\"name\":\"balance\",\"value\":42}", json);
    }

    /// <summary>
    /// A woven <c>[SpecMock]</c> field call must be redirected to the runtime's
    /// mock table: the real dependency is never invoked (it throws), the request
    /// is traced, and an unmatched call returns the spec default.
    /// </summary>
    [Fact]
    public void SpecMock_FieldCallRedirectsToRuntime()
    {
        SpecGateRuntime.Reset();
        var service = UserService.Make();
        SpecGateRuntime.RegisterObject(service, null);

        string result = service.GetUser("u-1");

        Assert.Equal(string.Empty, result);
        string json = SpecGateRuntime.GetTracesJson();
        Assert.Contains("{\"kind\":\"Event\",\"name\":\"db.request\",\"value\":\"u-1\"}", json);
    }

    /// <summary>
    /// Every <c>[SpecMock]</c> call in an operation must be redirected, not just
    /// the first: once the initial call resolves from the mock table, execution
    /// continues to a second mocked call which must also be intercepted rather
    /// than reaching the real (throwing) dependency.
    /// </summary>
    [Fact]
    public void SpecMock_MultipleFieldCallsAllRedirect()
    {
        SpecGateRuntime.Reset();
        SpecGateRuntime.SetMock("db", new Dictionary<string, string>
        {
            ["a-1"] = "Ann",
            ["b-2"] = "Bob",
        });
        var service = Conformance.Mocks.MockMultiResponse.UserService.Make();
        SpecGateRuntime.RegisterObject(service, null);

        string result = service.GetTwoUsers("a-1", "b-2");

        Assert.Equal("Ann and Bob", result);
    }
}
