using System;
using System.Collections.Generic;
using System.Reflection;

namespace SpecGate.Reflection;

/// <summary>Effective public instance members, in stable base-first slots.</summary>
internal static class RecordMembers
{
    internal static IReadOnlyList<MemberInfo> Effective(Type type)
    {
        Stack<Type> hierarchy = new();
        for (Type? current = type; current is not null; current = current.BaseType)
        {
            hierarchy.Push(current);
        }

        List<MemberInfo> members = [];
        Dictionary<string, int> slots = new(StringComparer.Ordinal);
        const BindingFlags selection = BindingFlags.Public | BindingFlags.Instance | BindingFlags.DeclaredOnly;
        while (hierarchy.Count > 0)
        {
            Type current = hierarchy.Pop();
            // Preserve the existing properties-before-fields order within each declaration.
            foreach (MemberInfo member in current.GetProperties(selection))
            {
                Add(member);
            }
            foreach (MemberInfo member in current.GetFields(selection))
            {
                Add(member);
            }
        }
        return members;

        void Add(MemberInfo member)
        {
            if (slots.TryGetValue(member.Name, out int slot))
            {
                members[slot] = member;
            }
            else
            {
                slots.Add(member.Name, members.Count);
                members.Add(member);
            }
        }
    }
}
