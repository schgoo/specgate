using Xunit;
using MultiField = SpecGate.CtscFixtures.Conformance.Stateful.MultiFieldCapture;
using MultiMutation = SpecGate.CtscFixtures.Conformance.Stateful.MultiMutation;
using MultiSetup = SpecGate.CtscFixtures.Conformance.Stateful.MultiSetup;
using MultiStep = SpecGate.CtscFixtures.Conformance.Stateful.MultiStep;
using Nested = SpecGate.CtscFixtures.Conformance.Stateful.NestedOperations;
using Readonly = SpecGate.CtscFixtures.Conformance.Stateful.ReadonlyOperation;
using SetupWithParams = SpecGate.CtscFixtures.Conformance.Stateful.SetupWithParams;
using Shared = SpecGate.CtscFixtures.Conformance.Stateful.SharedSetup;
using StateMachine = SpecGate.CtscFixtures.Conformance.Stateful.StatemachineCounter;
using VoidOperation = SpecGate.CtscFixtures.Conformance.Stateful.VoidOperation;

namespace SpecGate.CtscFixtures.Tests;

public sealed class StatefulBehaviorTests
{
    [Fact]
    public void MultiFieldAndRepeatedMutationPreserveIntermediateSemantics()
    {
        MultiField.Account account = MultiField.Account.Make();
        account.Withdraw(25);
        Assert.Equal(75, account.Balance);
        Assert.Equal(1, account.TransactionCount);
        Assert.Equal(75, MultiField.Account.ObserveAccount(account).Balance);

        MultiMutation.Counter counter = MultiMutation.Counter.Make();
        counter.IncrementTwice();
        Assert.Equal(2, counter.Count);
        Assert.Equal(1, MultiMutation.Counter.ObserveCount("after_first", 1));
        Assert.Equal(2, MultiMutation.Counter.ObserveCount("after_second", 2));
    }

    [Fact]
    public void MultipleAndSharedSetupsFillDistinctParameters()
    {
        MultiSetup.Account source = MultiSetup.Transfer.MakeSource();
        MultiSetup.Account target = MultiSetup.Transfer.MakeTarget();
        MultiSetup.Transfer.Execute(source, target, 30);
        Assert.Equal(70, source.Balance);
        Assert.Equal(30, target.Balance);

        Shared.BoxVal left = Shared.Combine.MakeBox();
        Shared.BoxVal right = Shared.Combine.MakeBox();
        Assert.Equal(4, Shared.Combine.Two(left, right));
        Assert.Equal(
            3,
            Shared.Combine.Three(
                Shared.Combine.MakeUnit(),
                Shared.Combine.MakeUnit(),
                Shared.Combine.MakeUnit()));
    }

    [Fact]
    public void MultiStepAndNestedOperationsShareReceiverState()
    {
        MultiStep.Counter counter = MultiStep.Counter.Make();
        counter.Increment();
        counter.Increment();
        counter.Decrement();
        Assert.Equal(1, counter.Count);

        Nested.Account account = Nested.Account.Make();
        account.Transfer(20);
        Assert.Equal(100, account.Balance);
        account.Withdraw(15);
        Assert.Equal(85, account.Balance);
        account.Deposit(5);
        Assert.Equal(90, account.Balance);
    }

    [Fact]
    public void ParameterizedReadonlyStateMachineAndVoidSurfacesRemainBehavioral()
    {
        SetupWithParams.Counter parameterized = SetupWithParams.Counter.Make(9);
        parameterized.Increment();
        Assert.Equal(10, parameterized.Count);

        Readonly.Counter readOnly = Readonly.Counter.Make();
        Assert.Equal(42, readOnly.GetCount());
        Assert.Equal(42, readOnly.Count);

        StateMachine.Counter stateMachine = StateMachine.Counter.Make();
        stateMachine.Increment();
        Assert.Equal(1, stateMachine.Count);
        Assert.Equal(1, StateMachine.Counter.ObserveState(stateMachine).Count);

        VoidOperation.Logger logger = VoidOperation.Logger.Make();
        logger.Log("captured");
        Assert.Equal(1, logger.Count);
    }
}
