using System.Reflection;
using SpecGate.Annotations;
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
        RecordedSpan span = Capture(
            nameof(Declarations.Scalars),
            [true, (sbyte)-1, (short)2, 3, 4L, 1.5f, 2.5, "hello"]);

        // Inputs are recorded sorted by name, not in parameter order, because
        // Rust capture stores them in a BTreeMap (capture.rs:67-85).
        Assert.Equal(
            ["count", "flag", "ratio", "single", "small", "text", "tiny", "total"],
            span.Inputs.Select(input => input.Name));
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
            span.Inputs.Select(input => input.Value));
    }

    [Fact]
    public void TheReturnValueIsRecordedUnderReturn()
    {
        RecordedSpan span = Capture(nameof(Declarations.Total), [new List<int> { 1, 2 }], 3);

        Assert.Equal("return", span.Result?.Name);
        Assert.Equal(Value.Integer(3), span.Result?.Value);
    }

    [Fact]
    public void AVoidOperationRecordsNoResult()
    {
        RecordedSpan span = Capture(nameof(Declarations.Scalars), [true, (sbyte)0, (short)0, 0, 0L, 0f, 0d, ""]);

        Assert.Null(span.Result);
    }

    [Fact]
    public void AbsentOptionalsProjectAsNone()
    {
        RecordedSpan span = Capture(
            nameof(Declarations.Optionals),
            [null, null, Option<int>.None(), null]);

        Value none = Value.Tagged("None", Value.Unit);
        Assert.Equal([none, none, none, none], span.Inputs.Select(input => input.Value));
    }

    [Fact]
    public void PresentOptionalsProjectAsSome()
    {
        RecordedSpan span = Capture(
            nameof(Declarations.Optionals),
            [7, "text", Option<int>.Some(9), new Address { City = "Seattle" }]);

        Assert.Equal(
            [
                Value.Tagged("Some", Value.Integer(7)),
                Value.Tagged("Some", Value.Integer(9)),
                Value.Tagged("Some", Map(("city", Value.String("Seattle")))),
                Value.Tagged("Some", Value.String("text")),
            ],
            span.Inputs.Select(input => input.Value));
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
        RecordedSpan span = Capture(
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
    public void ANonStringMapKeyIsRejected()
    {
        Assert.Equal(
            CaptureRejection.UnsupportedMapKey,
            Reject(
                nameof(Declarations.KeyedByNumber),
                [new SortedDictionary<int, string> { [1] = "one" }]).Rejection);
    }

    [Fact]
    public void ANullInANonNullableDeclarationIsRejected()
    {
        Assert.Equal(
            CaptureRejection.NullValue,
            Reject(nameof(Declarations.Nested), [null]).Rejection);
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
        RecordedSpan span = Capture(nameof(Declarations.EmptyNames), [1]);

        // C# discovery publishes `inp?.Name ?? p.Name`, so an empty declared name
        // reaches the registry. Capture has to agree with it.
        Assert.Equal(string.Empty, Assert.Single(span.Inputs).Name);
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
        RecordedSpan span = Capture(
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
            span.Inputs.Select(input => input.Value));
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
    private static Value Named(RecordedSpan span, string name) =>
        Assert.Single(span.Inputs, input => input.Name == name).Value;

    private static Value Map(params (string Key, Value Value)[] entries) =>
        Value.Map(entries.Select(entry => new KeyValuePair<string, Value>(entry.Key, entry.Value)));

    private static Value Single(string declaration, object?[] arguments) =>
        Assert.Single(Capture(declaration, arguments).Inputs).Value;

    private static RecordedSpan Capture(string declaration, object?[] arguments, object? result = null)
    {
        using CaptureSession session = SpecGateRuntime.BeginSession(Component);

        OperationScope scope = SpecGateRuntime.EnterOperation(
            Declarations.Of(declaration), declaration, Component);
        SpecGateRuntime.RecordInputs(scope, arguments);
        SpecGateRuntime.ExitOk(scope, result);

        return Assert.Single(session.Spans);
    }

    private static SpecGateCaptureException Reject(string declaration, object?[] arguments)
    {
        using CaptureSession session = SpecGateRuntime.BeginSession(Component);

        OperationScope scope = SpecGateRuntime.EnterOperation(
            Declarations.Of(declaration), declaration, Component);

        // A rejected RecordInputs abandons its own scope, so the caller does not
        // close it; the woven try region has not been entered yet.
        return Assert.Throws<SpecGateCaptureException>(
            () => SpecGateRuntime.RecordInputs(scope, arguments));
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

        internal static void Variants(Shape shape) => _ = shape;

        internal static void Ranked(Level level) => _ = level;

        internal static void Widened(Wide value) => _ = value;

        internal static void AliasedRanked(Aliased value) => _ = value;

        internal static void Collided(Collided value) => _ = value;

        internal static void Erased(object value) => _ = value;

        internal static void Unrepresentable(byte value) => _ = value;

        internal static void KeyedByNumber(SortedDictionary<int, string> counts) => _ = counts;

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
}

[SpecEvent]
internal sealed class Person
{
    [SpecEvent("name")]
    public string Name { get; init; } = string.Empty;

    [SpecEvent("home")]
    public Address Home { get; init; } = new();
}

[SpecEvent]
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
[SpecEvent]
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
