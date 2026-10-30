using System.Reflection;
using System.Reflection.Emit;
using System.Runtime.Loader;
using System.Text.Json;
using SpecGate.Annotations;
using Xunit;

namespace SpecGate.Discovery.Tests;

/// <summary>
/// Annotated declarations reflected over by <see cref="DiscoveryRunnerTests"/>.
/// They live in the test assembly so discovery runs against real metadata.
/// </summary>
public static class Fixtures
{
    [SpecEvent]
    public enum Phase
    {
        Later = 7,
        Earlier = 1,
    }

    [SpecEvent]
    public sealed class Reading
    {
        [SpecEvent]
        public Phase Phase { get; init; }

        [SpecEvent("renamed")]
        public int Raw { get; init; }

        public int Ignored { get; init; }
    }

    [SpecOperation("record_reading", Spec = "fixtures.alpha")]
    [Exploding]
    public static Reading RecordReading([SpecInput("label")][Exploding] int count) => new() { Raw = count };

    [Exploding]
    public static void UnannotatedHelper()
    {
    }

    [SpecOperation("beta_only", Spec = "fixtures.beta")]
    public static void BetaOnly()
    {
    }

    [SpecSetup("record_reading")]
    public static Reading PrepareReading() => new();

    /// <summary>Declares the input name that <see cref="DerivedSource"/> overrides.</summary>
    public abstract class Source
    {
        /// <summary>Reads one value.</summary>
        /// <param name="count">The semantic input, named on this declaration only.</param>
        /// <returns>The reading.</returns>
        public abstract Reading Read([SpecInput("label")] int count);
    }

    /// <summary>Annotates the override while leaving the input name on the base declaration.</summary>
    public sealed class DerivedSource : Source
    {
        /// <inheritdoc/>
        [SpecOperation("derived_read", Spec = "fixtures.gamma")]
        public override Reading Read(int count) => new() { Raw = count };
    }
}

/// <summary>
/// An unrelated attribute whose constructor throws. Discovery must name
/// annotations from metadata rather than constructing every attribute it meets.
/// </summary>
[AttributeUsage(AttributeTargets.Method | AttributeTargets.Parameter)]
public sealed class ExplodingAttribute : Attribute
{
    /// <summary>Always throws.</summary>
    public ExplodingAttribute() => throw new InvalidOperationException("discovery must not construct unrelated attributes");
}

public class DiscoveryRunnerTests
{
    [Theory]
    [InlineData(typeof(IEnumerable<int>), "Bag<i32>")]
    [InlineData(typeof(ICollection<int>), "Bag<i32>")]
    [InlineData(typeof(IReadOnlyCollection<int>), "Bag<i32>")]
    [InlineData(typeof(System.Collections.Concurrent.ConcurrentBag<int>), "Bag<i32>")]
    [InlineData(typeof(System.Collections.Concurrent.BlockingCollection<int>), "Bag<i32>")]
    [InlineData(typeof(IOrderedEnumerable<int>), "List<i32>")]
    [InlineData(typeof(IReadOnlyList<int>), "List<i32>")]
    [InlineData(typeof(Queue<int>), "List<i32>")]
    [InlineData(typeof(Stack<int>), "List<i32>")]
    [InlineData(typeof(LinkedList<int>), "List<i32>")]
    [InlineData(typeof(ArraySegment<int>), "List<i32>")]
    [InlineData(typeof(System.Collections.Concurrent.ConcurrentQueue<int>), "List<i32>")]
    [InlineData(typeof(System.Collections.Concurrent.ConcurrentStack<int>), "List<i32>")]
    [InlineData(typeof(System.Collections.Immutable.ImmutableArray<int>), "List<i32>")]
    [InlineData(typeof(System.Collections.Immutable.ImmutableList<int>), "List<i32>")]
    [InlineData(typeof(System.Collections.Immutable.ImmutableQueue<int>), "List<i32>")]
    [InlineData(typeof(System.Collections.Immutable.ImmutableStack<int>), "List<i32>")]
    [InlineData(typeof(System.Collections.Frozen.FrozenSet<int>), "set<i32>")]
    [InlineData(typeof(System.Collections.Immutable.ImmutableHashSet<int>), "set<i32>")]
    [InlineData(typeof(SortedSet<int>), "set<i32>")]
    [InlineData(typeof(IReadOnlyDictionary<string, int>), "map<string, i32>")]
    [InlineData(typeof(System.Collections.Concurrent.ConcurrentDictionary<string, int>), "map<string, i32>")]
    [InlineData(typeof(System.Collections.Frozen.FrozenDictionary<string, int>), "map<string, i32>")]
    [InlineData(typeof(OrderedDictionary<string, int>), "OrderedMap<string, i32>")]
    [InlineData(typeof(PriorityQueue<string, int>), "Bag<Tuple<string, i32>>")]
    [InlineData(typeof(ConcreteEnumerable), "Bag<Reading>")]
    [InlineData(typeof(IOrderedUnique), "OrderedSet<i32>")]
    public void CollectionContractsPublishStructuralSemantics(Type type, string expected)
    {
        SemanticTypes types = new(new NullabilityInfoContext());
        Assert.Equal(expected, types.Render(type, null));
    }

    private sealed class ConcreteEnumerable : IEnumerable<Fixtures.Reading>
    {
        IEnumerator<Fixtures.Reading> IEnumerable<Fixtures.Reading>.GetEnumerator() =>
            Enumerable.Empty<Fixtures.Reading>().GetEnumerator();
        System.Collections.IEnumerator System.Collections.IEnumerable.GetEnumerator() =>
            ((IEnumerable<Fixtures.Reading>)this).GetEnumerator();
    }

    private interface IOrderedUnique : IReadOnlyList<int>, IReadOnlySet<int>;

    [Fact]
    public void ConflictingEnumerableContractsAreRejected()
    {
        SemanticTypes types = new(new NullabilityInfoContext());
        Assert.Throws<InvalidOperationException>(() => types.Render(typeof(AmbiguousEnumerable), null));
    }

    private sealed class AmbiguousEnumerable : IEnumerable<int>, IEnumerable<string>
    {
        IEnumerator<int> IEnumerable<int>.GetEnumerator() => Enumerable.Empty<int>().GetEnumerator();
        IEnumerator<string> IEnumerable<string>.GetEnumerator() => Enumerable.Empty<string>().GetEnumerator();
        System.Collections.IEnumerator System.Collections.IEnumerable.GetEnumerator() =>
            throw new InvalidOperationException();
    }

    [Fact]
    public void GenericSpecializationsPublishConcreteSignatures()
    {
        MethodInfo method = typeof(GenericFixture).GetMethod(nameof(GenericFixture.Items))!;
        AnnotatedMethod[] methods =
        [
            new(method,
                [
                    new SpecOperationAttribute("numbers") { Spec = "fixtures.generic", TypeArguments = [typeof(int)] },
                    new SpecOperationAttribute("readings") { Spec = "fixtures.generic", TypeArguments = [typeof(Fixtures.Reading)] },
                ], []),
        ];
        ComponentDocument document = new("fixtures.generic", [typeof(Fixtures.Reading)], methods, new NullabilityInfoContext());

        JsonElement payload = JsonSerializer.SerializeToElement(document.Build());
        JsonElement[] operations = [.. payload.GetProperty("operations").EnumerateArray()];
        Assert.Equal(["numbers", "readings"], operations.Select(operation => operation.GetProperty("name").GetString()));
        Assert.Equal(["List<i32>", "List<Reading>"], operations.Select(operation => operation.GetProperty("return_type").GetString()));
        Assert.Contains(payload.GetProperty("types").EnumerateArray(), type => type.GetProperty("name").GetString() == "Reading");
    }

    private static class GenericFixture
    {
        [SpecOperation("numbers", Spec = "fixtures.generic", TypeArguments = new[] { typeof(int) })]
        [SpecOperation("readings", Spec = "fixtures.generic", TypeArguments = new[] { typeof(Fixtures.Reading) })]
        public static Task<List<T>> Items<T>() where T : notnull => Task.FromResult(new List<T>());

        public static int Reference<T>() where T : class => 0;

        public static int Value<T>() where T : struct => 0;

        public static int Constructible<T>() where T : new() => 0;

        public static int Comparable<T>() where T : IComparable => 0;

        public static int Stream<T>() where T : Stream => 0;
    }

    [Theory]
    [InlineData(nameof(GenericFixture.Reference), typeof(int))]
    [InlineData(nameof(GenericFixture.Value), typeof(string))]
    [InlineData(nameof(GenericFixture.Constructible), typeof(string))]
    [InlineData(nameof(GenericFixture.Comparable), typeof(object))]
    [InlineData(nameof(GenericFixture.Stream), typeof(object))]
    public void InvalidGenericConstraintsNameTheDeclaredSpecialization(string name, Type argument)
    {
        MethodInfo method = typeof(GenericFixture).GetMethod(name)!;
        AnnotatedMethod[] methods =
        [
            new(method,
                [new SpecOperationAttribute("invalid") { Spec = "fixtures.generic", TypeArguments = [argument] }], []),
        ];
        ComponentDocument document = new("fixtures.generic", [], methods, new NullabilityInfoContext());

        GenericSpecializationException failure = Assert.Throws<GenericSpecializationException>(() => document.Build());

        Assert.Contains("fixtures.generic::invalid", failure.Message, StringComparison.Ordinal);
        Assert.Contains(argument.FullName!, failure.Message, StringComparison.Ordinal);
        Assert.Contains("generic constraints", failure.Message, StringComparison.Ordinal);
        Assert.IsType<ArgumentException>(failure.InnerException);
    }

    [Fact]
    public void ValidGenericConstraintsStillPublishConcreteSignatures()
    {
        MethodInfo method = typeof(GenericFixture).GetMethod(nameof(GenericFixture.Value))!;
        AnnotatedMethod[] methods =
        [
            new(method,
                [new SpecOperationAttribute("valid") { Spec = "fixtures.generic", TypeArguments = [typeof(int)] }], []),
        ];
        ComponentDocument document = new("fixtures.generic", [], methods, new NullabilityInfoContext());
        JsonElement payload = JsonSerializer.SerializeToElement(document.Build());
        Assert.Equal("i32", payload.GetProperty("operations")[0].GetProperty("return_type").GetString());
    }

    [Fact]
    public void InvalidGenericConstraintsFailThroughTheDiscoveryEntryPoint()
    {
        AssemblyBuilder assembly = AssemblyBuilder.DefineDynamicAssembly(
            new AssemblyName("InvalidGenericSpecialization"), AssemblyBuilderAccess.Run);
        TypeBuilder holder = assembly.DefineDynamicModule("fixture").DefineType("Holder", TypeAttributes.Public);
        MethodBuilder method = holder.DefineMethod("Reference", MethodAttributes.Public | MethodAttributes.Static, typeof(void), Type.EmptyTypes);
        method.DefineGenericParameters("T")[0].SetGenericParameterAttributes(GenericParameterAttributes.ReferenceTypeConstraint);
        method.SetCustomAttribute(new CustomAttributeBuilder(
            typeof(SpecOperationAttribute).GetConstructor([typeof(string)])!,
            ["invalid"],
            [typeof(SpecOperationAttribute).GetProperty(nameof(SpecOperationAttribute.Spec))!,
                typeof(SpecOperationAttribute).GetProperty(nameof(SpecOperationAttribute.TypeArguments))!],
            ["fixtures.invalid", new[] { typeof(int) }]));
        method.GetILGenerator().Emit(OpCodes.Ret);
        holder.CreateType();
        RecordingSystem system = new(assembly);

        Assert.Equal(1, DiscoveryRunner.Run(["out", "assembly.dll", "bin", "fixtures.invalid"], system.Adapter));
        Assert.DoesNotContain(Path.Combine("out", "0.json"), system.Files.Keys);
    }

    [Fact]
    public void GenericSpecializationsPassTheDiscoveryEntryPoint()
    {
        RecordingSystem system = new();

        Assert.Equal(0, DiscoveryRunner.Run(["out", "assembly.dll", "bin", "fixtures.generic"], system.Adapter));

        JsonElement[] operations = [.. Document(system, 0).GetProperty("operations").EnumerateArray()];
        Assert.Equal(["numbers", "readings"], operations.Select(operation => operation.GetProperty("name").GetString()));
        Assert.Equal(["List<i32>", "List<Reading>"], operations.Select(operation => operation.GetProperty("return_type").GetString()));
    }

    [Fact]
    public void AMethodDeclaringSeveralComponentsIsRejected()
    {
        MethodInfo method = typeof(Fixtures).GetMethod(nameof(Fixtures.BetaOnly))!;
        AnnotatedMethod[] methods =
        [
            new(method, [new SpecOperationAttribute("beta_only") { Spec = "fixtures.beta" }], []),
            new(
                method,
                [
                    new SpecOperationAttribute("beta_only") { Spec = "fixtures.beta" },
                    new SpecOperationAttribute("beta_only") { Spec = "fixtures.other" },
                ],
                []),
        ];

        string message = Assert.IsType<string>(DiscoveryRunner.MultiComponentConflict(methods));

        Assert.Contains("BetaOnly", message, StringComparison.Ordinal);
        Assert.Contains("'fixtures.beta', 'fixtures.other'", message, StringComparison.Ordinal);
        Assert.Null(DiscoveryRunner.MultiComponentConflict(methods.Take(1)));
    }

    [Fact]
    public void TooFewArgumentsAreRejected()
    {
        RecordingSystem system = new();

        Assert.Equal(2, DiscoveryRunner.Run(["out", "assembly.dll"], system.Adapter));
        Assert.Empty(system.Files);
        Assert.Empty(system.Directories);
    }

    [Fact]
    public void ComponentInventoryIsDistinctAndOrdered()
    {
        RecordingSystem system = new();

        Assert.Equal(0, DiscoveryRunner.Run(["out", "assembly.dll", "bin"], system.Adapter));

        Assert.Equal(["out"], system.Directories);
        string[] inventory = JsonSerializer.Deserialize<string[]>(system.Files[Path.Combine("out", "components.json")])!;
        Assert.Equal(["fixtures.alpha", "fixtures.beta", "fixtures.gamma", "fixtures.generic"], inventory);
    }

    /// <summary>
    /// Requesting no component publishes every inventoried one, in inventory
    /// order, so a caller that cannot name them in advance reflects once.
    /// </summary>
    [Fact]
    public void NoRequestedComponentPublishesEveryInventoriedComponent()
    {
        RecordingSystem system = new();

        Assert.Equal(0, DiscoveryRunner.Run(["out", "assembly.dll", "bin"], system.Adapter));

        string[] inventory = JsonSerializer.Deserialize<string[]>(system.Files[Path.Combine("out", "components.json")])!;
        Assert.Equal(["fixtures.alpha", "fixtures.beta", "fixtures.gamma", "fixtures.generic"], inventory);
        Assert.Equal(
            ["record_reading", "record_reading"],
            Document(system, 0).GetProperty("operations").EnumerateArray().Select(operation => operation.GetProperty("name").GetString()));
        Assert.Equal("beta_only", Assert.Single(Document(system, 1).GetProperty("operations").EnumerateArray()).GetProperty("name").GetString());
        Assert.Contains(
            Document(system, 2).GetProperty("operations").EnumerateArray(),
            operation => operation.GetProperty("name").GetString() == "derived_read");
    }

    /// <summary>
    /// An unrelated attribute is named from metadata, never constructed, so a
    /// throwing constructor on an annotated member cannot abort discovery.
    /// </summary>
    [Fact]
    public void UnrelatedAttributesAreNeverConstructed()
    {
        Assert.Throws<InvalidOperationException>(() => new ExplodingAttribute());

        RecordingSystem system = new();

        Assert.Equal(0, DiscoveryRunner.Run(["out", "assembly.dll", "bin", "fixtures.alpha"], system.Adapter));
        Assert.Contains(
            Document(system, 0).GetProperty("operations").EnumerateArray(),
            operation => operation.GetProperty("name").GetString() == "record_reading");
    }

    /// <summary>
    /// An override publishes the input name declared on the base parameter, which
    /// only an inheritance-aware parameter read can see.
    /// </summary>
    [Fact]
    public void OverriddenParametersInheritTheirInputName()
    {
        RecordingSystem system = new();

        Assert.Equal(0, DiscoveryRunner.Run(["out", "assembly.dll", "bin", "fixtures.gamma"], system.Adapter));

        JsonElement operation = Document(system, 0)
            .GetProperty("operations")
            .EnumerateArray()
            .Single(entry => entry.GetProperty("name").GetString() == "derived_read");
        Assert.Equal(["label", "i32"], operation.GetProperty("params")[0].EnumerateArray().Select(entry => entry.GetString()));
        Assert.Equal(["label", "int"], operation.GetProperty("cs_params")[0].EnumerateArray().Select(entry => entry.GetString()));
    }

    [Fact]
    public void RequestedComponentsArePublishedByPosition()
    {
        RecordingSystem system = new();

        Assert.Equal(0, DiscoveryRunner.Run(["out", "assembly.dll", "bin", "fixtures.beta", "fixtures.alpha"], system.Adapter));

        JsonElement beta = Document(system, 0);
        JsonElement alpha = Document(system, 1);
        Assert.Equal("beta_only", Assert.Single(beta.GetProperty("operations").EnumerateArray()).GetProperty("name").GetString());
        Assert.Equal(
            ["record_reading", "record_reading"],
            alpha.GetProperty("operations").EnumerateArray().Select(operation => operation.GetProperty("name").GetString()));
    }

    [Fact]
    public void UnscopedSetupStaysWithTheComponentDeclaringItsOperation()
    {
        RecordingSystem system = new();

        Assert.Equal(0, DiscoveryRunner.Run(["out", "assembly.dll", "bin", "fixtures.beta"], system.Adapter));

        Assert.DoesNotContain(
            Document(system, 0).GetProperty("operations").EnumerateArray(),
            operation => operation.GetProperty("is_setup").GetBoolean());
    }

    [Fact]
    public void OperationPublishesSemanticAndRawMetadata()
    {
        RecordingSystem system = new();

        Assert.Equal(0, DiscoveryRunner.Run(["out", "assembly.dll", "bin", "fixtures.alpha"], system.Adapter));

        JsonElement operation = Document(system, 0)
            .GetProperty("operations")
            .EnumerateArray()
            .Single(entry => !entry.GetProperty("is_setup").GetBoolean());

        Assert.False(operation.GetProperty("is_async").GetBoolean());
        Assert.True(operation.GetProperty("is_public").GetBoolean());
        Assert.True(operation.GetProperty("cs_is_static").GetBoolean());
        Assert.Equal("Reading", operation.GetProperty("return_type").GetString());
        Assert.Equal("SpecGate.Discovery.Tests.Fixtures", operation.GetProperty("cs_class").GetString());
        Assert.Equal(["label", "i32"], operation.GetProperty("params")[0].EnumerateArray().Select(entry => entry.GetString()));
        Assert.Equal(["label", "int"], operation.GetProperty("cs_params")[0].EnumerateArray().Select(entry => entry.GetString()));
    }

    /// <summary>
    /// Variants follow declaration order, not the numeric order
    /// <c>Enum.GetNames</c> imposes, and a renamed member publishes its
    /// <c>SpecEvent</c> name.
    /// </summary>
    [Fact]
    public void ReferencedTypesArePublishedInDeclarationOrder()
    {
        RecordingSystem system = new();

        Assert.Equal(0, DiscoveryRunner.Run(["out", "assembly.dll", "bin", "fixtures.alpha"], system.Adapter));

        JsonElement[] types = [.. Document(system, 0).GetProperty("types").EnumerateArray()];
        JsonElement reading = types.Single(type => type.GetProperty("name").GetString() == "Reading");
        Assert.Equal(
            ["Phase", "renamed"],
            reading.GetProperty("fields").EnumerateArray().Select(field => field[0].GetString()));

        JsonElement phase = types.Single(type => type.GetProperty("name").GetString() == "Phase");
        Assert.Equal("enum", phase.GetProperty("kind").GetString());
        Assert.Equal(
            ["Later", "Earlier"],
            phase.GetProperty("variants").EnumerateArray().Select(variant => variant.GetProperty("name").GetString()));
    }

    private static JsonElement Document(RecordingSystem system, int index) =>
        JsonDocument.Parse(system.Files[Path.Combine("out", index + ".json")]).RootElement;
}

/// <summary>
/// A <see cref="RunnerSystem"/> that reflects over the test assembly and keeps
/// every written document in memory.
/// </summary>
internal sealed class RecordingSystem
{
    private static readonly Assembly Target = typeof(Fixtures).Assembly;

    internal Dictionary<string, string> Files { get; } = [];

    internal List<string> Directories { get; } = [];

    internal RunnerSystem Adapter { get; }

    internal RecordingSystem(Assembly? target = null) =>
        Adapter = new RunnerSystem(
            new FileSystemServices(
                _ => false,
                Directories.Add,
                (path, contents) => Files[path] = contents),
            new AssemblyLoadingServices(_ => target ?? Target, (_, _) => target ?? Target),
            new DependencyResolutionServices(
                _ => new AssemblyDependencyResolver(Target.Location),
                (_, _) => null,
                _ => new Subscription()));

    private sealed class Subscription : IDisposable
    {
        public void Dispose()
        {
        }
    }
}
