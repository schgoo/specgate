using System;
using System.Collections.Generic;
using System.Linq;

namespace SpecGate.Collections;

internal sealed class CollectionShape
{
    private CollectionShape(string constructor, Type[] arguments, Type[] templates, string? enumerationProperty = null)
    {
        Constructor = constructor;
        Arguments = arguments;
        Templates = templates;
        EnumerationProperty = enumerationProperty;
    }

    internal string Constructor { get; }
    internal Type[] Arguments { get; }
    private Type[] Templates { get; }
    internal string? EnumerationProperty { get; }

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
            CollectionShape queue = FromContract(type, priorityQueue, "Bag");
            return new CollectionShape("Bag",
                [typeof(ValueTuple<,>).MakeGenericType(queue.Arguments)],
                [typeof(ValueTuple<,>).MakeGenericType(queue.Templates)], "UnorderedItems");
        }

        Type[] maps = Matches(contracts,
            "System.Collections.Generic.IDictionary`2",
            "System.Collections.Generic.IReadOnlyDictionary`2");
        Type[][] pairs = [.. maps.Select(contract => contract.GetGenericArguments())
            .Distinct(TypeArgumentsComparer.Instance)];
        if (pairs.Length > 1)
        {
            throw new InvalidOperationException($"'{type}' declares conflicting dictionary key/value types.");
        }
        if (maps.Length > 0)
        {
            string constructor = Family(type, "System.Collections.Generic.OrderedDictionary`2")
                ? "OrderedMap" : "map";
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
        bool ordered = Matches(contracts,
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
                "System.Linq.OrderedParallelQuery`1");
        string name = (ordered, unique) switch
        {
            (true, false) => "List",
            (false, false) => "Bag",
            (false, true) => "set",
            (true, true) => "OrderedSet",
        };
        return FromContract(type, enumerables[0], name);
    }

    internal T[] Bind<T>(
        Type declaration, Func<int, T> parameter, Func<Type, T> concrete, Func<string, T[], Type?, T> construct)
    {
        Type[] parameters = declaration.IsGenericType
            ? declaration.GetGenericTypeDefinition().GetGenericArguments() : [];
        Type Substitute(Type expression) => expression.IsGenericParameter
            ? declaration.GetGenericArguments()[Array.IndexOf(parameters, expression)]
            : expression.IsArray
                ? (expression.GetArrayRank() == 1
                    ? Substitute(expression.GetElementType()!).MakeArrayType()
                    : Substitute(expression.GetElementType()!).MakeArrayType(expression.GetArrayRank()))
                : expression.IsGenericType
                    ? expression.GetGenericTypeDefinition().MakeGenericType(
                        [.. expression.GetGenericArguments().Select(Substitute)])
                    : expression;
        T BindType(Type template)
        {
            int index = Array.IndexOf(parameters, template);
            if (index >= 0)
            {
                return parameter(index);
            }
            if (!template.ContainsGenericParameters)
            {
                return concrete(template);
            }
            if (template.IsArray)
            {
                return construct("List", [BindType(template.GetElementType()!)], Substitute(template));
            }
            if (Nullable.GetUnderlyingType(template) is Type inner)
            {
                return construct("Option", [BindType(inner)], null);
            }
            if (Is(template, "System.ValueTuple`2"))
            {
                return construct("Tuple", [.. template.GetGenericArguments().Select(BindType)], null);
            }
            if (Is(template, "SpecGate.Annotations.Option`1", "SpecGate.Annotations.Result`2"))
            {
                string constructor = Is(template, "SpecGate.Annotations.Option`1") ? "Option" : "Result";
                return construct(constructor, [.. template.GetGenericArguments().Select(BindType)], null);
            }
            CollectionShape? nested = Resolve(template);
            if (nested is not null)
            {
                return construct(nested.Constructor, [.. nested.Arguments.Select(BindType)], Substitute(template));
            }
            return concrete(Substitute(template));
        }
        return [.. Templates.Select(BindType)];
    }

    private static CollectionShape FromContract(Type declaration, Type contract, string constructor)
    {
        Type template = contract;
        if (declaration.IsGenericType)
        {
            Type definition = declaration.GetGenericTypeDefinition();
            template = Hierarchy(definition).Concat(definition.GetInterfaces())
                .Single(candidate => candidate.IsGenericType
                    && candidate.GetGenericTypeDefinition() == contract.GetGenericTypeDefinition());
        }
        return new CollectionShape(constructor, contract.GetGenericArguments(), template.GetGenericArguments());
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
