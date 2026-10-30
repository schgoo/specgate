using System;
using System.Collections.Generic;
using System.Reflection;
using SpecGate.Collections;

namespace SpecGate.Discovery;

/// <summary>
/// Normalizes CLR types onto the language-neutral semantic surface published in
/// the CTSC registry, collecting the annotated types it encounters.
/// </summary>
/// <remarks>
/// Collection is stateful: rendering a type that carries <c>SpecEvent</c> queues
/// that type for publication. One instance serves one component document.
/// </remarks>
internal sealed class SemanticTypes
{
    private readonly NullabilityInfoContext nullability;
    private readonly List<Type> queue = [];
    private readonly HashSet<Type> seen = [];
    private bool collecting = true;

    internal SemanticTypes(NullabilityInfoContext nullability) => this.nullability = nullability;

    /// <summary>
    /// Annotated types encountered so far, in discovery order. The list grows
    /// while it is being walked, because publishing a type renders its members.
    /// </summary>
    internal IReadOnlyList<Type> Queue => queue;

    /// <summary>Queues <paramref name="type"/> for publication unless collection is suspended.</summary>
    internal void Collect(Type type)
    {
        if (collecting && seen.Add(type))
        {
            queue.Add(type);
        }
    }

    /// <summary>
    /// Suspends collection until the returned scope is disposed, so rendering a
    /// type for its name alone does not publish it.
    /// </summary>
    internal IDisposable SuspendCollection() => new Suspension(this);

    /// <summary>Renders <paramref name="type"/> including its nullable annotation.</summary>
    internal string Render(Type type, NullabilityInfo? info)
    {
        Type? underlying = Nullable.GetUnderlyingType(type);
        if (underlying is not null)
        {
            NullabilityInfo? inner = info?.GenericTypeArguments is { Length: > 0 } g ? g[0] : null;
            return "Option<" + RenderCore(underlying, inner) + ">";
        }

        if (info is not null && !type.IsValueType && info.ReadState == NullabilityState.Nullable)
        {
            return "Option<" + RenderCore(type, info) + ">";
        }

        return RenderCore(type, info);
    }

    /// <summary>Renders the semantic type of <paramref name="parameter"/>.</summary>
    internal string RenderParameter(ParameterInfo parameter) =>
        Render(parameter.ParameterType, nullability.Create(parameter));

    /// <summary>The published name of an annotated member.</summary>
    internal static string NameOf(MemberInfo member) =>
        Annotations.Text(Annotations.FirstOn(member, Annotations.Event, inherit: false), "Name") ?? member.Name;

    private string RenderCore(Type type, NullabilityInfo? info)
    {
        if (type == typeof(object)) return "value";
        if (type == typeof(int)) return "i32";
        if (type == typeof(long)) return "i64";
        if (type == typeof(uint)) return "u32";
        if (type == typeof(ulong)) return "u64";
        if (type == typeof(short)) return "i16";
        if (type == typeof(sbyte)) return "i8";
        if (type == typeof(double)) return "f64";
        if (type == typeof(float)) return "f32";
        if (type == typeof(bool)) return "bool";
        if (type == typeof(string)) return "string";

        if (type.IsArray)
        {
            Type element = type.GetElementType()!;
            return "List<" + Render(element, info?.ElementType) + ">";
        }

        if (type.IsGenericType)
        {
            Type definition = type.GetGenericTypeDefinition();
            Type[] arguments = type.GetGenericArguments();
            NullabilityInfo[]? generics = info?.GenericTypeArguments;
            NullabilityInfo? At(int index) =>
                generics is not null && index < generics.Length ? generics[index] : null;

            if (Annotations.IsGenericDefinition(definition, Annotations.OptionType))
            {
                return "Option<" + Render(arguments[0], At(0)) + ">";
            }

            if (Annotations.IsGenericDefinition(definition, Annotations.ResultType))
            {
                return "Result<" + Render(arguments[0], At(0)) + ", " + Render(arguments[1], At(1)) + ">";
            }

            if (definition == typeof(ValueTuple<,>))
            {
                return "Tuple<" + Render(arguments[0], At(0)) + ", " + Render(arguments[1], At(1)) + ">";
            }
        }

        if (Annotations.Has(type, Annotations.Event, inherit: false))
        {
            Collect(type);
            return NameOf(type);
        }

        CollectionShape? collection = CollectionShape.Resolve(type);
        if (collection is not null)
        {
            string Construct(string name, string[] arguments) => name + "<" + string.Join(", ", arguments) + ">";
            string[] arguments = collection.Bind(type,
                index => Render(type.GetGenericArguments()[index],
                    info?.GenericTypeArguments is { } generics && index < generics.Length ? generics[index] : null),
                argument => Render(argument, null), (name, mapped, _) => Construct(name, mapped));
            return Construct(collection.Constructor, arguments);
        }

        return type.Name;
    }

    private sealed class Suspension : IDisposable
    {
        private readonly SemanticTypes owner;

        internal Suspension(SemanticTypes owner)
        {
            this.owner = owner;
            owner.collecting = false;
        }

        public void Dispose() => owner.collecting = true;
    }
}
