using System;
using System.Collections.Generic;
using System.Linq;

namespace SpecGate.Collections;

/// <summary>Resolves native contracts without inspecting collection contents.</summary>
internal static class CollectionContractResolver
{
    internal static CollectionShape? Resolve(Type type)
    {
        Type[] contracts = [.. Hierarchy(type), .. type.GetInterfaces()];
        Type[] enumerables = Matches(contracts, "System.Collections.Generic.IEnumerable`1");
        if (enumerables.Length > 1)
        {
            throw new InvalidOperationException($"'{type}' declares conflicting IEnumerable<T> element types.");
        }

        Type? priorityQueue = contracts.FirstOrDefault(contract => Is(contract, "System.Collections.Generic.PriorityQueue`2"));
        if (priorityQueue is not null)
        {
            CollectionShape queue = FromContract(type, priorityQueue, ShapeConstructor.Bag);
            return new CollectionShape(ShapeConstructor.Bag,
                [typeof(ValueTuple<,>).MakeGenericType(queue.Arguments)],
                [typeof(ValueTuple<,>).MakeGenericType(queue.Templates)], "UnorderedItems");
        }

        Type[] lookups = Matches(contracts, "System.Linq.ILookup`2");
        Type[] groupings = Matches(contracts, "System.Linq.IGrouping`2");
        Type[] maps = Matches(contracts,
            "System.Collections.Generic.IDictionary`2",
            "System.Collections.Generic.IReadOnlyDictionary`2");
        Type[][] pairs = [.. maps.Select(contract => contract.GetGenericArguments())
            .Distinct(TypeArgumentsComparer.Instance)];
        if (pairs.Length > 1 || lookups.Length > 1 || groupings.Length > 1
            || (lookups.Length > 0 && (groupings.Length > 0 || maps.Length > 0))
            || (groupings.Length > 0 && maps.Length > 0))
        {
            throw new InvalidOperationException($"'{type}' declares conflicting keyed collection contracts.");
        }
        if (lookups.Length > 0)
        {
            return FromContract(type, lookups[0], ShapeConstructor.Map, CollectionProjection.Lookup);
        }
        if (groupings.Length > 0)
        {
            return FromContract(type, groupings[0], ShapeConstructor.Record, CollectionProjection.Grouping);
        }
        if (maps.Length > 0)
        {
            ShapeConstructor constructor = Family(type, "System.Collections.Generic.OrderedDictionary`2")
                ? ShapeConstructor.OrderedMap : ShapeConstructor.Map;
            return FromContract(type, maps[0], constructor);
        }

        if (enumerables.Length == 0)
        {
            return null;
        }

        bool unique = Matches(contracts,
            "System.Collections.Generic.ISet`1",
            "System.Collections.Generic.IReadOnlySet`1",
            "System.Collections.Immutable.IImmutableSet`1").Length > 0;
        bool sortedSet = Family(type,
            "System.Collections.Generic.SortedSet`1",
            "System.Collections.Immutable.ImmutableSortedSet`1");
        bool ordered = !sortedSet && (Matches(contracts,
            "System.Collections.Generic.IList`1",
            "System.Collections.Generic.IReadOnlyList`1",
            "System.Linq.IOrderedEnumerable`1",
            "System.Linq.IOrderedQueryable`1",
            "System.Collections.Immutable.IImmutableQueue`1",
            "System.Collections.Immutable.IImmutableStack`1").Length > 0
            || Family(type,
                "System.Collections.Generic.LinkedList`1",
                "System.Collections.Generic.Queue`1",
                "System.Collections.Generic.Stack`1",
                "System.Collections.Concurrent.ConcurrentQueue`1",
                "System.Collections.Concurrent.ConcurrentStack`1",
                "System.Linq.OrderedParallelQuery`1"));
        ShapeConstructor name = (ordered, unique) switch
        {
            (true, false) => ShapeConstructor.List,
            (false, false) => ShapeConstructor.Bag,
            (false, true) => ShapeConstructor.Set,
            (true, true) => ShapeConstructor.OrderedSet,
        };
        return FromContract(type, enumerables[0], name);
    }

    private static CollectionShape FromContract(Type declaration, Type contract, ShapeConstructor constructor,
        CollectionProjection projection = CollectionProjection.Elements)
    {
        Type template = contract;
        if (declaration.IsGenericType)
        {
            Type definition = declaration.GetGenericTypeDefinition();
            template = Hierarchy(definition).Concat(definition.GetInterfaces())
                .Single(candidate => candidate.IsGenericType
                    && candidate.GetGenericTypeDefinition() == contract.GetGenericTypeDefinition());
        }
        return new CollectionShape(constructor, contract.GetGenericArguments(), template.GetGenericArguments(),
            projection: projection);
    }

    private static Type[] Matches(Type[] types, params string[] definitions) =>
        [.. types.Where(type => Is(type, definitions)).Distinct()];

    private static bool Is(Type type, params string[] definitions) =>
        type.IsGenericType
        && definitions.Contains(type.GetGenericTypeDefinition().FullName, StringComparer.Ordinal);

    private static bool Family(Type type, params string[] definitions) =>
        Hierarchy(type).Any(current => Is(current, definitions));

    private static IEnumerable<Type> Hierarchy(Type type)
    {
        for (Type? current = type; current is not null; current = current.BaseType)
        {
            yield return current;
        }
    }

    private sealed class TypeArgumentsComparer : IEqualityComparer<Type[]>
    {
        internal static TypeArgumentsComparer Instance { get; } = new();
        public bool Equals(Type[]? left, Type[]? right) =>
            left is not null && right is not null && left.SequenceEqual(right);
        public int GetHashCode(Type[] arguments)
        {
            HashCode hash = new();
            foreach (Type argument in arguments)
            {
                hash.Add(argument);
            }
            return hash.ToHashCode();
        }
    }
}
