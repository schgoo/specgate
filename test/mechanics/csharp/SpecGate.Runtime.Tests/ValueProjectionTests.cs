using System.Reflection;
using SpecGate.Annotations;
using SpecGate.TestSupport;
using Xunit;

namespace SpecGate.Runtime.Tests;

/// <summary>
/// Covers what capture records for each CTSC value family, driven through the
/// same entry points woven code calls.
/// </summary>
/// <remarks>
/// The expected shapes are the ones the Rust runtime produces, because a C#
/// reference capture and a Rust candidate replay are compared against each
/// other: a record is a map of its members, a tagged union is a single-entry map
/// from variant name to payload, a payload-free variant's payload is the empty
/// map, an optional is <c>Some</c> or <c>None</c>, and a result is <c>Ok</c> or
/// <c>Err</c>.
/// </remarks>
[Collection(nameof(CaptureSessionSerialization))]
public sealed class ValueProjectionTests
{
    private const string Component = "fixture.projection";

    [Fact]
    public void ScalarsProjectOntoTheirValueFamilies()
    {
        OperationSpan span = Capture(
            nameof(Declarations.Scalars),
            [true, (sbyte)-1, (short)2, 3, 4L, 1.5f, 2.5, "hello"]);

        // Inputs are recorded sorted by name, not in parameter order, because
        // Rust capture stores them in a BTreeMap (capture.rs:67-85).
        Assert.Equal(
            ["count", "flag", "ratio", "single", "small", "text", "tiny", "total"],
            span.Inputs.Keys);
        Assert.Equal(
            [
                Value.Integer(3),
                Value.Bool(true),
                Value.Float(2.5),
                Value.Float(1.5),
                Value.Integer(2),
                Value.String("hello"),
                Value.Integer(-1),
                Value.Integer(4),
            ],
            span.Inputs.Values);
    }

    [Fact]
    public void TheReturnValueIsRecordedAsTheResultCompletion()
    {
        OperationSpan span = Capture(nameof(Declarations.Total), [new List<int> { 1, 2 }], 3);

        Assert.Equal(Value.Integer(3), span.Result());
    }

    [Fact]
    public void UnsignedScalarsPreserveTheirFullRangeInInputsAndResults()
    {
        foreach (uint value in new uint[] { 0, int.MaxValue, 2_147_483_648, uint.MaxValue })
        {
            OperationSpan span = Capture(nameof(Declarations.Unsigned32), [value], value);
            Assert.Equal(Value.Integer(value), Assert.Single(span.Inputs).Value);
            Assert.Equal(Value.Integer(value), span.Result());
        }

        foreach (ulong value in new ulong[] { 0, long.MaxValue, 9_223_372_036_854_775_808, ulong.MaxValue })
        {
            OperationSpan span = Capture(nameof(Declarations.Unsigned64), [value], value);
            Assert.Equal(Value.Unsigned(value), Assert.Single(span.Inputs).Value);
            Assert.Equal(Value.Unsigned(value), span.Result());
        }
    }

    [Fact]
    public void NestedUnsignedValuesKeepTheirDeclaredProjection()
    {
        OperationSpan span = Capture(
            nameof(Declarations.NestedUnsigned),
            [uint.MaxValue, new List<ulong?> { ulong.MaxValue, null }]);

        Assert.Equal(Value.Tagged("Some", Value.Integer(uint.MaxValue)), Named(span, "count"));
        Assert.Equal(
            Value.List(
            [
                Value.Tagged("Some", Value.Unsigned(ulong.MaxValue)),
                Value.Tagged("None", Value.Unit),
            ]),
            Named(span, "totals"));
        Assert.Equal(
            Value.Tagged("None", Value.Unit),
            Named(Capture(nameof(Declarations.NestedUnsigned), [null, new List<ulong?>()]), "count"));
    }

    [Fact]
    public void AVoidOperationRecordsNoResult()
    {
        OperationSpan span = Capture(nameof(Declarations.Scalars), [true, (sbyte)0, (short)0, 0, 0L, 0f, 0d, ""]);

        Assert.Null(span.OperationCompletion);
    }

    [Fact]
    public void AbsentOptionalsProjectAsNone()
    {
        OperationSpan span = Capture(
            nameof(Declarations.Optionals),
            [null, null, Option<int>.None(), null]);

        Value none = Value.Tagged("None", Value.Unit);
        Assert.Equal([none, none, none, none], span.Inputs.Values);
    }

    [Fact]
    public void PresentOptionalsProjectAsSome()
    {
        OperationSpan span = Capture(
            nameof(Declarations.Optionals),
            [7, "text", Option<int>.Some(9), new Address { City = "Seattle" }]);

        Assert.Equal(
            [
                Value.Tagged("Some", Value.Integer(7)),
                Value.Tagged("Some", Value.Integer(9)),
                Value.Tagged("Some", Map(("city", Value.String("Seattle")))),
                Value.Tagged("Some", Value.String("text")),
            ],
            span.Inputs.Values);
    }

    [Fact]
    public void ResultsProjectAsOkOrErr()
    {
        Assert.Equal(
            Value.Tagged("Ok", Value.Integer(4)),
            Single(nameof(Declarations.Outcome), [Result<int, string>.Ok(4)]));
        Assert.Equal(
            Value.Tagged("Err", Value.String("boom")),
            Single(nameof(Declarations.Outcome), [Result<int, string>.Err("boom")]));
    }

    [Fact]
    public void CollectionsProjectOntoListSetAndMap()
    {
        OperationSpan span = Capture(
            nameof(Declarations.Collections),
            [
                new List<int> { 3, 1 },
                new SortedSet<string> { "b", "a" },
                new SortedDictionary<string, int> { ["b"] = 2, ["a"] = 1 },
                new List<SortedDictionary<string, string>> { new() { ["k"] = "v" } },
            ]);

        Assert.Equal(
            Map(("a", Value.Integer(1)), ("b", Value.Integer(2))),
            Named(span, "counts"));
        Assert.Equal(
            Value.List([Value.Integer(3), Value.Integer(1)]),
            Named(span, "numbers"));
        Assert.Equal(
            Value.List([Map(("k", Value.String("v")))]),
            Named(span, "rows"));
        Assert.Equal(
            Value.Set([Value.String("a"), Value.String("b")]),
            Named(span, "tags"));
    }

    [Fact]
    public void ARecordProjectsAsAMapOfItsAnnotatedMembers()
    {
        Value person = Single(
            nameof(Declarations.Structured),
            [new Person { Name = "Ada", Home = new Address { City = "London" } }]);

        Assert.Equal(
            Map(
                ("name", Value.String("Ada")),
                ("home", Map(("city", Value.String("London"))))),
            person);
    }

    [Fact]
    public void AnUnannotatedMemberIsNotRecorded()
    {
        Value address = Single(
            nameof(Declarations.Nested),
            [new Address { City = "Oslo", Note = "ignored" }]);

        Assert.Equal(Map(("city", Value.String("Oslo"))), address);
    }

    [Fact]
    public void AnOrdinaryEventDoesNotProjectWriteOnlyProperties()
    {
        Value address = Single(
            nameof(Declarations.Nested),
            [new Address { City = "Oslo", WriteOnly = "ignored" }]);

        Assert.Equal(Map(("city", Value.String("Oslo"))), address);
    }

    [Fact]
    public void PropertySetupOptInDoesNotFlattenEnumsOrAbstractUnions()
    {
        Assert.Equal(
            Value.Tagged("Medium", Value.Unit),
            Named(Capture(nameof(Declarations.Ranked), [Level.Medium]), "level"));
        Assert.Equal(
            Value.Tagged("Circle", Map(("radius", Value.Float(2.5)))),
            Named(Capture(nameof(Declarations.Variants), [new Circle { Radius = 2.5 }]), "shape"));
    }

    [Fact]
    public void NestedPropertySetupsRecordOnlyFlatSemanticLeaves()
    {
        var request = new PropertyRequest
        {
            Group = new PropertyGroup { Leaf = new PropertyLeaf { Selected = 7 } },
        };
        OperationSpan span = Capture(nameof(Declarations.PropertyInputs), [request]);
        Assert.Equal(["limit"], span.Inputs.Keys);
        request.Group.Leaf.Selected = 9;
        Assert.Equal(Value.Integer(7), Named(span, "limit"));
    }

    [Fact]
    public void PropertySetupExpansionStopsAtContainers()
    {
        OperationSpan span = Capture(
            nameof(Declarations.PropertyList),
            [new List<PropertyLeaf> { new() { Selected = 7 } }]);
        Assert.Equal(Value.List([Map(("limit", Value.Integer(7)))]), Named(span, "values"));
    }

    [Fact]
    public void NestedPropertySetupCollisionsAndCyclesAreExplicitRejections()
    {
        SpecGateCaptureException collision = Reject(
            nameof(Declarations.CollidingProperties), [new PropertyRequest(), 3]);
        Assert.Equal(CaptureRejection.AmbiguousDeclaration, collision.Rejection);
        Assert.Contains("limit", collision.Message, StringComparison.Ordinal);
        SpecGateCaptureException cycle = Reject(
            nameof(Declarations.CyclicProperties), [new RecursiveProperty()]);
        Assert.Equal(CaptureRejection.AmbiguousDeclaration, cycle.Rejection);
        Assert.Contains("recursive property setup", cycle.Message, StringComparison.Ordinal);
        Assert.Equal(
            CaptureRejection.TypeMismatch,
            Reject(nameof(Declarations.PropertyInputs), [new object()]).Rejection);
    }

    [Fact]
    public void ANullPropertySetupOrNestedMemberIsReportedNotThrown()
    {
        FaultCompletion top = Unrecordable(nameof(Declarations.PropertyInputs), [null]);
        Assert.Contains("parameter 'value'", top.Message, StringComparison.Ordinal);
        Assert.Contains("requires a non-null", top.Message, StringComparison.Ordinal);

        FaultCompletion nested = Unrecordable(
            nameof(Declarations.PropertyInputs), [new PropertyRequest { Group = null! }]);
        Assert.Contains("parameter 'value'", nested.Message, StringComparison.Ordinal);
        Assert.Contains("PropertyGroup", nested.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void AVariantProjectsAsItsNameOverItsOwnMembers()
    {
        Assert.Equal(
            Value.Tagged("Circle", Map(("radius", Value.Float(2.5)))),
            Single(nameof(Declarations.Variants), [new Circle { Radius = 2.5 }]));
    }

    [Fact]
    public void APayloadFreeVariantProjectsOverTheEmptyMap()
    {
        Assert.Equal(
            Value.Tagged("Origin", Value.Unit),
            Single(nameof(Declarations.Variants), [new Origin()]));
    }

    [Fact]
    public void AnErasedValueProjectsFromItsRuntimeType()
    {
        Assert.Equal(Value.Integer(5), Single(nameof(Declarations.Erased), [5]));
        Assert.Equal(Value.Integer(uint.MaxValue), Single(nameof(Declarations.Erased), [uint.MaxValue]));
        Assert.Equal(Value.Unsigned(ulong.MaxValue), Single(nameof(Declarations.Erased), [ulong.MaxValue]));
        Assert.Equal(Value.String("text"), Single(nameof(Declarations.Erased), ["text"]));
        Assert.Equal(
            Value.Tagged("Circle", Map(("radius", Value.Float(1.0)))),
            Single(nameof(Declarations.Erased), [new Circle { Radius = 1.0 }]));
    }

    [Fact]
    public void AnErasedCollectionIsRejected()
    {
        Assert.Equal(
            CaptureRejection.UnsupportedType,
            Reject(nameof(Declarations.Erased), [new List<int> { 1 }]).Rejection);
    }

    [Fact]
    public void ATypeWithNoCtscRepresentationIsRejected()
    {
        Assert.Equal(
            CaptureRejection.UnsupportedType,
            Reject(nameof(Declarations.Unrepresentable), [(byte)1]).Rejection);
    }

    [Fact]
    public void ANonStringMapKeyUsesEntryRecords()
    {
        Assert.Equal(
            Value.List([Map(("key", Value.Integer(1)), ("value", Value.String("one")))]),
            Single(
                nameof(Declarations.KeyedByNumber),
                [new SortedDictionary<int, string> { [1] = "one" }]));
    }

    [Fact]
    public void EnumerableContractsCaptureDuplicatesAndCustomImplementations()
    {
        Value expected = Value.List([Value.Integer(2), Value.Integer(1), Value.Integer(2)]);
        int[] values = [2, 1, 2];
        Assert.Equal(expected, Single(nameof(Declarations.Enumerable), [values]));
        Assert.Equal(expected, Single(nameof(Declarations.CustomEnumerable), [new Numbers()]));
        Assert.Equal(
            Value.List([Value.Tagged("None", Value.Unit), Value.Tagged("Some", Value.String("one"))]),
            Single(nameof(Declarations.ReorderedEnumerable), [new Reordered<int, string?>()]));
    }

    [Fact]
    public void OrderedMapsAndPriorityQueuesKeepTheirFullContents()
    {
        OrderedDictionary<string, int> map = new() { ["b"] = 2, ["a"] = 1 };
        Assert.Equal(
            Value.List([
                Map(("key", Value.String("b")), ("value", Value.Integer(2))),
                Map(("key", Value.String("a")), ("value", Value.Integer(1))),
            ]),
            Single(nameof(Declarations.OrderedMap), [map]));
        Assert.Equal(["b", "a"], map.Keys);

        PriorityQueue<string, int> queue = new();
        queue.Enqueue("later", 7);
        queue.Enqueue("first", 1);
        Value projected = Single(nameof(Declarations.PriorityQueue), [queue]);
        Assert.Equal(
            queue.UnorderedItems.Select(item => Value.List([Value.String(item.Element), Value.Integer(item.Priority)])),
            projected.AsList());
        Assert.Equal(2, queue.Count);
        Assert.Equal("first", queue.Peek());
    }

    [Fact]
    public void ReadOnlyDictionaryContractsDoNotRequireNonGenericDictionary()
    {
        IReadOnlyDictionary<string, int> map = new ReadOnlyNumbers();
        Assert.Equal(Map(("a", Value.Integer(1))), Single(nameof(Declarations.ReadOnlyMap), [map]));
    }

    [Fact]
    public void NestedGenericContractsPreserveElementNullabilityAndProjectionMetadata()
    {
        Assert.Equal(
            Value.List([Value.List([Value.Tagged("None", Value.Unit)])]),
            Single(nameof(Declarations.NestedEnumerable), [new Nested<string?>([null])]));
        PriorityQueue<string?, int> queue = new();
        queue.Enqueue(null, 3);
        Assert.Equal(
            Value.List([Value.List([Value.List([Value.Tagged("None", Value.Unit), Value.Integer(3)])])]),
            Single(nameof(Declarations.NestedPriority), [new NestedPriority<string?>(queue)]));
        DerivedPriority<string> derived = new();
        derived.Enqueue("item", 1);
        Assert.Equal(
            Value.List([Value.List([Value.String("item"), Value.Integer(1)])]),
            Single(nameof(Declarations.DerivedPriority), [derived]));
    }

    [Fact]
    public void ProjectionCollisionsInUniqueCollectionsAreRejectedRatherThanDropped()
    {
        HashSet<Address> values = [new() { City = "same" }, new() { City = "same" }];
        Assert.Equal(
            CaptureRejection.UnrepresentableValue,
            Reject(nameof(Declarations.UniqueAddresses), [values]).Rejection);
    }

    [Fact]
    public void UniquenessUsesRecursiveBagSemanticsForElementsAndKeys()
    {
        IEnumerable<int> first = [1, 2];
        IEnumerable<int> second = [2, 1];
        HashSet<IEnumerable<int>> values = [first, second];
        Dictionary<IEnumerable<int>, string> keys = new() { [first] = "one", [second] = "two" };
        Assert.Equal(CaptureRejection.UnrepresentableValue,
            Reject(nameof(Declarations.UniqueBags), [values]).Rejection);
        Assert.Equal(CaptureRejection.UnrepresentableValue,
            Reject(nameof(Declarations.BagKeys), [keys]).Rejection);
    }

    private sealed class Numbers : IEnumerable<int>
    {
        IEnumerator<int> IEnumerable<int>.GetEnumerator()
        {
            yield return 2;
            yield return 1;
            yield return 2;
        }
        System.Collections.IEnumerator System.Collections.IEnumerable.GetEnumerator() =>
            throw new InvalidOperationException("Capture must use the declared generic enumeration contract.");
    }

    private sealed class Nested<T>(List<T> values) : IEnumerable<List<T>>
    {
        public IEnumerator<List<T>> GetEnumerator()
        {
            yield return values;
        }
        System.Collections.IEnumerator System.Collections.IEnumerable.GetEnumerator() => GetEnumerator();
    }

    private sealed class NestedPriority<T>(PriorityQueue<T, int> queue) : IEnumerable<PriorityQueue<T, int>>
    {
        public IEnumerator<PriorityQueue<T, int>> GetEnumerator()
        {
            yield return queue;
        }
        System.Collections.IEnumerator System.Collections.IEnumerable.GetEnumerator() => GetEnumerator();
    }

    private sealed class DerivedPriority<T> : PriorityQueue<T, int>;

    private sealed class ReadOnlyNumbers : IReadOnlyDictionary<string, int>
    {
        public int this[string key] => key == "a" ? 1 : throw new KeyNotFoundException();
        public IEnumerable<string> Keys => ["a"];
        public IEnumerable<int> Values => [1];
        public int Count => 1;
        public bool ContainsKey(string key) => key == "a";
        public bool TryGetValue(string key, out int value)
        {
            value = key == "a" ? 1 : 0;
            return key == "a";
        }
        public IEnumerator<KeyValuePair<string, int>> GetEnumerator()
        {
            yield return new("a", 1);
        }
        System.Collections.IEnumerator System.Collections.IEnumerable.GetEnumerator() =>
            throw new InvalidOperationException("Capture must use the generic dictionary contract.");
    }

    private sealed class Reordered<TIgnored, TItem> : IEnumerable<TItem>
    {
        public IEnumerator<TItem> GetEnumerator()
        {
            yield return default!;
            yield return (TItem)(object)"one";
        }
        System.Collections.IEnumerator System.Collections.IEnumerable.GetEnumerator() => GetEnumerator();
    }

    /// <summary>
    /// A null where the declaration is not optional does not fail the test: the
    /// operation runs as written and its span carries an unrecordable-value
    /// fault naming the position, which capture reports when it drops the
    /// scenario.
    /// </summary>
    [Fact]
    public void ANullInANonNullableDeclarationIsReportedNotThrown()
    {
        FaultCompletion fault = Unrecordable(nameof(Declarations.Nested), [null]);

        Assert.Equal("target", fault.Observer);
        Assert.Equal(
            "parameter 'home' of 'fixture.projection::Nested': a null was observed where "
            + "'SpecGate.Runtime.Tests.Address' is required. Declare the parameter, member, or return "
            + "value nullable to record absence.",
            fault.Message);
    }

    [Fact]
    public void ANullReturnValueInANonNullableDeclarationIsReportedNotThrown()
    {
        FaultCompletion fault = Unrecordable(nameof(Declarations.Echo), ["text"], result: null);

        Assert.StartsWith("return value of", fault.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void AValueOfTheWrongTypeIsRejected()
    {
        Assert.Equal(
            CaptureRejection.TypeMismatch,
            Reject(nameof(Declarations.Total), ["not a list"]).Rejection);
    }

    [Fact]
    public void AnUnpairedSurrogateIsRejected()
    {
        Assert.Equal(
            CaptureRejection.UnrepresentableValue,
            Reject(nameof(Declarations.Erased), ["\ud800"]).Rejection);
    }

    [Fact]
    public void AnUnpairedSurrogateInAMapKeyIsRejected()
    {
        Assert.Equal(
            CaptureRejection.UnrepresentableValue,
            Reject(
                nameof(Declarations.Collections),
                [
                    new List<int>(),
                    new SortedSet<string>(),
                    new SortedDictionary<string, int> { ["\ud800"] = 1 },
                    new List<SortedDictionary<string, string>>(),
                ]).Rejection);
    }

    [Fact]
    public void AnExplicitlyEmptyInputNameIsPreserved()
    {
        OperationSpan span = Capture(nameof(Declarations.EmptyNames), [1]);

        // C# discovery publishes `inp?.Name ?? p.Name`, so an empty declared name
        // reaches the registry. Capture has to agree with it.
        Assert.Equal(string.Empty, Assert.Single(span.Inputs).Key);
    }

    [Fact]
    public void AnExplicitlyEmptyMemberNameIsPreserved()
    {
        Assert.Equal(
            Map((string.Empty, Value.Integer(7))),
            Single(nameof(Declarations.EmptyMemberName), [new Unnamed { Count = 7 }]));
    }

    [Fact]
    public void AnArgumentCountThatDisagreesWithTheDeclarationIsRejected()
    {
        Assert.Equal(
            CaptureRejection.TypeMismatch,
            Reject(nameof(Declarations.Nested), []).Rejection);
    }

    [Fact]
    public void NullabilityIsReadPositionallyInsideGenerics()
    {
        OperationSpan span = Capture(
            nameof(Declarations.NestedNullability),
            [
                new List<string?> { "a", null },
                new List<string> { "b" },
                Option<Address>.Some(new Address { City = "Kyoto" }),
                new SortedDictionary<string, int?> { ["k"] = null },
            ]);

        Assert.Equal(
            [
                Value.Tagged("Some", Map(("city", Value.String("Kyoto")))),
                Map(("k", Value.Tagged("None", Value.Unit))),
                Value.List([Value.Tagged("Some", Value.String("a")), Value.Tagged("None", Value.Unit)]),
                Value.List([Value.String("b")]),
            ],
            span.Inputs.Values);
    }

    [Fact]
    public void AnEnumProjectsAsItsMemberNameOverTheEmptyMap()
    {
        Assert.Equal(
            Value.Tagged("Medium", Value.Unit),
            Single(nameof(Declarations.Ranked), [Level.Medium]));
    }

    /// <summary>
    /// <see cref="Level"/> declares its members in an order its underlying
    /// values deliberately contradict, so a projection that read names by
    /// numeric order would report the wrong member here.
    /// </summary>
    [Fact]
    public void AValueReorderedEnumProjectsEachMemberUnderItsOwnName()
    {
        Assert.Equal(
            [
                Value.Tagged("Low", Value.Unit),
                Value.Tagged("Medium", Value.Unit),
                Value.Tagged("High", Value.Unit),
            ],
            new[] { Level.Low, Level.Medium, Level.High }
                .Select(level => Single(nameof(Declarations.Ranked), [level])));
    }

    [Fact]
    public void AnEnumWithANonIntUnderlyingTypeProjectsTheSameWay()
    {
        Assert.Equal(
            Value.Tagged("Second", Value.Unit),
            Single(nameof(Declarations.Widened), [Wide.Second]));
    }

    [Fact]
    public void AnErasedEnumProjectsFromItsRuntimeType()
    {
        Assert.Equal(
            Value.Tagged("High", Value.Unit),
            Single(nameof(Declarations.Erased), [Level.High]));
    }

    [Fact]
    public void AnUndeclaredEnumValueIsRejected()
    {
        Assert.Equal(
            CaptureRejection.UnrepresentableValue,
            Reject(nameof(Declarations.Ranked), [(Level)99]).Rejection);
    }

    /// <summary>
    /// An alias shares one underlying value with the member it aliases, and a
    /// CTSC variant carries no discriminant to tell them apart, so both record
    /// the one name the lookup deterministically returns.
    /// </summary>
    [Fact]
    public void AnAliasMemberRecordsTheNameItsDiscriminantResolvesTo()
    {
        Assert.Equal(
            Single(nameof(Declarations.AliasedRanked), [Aliased.Low]),
            Single(nameof(Declarations.AliasedRanked), [Aliased.Default]));
    }

    [Fact]
    public void AnEnumMemberHonoursItsDeclaredSemanticName()
    {
        Assert.Equal(
            Value.Tagged("renamed", Value.Unit),
            Single(nameof(Declarations.AliasedRanked), [Aliased.Renamed]));
    }

    /// <summary>
    /// A CTSC variant is identified by its name alone, so two members cannot
    /// rename onto one name even though their discriminants differ.
    /// </summary>
    [Fact]
    public void TwoEnumMembersRenamedOntoOneNameAreRejected()
    {
        Assert.Equal(
            CaptureRejection.AmbiguousDeclaration,
            Reject(nameof(Declarations.Collided), [Collided.First]).Rejection);
    }
    private static Value Named(OperationSpan span, string name) =>
        span.Inputs[name];

    private static Value Map(params (string Key, Value Value)[] entries) =>
        Value.Map(entries.Select(entry => new KeyValuePair<string, Value>(entry.Key, entry.Value)));

    private static Value Single(string declaration, object?[] arguments) =>
        Assert.Single(Capture(declaration, arguments).Inputs).Value;

    private static OperationSpan Capture(string declaration, object?[] arguments, object? result = null)
    {
        using CaptureSession session = CaptureScenario.Begin(Component);

        OperationScope scope = SpecGateRuntime.EnterOperation(
            Declarations.Of(declaration), declaration, Component);
        SpecGateRuntime.RecordInputs(scope, arguments);
        SpecGateRuntime.ExitOk(scope, result);

        return Assert.Single(session.Finish().Operations);
    }

    private static SpecGateCaptureException Reject(string declaration, object?[] arguments)
    {
        using CaptureSession session = CaptureScenario.Begin(Component);

        OperationScope scope = SpecGateRuntime.EnterOperation(
            Declarations.Of(declaration), declaration, Component);

        // A rejected RecordInputs abandons its own scope, so the caller does not
        // close it; the woven try region has not been entered yet.
        return Assert.Throws<SpecGateCaptureException>(
            () => SpecGateRuntime.RecordInputs(scope, arguments));
    }

    private static FaultCompletion Unrecordable(
        string declaration, object?[] arguments, object? result = null)
    {
        OperationSpan span = Capture(declaration, arguments, result);
        FaultCompletion fault = Assert.IsType<FaultCompletion>(span.OperationCompletion);
        Assert.Equal("specgate.unrecordable_value", fault.FaultType);
        return fault;
    }

    /// <summary>Declarations standing in for the woven methods under capture.</summary>
    private static class Declarations
    {
        internal static RuntimeMethodHandle Of(string name) =>
            typeof(Declarations)
                .GetMethod(name, BindingFlags.Static | BindingFlags.NonPublic)!
                .MethodHandle;

        internal static void Scalars(
            bool flag,
            sbyte tiny,
            short small,
            int count,
            long total,
            float single,
            double ratio,
            string text)
        {
            _ = (flag, tiny, small, count, total, single, ratio, text);
        }

        internal static int Total(List<int> numbers) => numbers.Sum();

        internal static uint Unsigned32(uint value) => value;

        internal static ulong Unsigned64(ulong value) => value;

        internal static void NestedUnsigned(uint? count, List<ulong?> totals) => _ = (count, totals);

        internal static void Optionals(
            int? count,
            string? text,
            Option<int> explicitCount,
            Address? home)
        {
            _ = (count, text, explicitCount, home);
        }

        internal static void Outcome(Result<int, string> outcome) => _ = outcome;

        internal static void Collections(
            List<int> numbers,
            SortedSet<string> tags,
            SortedDictionary<string, int> counts,
            List<SortedDictionary<string, string>> rows)
        {
            _ = (numbers, tags, counts, rows);
        }

        internal static void Structured(Person person) => _ = person;

        internal static void Nested(Address home) => _ = home;

        internal static string Echo(string text) => text;

        internal static void Variants(Shape shape) => _ = shape;

        internal static void Ranked(Level level) => _ = level;

        internal static void Widened(Wide value) => _ = value;

        internal static void AliasedRanked(Aliased value) => _ = value;

        internal static void Collided(Collided value) => _ = value;

        internal static void PropertyInputs(PropertyRequest value) => _ = value;

        internal static void PropertyList(List<PropertyLeaf> values) => _ = values;

        internal static void CollidingProperties(PropertyRequest value, int limit) => _ = (value, limit);

        internal static void CyclicProperties(RecursiveProperty value) => _ = value;

        internal static void Erased(object value) => _ = value;

        internal static void Unrepresentable(byte value) => _ = value;

        internal static void KeyedByNumber(SortedDictionary<int, string> counts) => _ = counts;

        internal static void Enumerable(IEnumerable<int> values) => _ = values;
        internal static void CustomEnumerable(Numbers values) => _ = values;
        internal static void ReorderedEnumerable(Reordered<int, string?> values) => _ = values;
        internal static void OrderedMap(OrderedDictionary<string, int> values) => _ = values;
        internal static void PriorityQueue(PriorityQueue<string, int> values) => _ = values;
        internal static void ReadOnlyMap(IReadOnlyDictionary<string, int> values) => _ = values;
        internal static void UniqueAddresses(HashSet<Address> values) => _ = values;
        internal static void NestedEnumerable(Nested<string?> values) => _ = values;
        internal static void NestedPriority(NestedPriority<string?> values) => _ = values;
        internal static void DerivedPriority(DerivedPriority<string> values) => _ = values;
        internal static void UniqueBags(HashSet<IEnumerable<int>> values) => _ = values;
        internal static void BagKeys(Dictionary<IEnumerable<int>, string> values) => _ = values;

        internal static void EmptyNames([SpecInput("")] int count) => _ = count;

        internal static void EmptyMemberName(Unnamed value) => _ = value;

        internal static void NestedNullability(
            List<string?> maybeTexts,
            List<string> texts,
            Option<Address> home,
            SortedDictionary<string, int?> maybeCounts)
        {
            _ = (maybeTexts, texts, home, maybeCounts);
        }
    }
}

[SpecEvent]
internal sealed class Address
{
    [SpecEvent("city")]
    public string City { get; init; } = string.Empty;

    public string Note { get; init; } = string.Empty;

    [SpecEvent]
    public string WriteOnly { init => Note = value; }
}

[SpecEvent(Setup = true)]
internal sealed class PropertyRequest
{
    public PropertyGroup Group { get; set; } = new();
}

[SpecEvent(Setup = true)]
internal sealed class PropertyGroup
{
    public PropertyLeaf Leaf { get; set; } = new();
    public EmptyProperty Empty { get; set; } = new();
}

[SpecEvent(Setup = true)]
internal sealed class PropertyLeaf
{
    [SpecEvent("limit")]
    public int Selected { get; set; }
    public string Omitted { get; set; } = "";
}

[SpecEvent(Setup = true)]
internal sealed class EmptyProperty;

[SpecEvent(Setup = true)]
internal sealed class RecursiveProperty
{
    public RecursiveProperty Child { get; set; } = null!;
}

[SpecEvent]
internal sealed class Person
{
    [SpecEvent("name")]
    public string Name { get; init; } = string.Empty;

    [SpecEvent("home")]
    public Address Home { get; init; } = new();
}

[SpecEvent(Setup = true)]
internal abstract class Shape;

[SpecEvent]
internal sealed class Circle : Shape
{
    [SpecEvent("radius")]
    public double Radius { get; init; }
}

[SpecEvent]
internal sealed class Origin : Shape;

/// <summary>A record whose only member declares an empty semantic name.</summary>
[SpecEvent]
internal sealed class Unnamed
{
    [SpecEvent("")]
    public int Count { get; init; }
}

/// <summary>An enum whose declaration order contradicts its numeric order.</summary>
[SpecEvent(Setup = true)]
internal enum Level
{
    Low = 30,
    Medium = 10,
    High = 20,
}

/// <summary>An enum whose discriminant is not an <see langword="int"/>.</summary>
[SpecEvent]
internal enum Wide : long
{
    First,
    Second,
}

/// <summary>An enum whose two members rename onto one variant name.</summary>
[SpecEvent]
internal enum Collided
{
    [SpecEvent("same")]
    First = 1,

    [SpecEvent("same")]
    Second = 2,
}
/// <summary>An enum declaring an alias member and a renamed member.</summary>
[SpecEvent]
internal enum Aliased
{
    Low = 1,
    Default = Low,

    [SpecEvent("renamed")]
    Renamed = 2,
}
