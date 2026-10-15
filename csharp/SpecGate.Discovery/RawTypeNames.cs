using System;
using System.Collections.Generic;
using System.Reflection;

namespace SpecGate.Discovery;

/// <summary>
/// Renders raw C# type spelling — keywords, generics, and nullable annotations —
/// for replay code generation.
/// </summary>
/// <remarks>
/// This is distinct from <see cref="SemanticTypes"/>, which normalizes the
/// language-neutral semantic surface published in the CTSC registry.
/// </remarks>
internal static class RawTypeNames
{
    /// <summary>Renders <paramref name="type"/> including its nullable annotation.</summary>
    internal static string Render(Type type, NullabilityInfo? info)
    {
        Type? underlying = Nullable.GetUnderlyingType(type);
        if (underlying is not null)
        {
            NullabilityInfo? inner = info?.GenericTypeArguments is { Length: > 0 } g ? g[0] : null;
            return RenderCore(underlying, inner) + "?";
        }

        string core = RenderCore(type, info);
        if (info is not null && !type.IsValueType && info.ReadState == NullabilityState.Nullable)
        {
            return core + "?";
        }

        return core;
    }

    private static string RenderCore(Type type, NullabilityInfo? info)
    {
        if (type == typeof(void)) return "void";
        if (type == typeof(object)) return "object";
        if (type == typeof(bool)) return "bool";
        if (type == typeof(string)) return "string";
        if (type == typeof(char)) return "char";
        if (type == typeof(int)) return "int";
        if (type == typeof(long)) return "long";
        if (type == typeof(short)) return "short";
        if (type == typeof(sbyte)) return "sbyte";
        if (type == typeof(byte)) return "byte";
        if (type == typeof(uint)) return "uint";
        if (type == typeof(ulong)) return "ulong";
        if (type == typeof(ushort)) return "ushort";
        if (type == typeof(double)) return "double";
        if (type == typeof(float)) return "float";
        if (type == typeof(decimal)) return "decimal";

        if (type.IsArray)
        {
            Type element = type.GetElementType()!;
            return Render(element, info?.ElementType) + "[]";
        }

        if (type.IsGenericType)
        {
            Type[] arguments = type.GetGenericArguments();
            NullabilityInfo[]? generics = info?.GenericTypeArguments;
            NullabilityInfo? At(int index) =>
                generics is not null && index < generics.Length ? generics[index] : null;

            string baseName = type.Name;
            int tick = baseName.IndexOf('`');
            if (tick >= 0) baseName = baseName.Substring(0, tick);

            var parts = new List<string>();
            for (int i = 0; i < arguments.Length; i++)
            {
                parts.Add(Render(arguments[i], At(i)));
            }

            return baseName + "<" + string.Join(", ", parts) + ">";
        }

        return type.Name;
    }
}
