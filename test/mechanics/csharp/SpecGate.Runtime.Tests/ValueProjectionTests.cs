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
    public void InheritedRecordsAndUnionPayloadsCaptureTheSameEffectiveMembers()
    {
        var value = new InheritedRecord { Changed = "new", Nested = new Address { City = "Oslo" } };
        using CaptureSession session = CaptureScenario.Begin(Component);
        Assert.Same(value, Declarations.Inheritance(value, value,
            new InheritedSubset { Selected = 7, Extra = 99 },
            new InheritedAll { Selected = 8, Extra = 9, Unhidden = 10 }));
        OperationSpan span = Assert.Single(session.Finish().Operations);
        Value expected = Map(
            ("child_title", Value.String("child")),
            ("nested", Value.Tagged("Some", Map(("city", Value.String("Oslo"))))),
            ("changed", Value.Tagged("Some", Value.String("new"))),
            ("middle", Value.Integer(5)),
            ("last", Value.Integer(6)));
        Assert.Equal(expected, Named(span, "value"));
        Assert.Equal(expected, span.Result());
        Assert.Equal(Value.Tagged(nameof(InheritedRecord), expected), Named(span, "variant"));
        Assert.Equal(["Extra", "Selected", "Unhidden", "selected", "value", "variant"], span.Inputs.Keys);
        Assert.Equal(Value.Integer(7), Named(span, "selected"));
        Assert.Equal(Value.Integer(8), Named(span, "Selected"));
        Assert.Equal(Value.Integer(9), Named(span, "Extra"));
        Assert.Equal(Value.Integer(10), Named(span, "Unhidden"));
        Assert.Equal(["Selected", "Unhidden", "Extra"],
            SemanticType.MembersOf(typeof(InheritedAll)).Select(member => member.Name));
        Assert.Equal(
            ["child_title", "nested", "changed", "middle", "last"],
            SemanticType.MembersOf(typeof(InheritedRecord)).Select(member => member.Name));
        Assert.Same(SemanticType.MembersOf(typeof(InheritedRecord)), SemanticType.MembersOf(typeof(InheritedRecord)));
    }

    [Fact]
    public void DistinctInheritedMembersWithTheSameSemanticNameRejectCapture()
    {
        Assert.Equal(CaptureRejection.AmbiguousDeclaration,
            Reject(nameof(Declarations.InheritedCollision), [new InheritedCollision()]).Rejection);
    }

    [Fact]
    public void InheritedNullableAbsenceUsesTheMembersDeclaringContext()
    {
        var value = new NullableContextRecord { Maybe = null, Required = "required", Local = "local" };
        OperationSpan span = Capture(nameof(Declarations.NullableContext), [value], value);
        Value expected = Map(
            ("Maybe", Value.Tagged("None", Value.Unit)),
            ("Required", Value.String("required")),
            ("Local", Value.String("local")));
        Assert.Equal(expected, Named(span, "value"));
        Assert.Equal(expected, span.Result());

        value.Required = null!;
        FaultCompletion fault = Unrecordable(nameof(Declarations.NullableContext), [value], value);
        Assert.Contains("a null was observed where", fault.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void AConcreteRecordSlotProjectsOnlyItsDeclaredSurfaceForADerivedInstance()
    {
        var value = new NullableContextRecord { Maybe = null, Required = "required", Local = "not recorded" };
        OperationSpan span = Capture(nameof(Declarations.DeclaredContext), [value], value);
        Value expected = Map(
            ("Maybe", Value.Tagged("None", Value.Unit)),
            ("Required", Value.String("required")));
        Assert.Equal(expected, Named(span, "value"));
        Assert.Equal(expected, span.Result());
    }

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

    [Fact]
    public void LookupsPreserveKeysAndMultiplicityIncludingEmptyGroups()
    {
        ILookup<string, int> lookup = new[] { ("b", 2), ("a", 1), ("b", 2) }
            .ToLookup(item => item.Item1, item => item.Item2);
        Assert.Equal(Map(
            ("a", Value.List([Value.Integer(1)])),
            ("b", Value.List([Value.Integer(2), Value.Integer(2)]))),
            Single(nameof(Declarations.Lookup), [lookup]));
        Assert.Equal(Value.Map([]),
            Single(nameof(Declarations.Lookup), [Array.Empty<int>().ToLookup(_ => "empty")]));
        ExplicitGrouping<int, int> group = new(7, []);
        ExplicitLookup<int, int> custom = new(group);
        Assert.Equal(Value.List([Map(("key", Value.Integer(7)), ("value", Value.List([])))]),
            Single(nameof(Declarations.NumberLookup), [custom]));
        Assert.Equal(1, group.KeyReads);
        Assert.Equal(1, group.Enumerations);
        Assert.Equal(1, custom.Enumerations);
    }

    [Fact]
    public void KeyedResultsUseTheSameProjectionAsInputs()
    {
        ILookup<string, int> lookup = new[] { ("b", 2), ("a", 1), ("b", 2) }
            .ToLookup(item => item.Item1, item => item.Item2);
        Assert.Equal(Single(nameof(Declarations.Lookup), [lookup]),
            Capture(nameof(Declarations.LookupResult), [], lookup).Result());
        Assert.Equal(Single(nameof(Declarations.Lookup), [lookup]),
            Capture(nameof(Declarations.ConcreteLookupResult), [], lookup).Result());
        InheritedGrouping<string, int> group = new("key", [2, 1, 2]);
        Assert.Equal(Single(nameof(Declarations.Grouping), [group]),
            Capture(nameof(Declarations.GroupingResult), [], group).Result());
    }

    [Fact]
    public void GroupingIsARecordAndUsesAFreshGenericEnumeratorOnce()
    {
        InheritedGrouping<string, int> group = new("b", [2, 1, 2]);
        using IEnumerator<int> caller = ((IEnumerable<int>)group).GetEnumerator();
        Assert.True(caller.MoveNext());
        Value expected = Map(("key", Value.String("b")),
            ("items", Value.List([Value.Integer(2), Value.Integer(1), Value.Integer(2)])));
        Assert.Equal(expected, Single(nameof(Declarations.InheritedGrouping), [group]));
        Assert.Equal(1, group.KeyReads);
        Assert.Equal(2, group.Enumerations);
        Assert.True(caller.MoveNext());
        Assert.Equal(1, caller.Current);
        Assert.Equal(expected, Single(nameof(Declarations.Grouping), [group]));
        Assert.Equal(2, group.KeyReads);
        Assert.Equal(3, group.Enumerations);
    }

    [Fact]
    public void KeyedContractsPreserveOptionalKeysAndValuesWithoutInference()
    {
        ExplicitGrouping<string?, string?> absent = new(null, [null, "one", null]);
        ExplicitGrouping<string?, string?> present = new("a", []);
        ExplicitLookup<string?, string?> lookup = new(absent, present);
        Value none = Value.Tagged("None", Value.Unit);
        Value items = Value.List([none, Value.Tagged("Some", Value.String("one")), none]);
        Assert.Equal(Value.List([
            Map(("key", none), ("value", items)),
            Map(("key", Value.Tagged("Some", Value.String("a"))), ("value", Value.List([]))),
        ]), Single(nameof(Declarations.OptionalLookup), [lookup]));
        Assert.Equal(Map(("key", none), ("items", items)),
            Single(nameof(Declarations.OptionalGrouping), [absent]));
        Assert.Equal(Value.List([
            Map(("key", none), ("value", items)),
            Map(("key", Value.Tagged("Some", Value.String("a"))), ("value", Value.List([]))),
        ]), Single(nameof(Declarations.ExplicitOptionalLookup), [lookup]));
        Assert.Equal(Value.List([Map(("key", none), ("items", Value.List([Value.Integer(3)])))]),
            Single(nameof(Declarations.NestedGrouping),
                [new NestedGrouping<string?> { new ExplicitGrouping<int, string?>(null, [3]) }]));
        Assert.Equal(Value.List([Map(("key", none), ("value", Value.List([Value.Integer(3)])))]),
            Single(nameof(Declarations.NullableNumberLookup),
                [new ExplicitLookup<int, int?>(new ExplicitGrouping<int, int?>(null, [3]))]));
    }

    [Fact]
    public void LookupCollisionsUseRecursiveCtscEqualityAndNeverMerge()
    {
        IEnumerable<int> first = [1, 2];
        IEnumerable<int> second = [2, 1];
        ExplicitLookup<int, IEnumerable<int>> lookup = new(
            new ExplicitGrouping<int, IEnumerable<int>>(first, [3]),
            new ExplicitGrouping<int, IEnumerable<int>>(second, [4]));
        Assert.Equal(CaptureRejection.UnrepresentableValue,
            Reject(nameof(Declarations.BagLookup), [lookup]).Rejection);

        IGrouping<string, int> firstGroup = new ExplicitGrouping<int, string>("k", [1, 2]);
        IGrouping<string, int> secondGroup = new ExplicitGrouping<int, string>("k", [2, 1]);
        Assert.Equal(CaptureRejection.UnrepresentableValue,
            Reject(nameof(Declarations.GroupingKeys), [new Dictionary<IGrouping<string, int>, int>
            {
                [firstGroup] = 3,
                [secondGroup] = 4,
            }]).Rejection);
    }

    [Theory]
    [InlineData(typeof(IConflictingGrouping))]
    [InlineData(typeof(IConflictingLookup))]
    public void RuntimeRejectsConflictingKeyedContracts(Type type)
    {
        SpecGateCaptureException error = Assert.Throws<SpecGateCaptureException>(
            () => SemanticType.Map(type, NullableFlagReader.Oblivious));
        Assert.Equal(CaptureRejection.AmbiguousDeclaration, error.Rejection);
    }

    [Theory]
    [InlineData(typeof(IRecursiveEnumerable))]
    [InlineData(typeof(IMutuallyRecursiveEnumerable))]
    public void RuntimeRejectsRecursiveEnumerableDeclarations(Type type)
    {
        SpecGateCaptureException error = Assert.Throws<SpecGateCaptureException>(
            () => SemanticType.Map(type, NullableFlagReader.Oblivious));
        Assert.Equal(CaptureRejection.UnsupportedType, error.Rejection);
        Assert.Contains("recursive collection declaration", error.Message);
        Assert.Equal(SemanticKind.List, SemanticType.Map(typeof(List<List<int>>), NullableFlagReader.Oblivious).Kind);
    }

    [Fact]
    public void RuntimeKeepsImmutableSortedSetsUnorderedAndUnique()
    {
        SemanticType type = SemanticType.Map(
            typeof(System.Collections.Immutable.ImmutableSortedSet<int>), NullableFlagReader.Oblivious);
        Assert.Equal(SemanticKind.Set, type.Kind);
    }

    private interface IRecursiveEnumerable : IEnumerable<IRecursiveEnumerable>;
    private interface IMutuallyRecursiveEnumerable : IEnumerable<IRecursiveLink>;
    private interface IRecursiveLink : IEnumerable<IMutuallyRecursiveEnumerable>;

    private interface IConflictingGrouping : IGrouping<int, int>, IGrouping<string, int>;
    private interface IConflictingLookup : ILookup<int, int>, ILookup<string, int>;

    private class ExplicitGrouping<TItem, TKey>(TKey key, TItem[] items) : IGrouping<TKey, TItem>
    {
        public int KeyReads { get; private set; }
        public int Enumerations { get; private set; }
        TKey IGrouping<TKey, TItem>.Key
        {
            get
            {
                KeyReads++;
                return key;
            }
        }
        IEnumerator<TItem> IEnumerable<TItem>.GetEnumerator()
        {
            Enumerations++;
            return ((IEnumerable<TItem>)items).GetEnumerator();
        }
        System.Collections.IEnumerator System.Collections.IEnumerable.GetEnumerator() =>
            throw new InvalidOperationException("Use the generic grouping contract.");
    }

    private sealed class InheritedGrouping<TKey, TItem>(TKey key, TItem[] items)
        : ExplicitGrouping<TItem, TKey>(key, items);

    private sealed class NestedGrouping<T> : List<IGrouping<T, int>>;

    private sealed class ExplicitLookup<TItem, TKey>(params IGrouping<TKey, TItem>[] groups) : ILookup<TKey, TItem>
    {
        public int Enumerations { get; private set; }
        public int Count => throw new InvalidOperationException("Do not inspect Count.");
        public IEnumerable<TItem> this[TKey key] => throw new InvalidOperationException("Do not index groups.");
        public bool Contains(TKey key) => throw new InvalidOperationException("Do not probe keys.");
        IEnumerator<IGrouping<TKey, TItem>> IEnumerable<IGrouping<TKey, TItem>>.GetEnumerator()
        {
            Enumerations++;
            return ((IEnumerable<IGrouping<TKey, TItem>>)groups).GetEnumerator();
        }
        System.Collections.IEnumerator System.Collections.IEnumerable.GetEnumerator() =>
            throw new InvalidOperationException("Use the generic lookup contract.");
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
        [SpecOperation("inheritance", Spec = Component)]
        internal static InheritedRecord Inheritance(
            InheritedRecord value, InheritedUnion variant, InheritedSubset subset, InheritedAll all)
        {
            OperationScope scope = SpecGateRuntime.EnterOperation(Of(nameof(Inheritance)), "inheritance", Component);
            SpecGateRuntime.RecordInputs(scope, [value, variant, subset, all]);
            SpecGateRuntime.ExitOk(scope, value);
            return value;
        }

        [SpecOperation("inherited_collision", Spec = Component)]
        internal static void InheritedCollision(InheritedCollision value) => _ = value;

        [SpecOperation("nullable_context", Spec = Component)]
        internal static NullableContextRecord NullableContext(NullableContextRecord value) => value;

        [SpecOperation("declared_context", Spec = Component)]
        internal static NullableContextBase DeclaredContext(NullableContextBase value) => value;

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
        internal static void Lookup(ILookup<string, int> values) => _ = values;
        internal static ILookup<string, int> LookupResult() => throw new NotSupportedException();
        internal static Lookup<string, int> ConcreteLookupResult() => throw new NotSupportedException();
        internal static IGrouping<string, int> GroupingResult() => throw new NotSupportedException();
        internal static void NumberLookup(ILookup<int, int> values) => _ = values;
        internal static void Grouping(IGrouping<string, int> values) => _ = values;
        internal static void InheritedGrouping(InheritedGrouping<string, int> values) => _ = values;
        internal static void OptionalLookup(ILookup<string?, string?> values) => _ = values;
        internal static void OptionalGrouping(IGrouping<string?, string?> values) => _ = values;
        internal static void ExplicitOptionalLookup(ExplicitLookup<string?, string?> values) => _ = values;
        internal static void NestedGrouping(NestedGrouping<string?> values) => _ = values;
        internal static void NullableNumberLookup(ILookup<int?, int> values) => _ = values;
        internal static void BagLookup(ILookup<IEnumerable<int>, int> values) => _ = values;
        internal static void GroupingKeys(Dictionary<IGrouping<string, int>, int> values) => _ = values;

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

internal class InheritedBase
{
    [SpecEvent("base_title")]
    public virtual string Title => "base";
    [SpecEvent("nested")]
    public Address? Nested { get; set; }
    [SpecEvent]
    public virtual int Removed => 1;
    [SpecEvent]
    public int Changed = 1;
    public int Unannotated => Private;
    [SpecEvent]
    public static int Static => 3;
    [SpecEvent]
    private int Private => Changed + 1;
    [SpecEvent]
    public int WriteOnly { set => Changed = value; }
    [SpecEvent]
    public int PrivateGetter { private get; set; }
}

[SpecEvent]
internal abstract class InheritedUnion : InheritedBase
{
    [SpecEvent("middle")]
    public int Middle = 5;
}

[SpecEvent]
internal sealed class InheritedRecord : InheritedUnion
{
    [SpecEvent("child_title")]
    public override string Title => "child";
    public override int Removed => 2;
    [SpecEvent("changed")]
    public new string? Changed { get; set; }
    [SpecEvent("last")]
    public int Last { get; set; } = 6;
}

internal class InheritedSelection
{
    [SpecEvent("selected")]
    public int Selected { get; set; }
    public int Unhidden { get; set; }
}

[SpecEvent(Setup = true)]
internal sealed class InheritedSubset : InheritedSelection
{
    public int Extra { get; set; }
}

[SpecEvent(Setup = true)]
internal sealed class InheritedAll : InheritedSelection
{
    public new int Selected { get; set; }
    public int Extra { get; set; }
}

[SpecEvent]
internal sealed class InheritedCollision : InheritedSelection
{
    [SpecEvent("selected")]
    public int Different { get; set; }
}

[SpecEvent]
internal class NullableContextBase
{
    [SpecEvent]
    public string? Maybe { get; set; }
    [SpecEvent]
    public string Required { get; set; } = "";
}

#nullable disable
[SpecEvent]
internal sealed class NullableContextRecord : NullableContextBase
{
    [SpecEvent]
    public string Local { get; set; } = "";
}
#nullable restore

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
