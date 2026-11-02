using System;

namespace SpecGate.Collections;

internal enum ShapeConstructor
{
    List, Bag, Set, OrderedSet, Map, OrderedMap, Tuple, Option, Result, Record,
}

internal enum CollectionProjection
{
    Elements, Lookup, Grouping,
}

/// <summary>A native contract and its language-neutral structural description.</summary>
internal sealed class CollectionShape(
    ShapeConstructor constructor, Type[] arguments, Type[] templates,
    string? enumerationProperty = null, CollectionProjection projection = CollectionProjection.Elements)
{
    internal ShapeConstructor Constructor { get; } = constructor;
    internal Type[] Arguments { get; } = arguments;
    internal Type[] Templates { get; } = templates;
    internal string? EnumerationProperty { get; } = enumerationProperty;
    internal CollectionProjection Projection { get; } = projection;

    internal T Describe<T>(T[] arguments, Type declaration, Func<ShapeConstructor, T[], Type?, T> construct)
    {
        if (Projection is CollectionProjection.Lookup or CollectionProjection.Grouping)
        {
            arguments = [arguments[0], construct(ShapeConstructor.Bag, [arguments[1]], null)];
        }
        return construct(Constructor, arguments, declaration);
    }
}
