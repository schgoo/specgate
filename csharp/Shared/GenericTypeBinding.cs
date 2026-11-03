using System;
using System.Linq;

namespace SpecGate.Collections;

/// <summary>Substitutes declared generic arguments while retaining their nullable positions.</summary>
internal static class GenericTypeBinding
{
    internal static T[] Bind<T>(CollectionShape shape,
        Type declaration, Func<int, T> parameter, Func<Type, T> concrete,
        Func<ShapeConstructor, T[], Type?, T> construct)
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
                return construct(ShapeConstructor.List, [BindType(template.GetElementType()!)], Substitute(template));
            }
            if (Nullable.GetUnderlyingType(template) is Type inner)
            {
                return construct(ShapeConstructor.Option, [BindType(inner)], null);
            }
            string? name = template.IsGenericType ? template.GetGenericTypeDefinition().FullName : null;
            ShapeConstructor? wrapper = name switch
            {
                "System.ValueTuple`2" => ShapeConstructor.Tuple,
                "SpecGate.Annotations.Option`1" => ShapeConstructor.Option,
                "SpecGate.Annotations.Result`2" => ShapeConstructor.Result,
                _ => null,
            };
            if (wrapper is ShapeConstructor constructor)
            {
                return construct(constructor, [.. template.GetGenericArguments().Select(BindType)], null);
            }
            CollectionShape? nested = CollectionContractResolver.Resolve(template);
            if (nested is not null)
            {
                return nested.Describe([.. nested.Arguments.Select(BindType)], Substitute(template), construct);
            }
            return concrete(Substitute(template));
        }
        return [.. shape.Templates.Select(BindType)];
    }
}
