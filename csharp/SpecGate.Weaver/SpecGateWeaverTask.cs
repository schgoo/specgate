using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using Microsoft.Build.Framework;
using Microsoft.Build.Utilities;
using Mono.Cecil;
using Mono.Cecil.Cil;
using Mono.Cecil.Rocks;

namespace SpecGate.Weaver;

/// <summary>
/// MSBuild task that instruments compiled SpecGate C# fixture assemblies.
/// </summary>
public sealed class SpecGateWeaverTask : Task
{
    private const string SpecOperationAttribute = "SpecGate.Annotations.SpecOperationAttribute";
    private const string SpecInputAttribute = "SpecGate.Annotations.SpecInputAttribute";
    private const string SpecEventAttribute = "SpecGate.Annotations.SpecEventAttribute";
    private const string SpecMockAttribute = "SpecGate.Annotations.SpecMockAttribute";

    private MethodReference? _enterOperation;
    private MethodReference? _emitMember;
    private MethodReference? _mockCallDefinition;
    private MethodReference? _specDefaultDefinition;
    private MethodReference? _arrayEmptyDefinition;

    /// <summary>
    /// Gets or sets the assembly path to weave in place.
    /// </summary>
    [Required]
    public string AssemblyPath { get; set; } = string.Empty;

    /// <inheritdoc />
    public override bool Execute()
    {
        try
        {
            if (string.IsNullOrWhiteSpace(AssemblyPath) || !File.Exists(AssemblyPath))
            {
                Log.LogError("SpecGate weaver assembly path does not exist: {0}", AssemblyPath);
                return false;
            }

            bool changed = Weave(AssemblyPath);
            Log.LogMessage(MessageImportance.Low, changed ? "Wove SpecGate instrumentation into {0}." : "SpecGate instrumentation already up to date for {0}.", AssemblyPath);
            return !Log.HasLoggedErrors;
        }
        catch (Exception ex)
        {
            Log.LogErrorFromException(ex, showStackTrace: true);
            return false;
        }
    }

    private bool Weave(string assemblyPath)
    {
        string? directory = Path.GetDirectoryName(assemblyPath);
        var resolver = new DefaultAssemblyResolver();
        if (!string.IsNullOrEmpty(directory))
        {
            resolver.AddSearchDirectory(directory);
        }

        var readerParameters = new ReaderParameters
        {
            AssemblyResolver = resolver,
            ReadSymbols = File.Exists(Path.ChangeExtension(assemblyPath, ".pdb")),
            ReadWrite = false,
        };

        bool changed = false;
        string tempAssembly = assemblyPath + ".specgate-weave";
        string? pdbPath = Path.ChangeExtension(assemblyPath, ".pdb");
        string tempPdb = tempAssembly + ".pdb";
        using (var assembly = AssemblyDefinition.ReadAssembly(assemblyPath, readerParameters))
        {
            ModuleDefinition module = assembly.MainModule;
            InitializeRuntimeReferences(module);

            foreach (TypeDefinition type in module.Types.SelectMany(FlattenTypes))
            {
                Dictionary<FieldDefinition, string> mockFields = GetMockFields(type);
                foreach (PropertyDefinition property in type.Properties)
                {
                    changed |= WeaveEventProperty(module, property);
                }

                foreach (MethodDefinition method in type.Methods)
                {
                    if (TryGetOperationName(method, out string? operation))
                    {
                        changed |= WeaveMockCalls(module, method, mockFields);
                        changed |= WeaveOperationEntry(module, method, operation!);
                    }
                }
            }

            if (!changed)
            {
                return false;
            }

            var writerParameters = new WriterParameters
            {
                WriteSymbols = readerParameters.ReadSymbols,
            };

            assembly.Write(tempAssembly, writerParameters);
        }

        File.Copy(tempAssembly, assemblyPath, overwrite: true);
        File.Delete(tempAssembly);
        if (readerParameters.ReadSymbols && File.Exists(tempPdb) && pdbPath is not null)
        {
            File.Copy(tempPdb, pdbPath, overwrite: true);
            File.Delete(tempPdb);
        }

        return true;
    }

    private static IEnumerable<TypeDefinition> FlattenTypes(TypeDefinition type)
    {
        yield return type;
        foreach (TypeDefinition nested in type.NestedTypes.SelectMany(FlattenTypes))
        {
            yield return nested;
        }
    }

    private void InitializeRuntimeReferences(ModuleDefinition module)
    {
        AssemblyNameReference runtimeAssembly = module.AssemblyReferences.FirstOrDefault(a => a.Name == "SpecGate.Runtime")
            ?? new AssemblyNameReference("SpecGate.Runtime", new Version(1, 0, 0, 0));
        if (!module.AssemblyReferences.Contains(runtimeAssembly))
        {
            module.AssemblyReferences.Add(runtimeAssembly);
        }
        var runtimeType = new TypeReference("SpecGate.Runtime", "SpecGateRuntime", module, runtimeAssembly);
        _enterOperation = module.ImportReference(new MethodReference("EnterOperation", module.TypeSystem.Void, runtimeType)
        {
            HasThis = false,
            Parameters =
            {
                new ParameterDefinition(module.TypeSystem.String),
                new ParameterDefinition(new ArrayType(module.TypeSystem.String)),
                new ParameterDefinition(new ArrayType(module.TypeSystem.Object)),
            },
        });
        _emitMember = module.ImportReference(new MethodReference("EmitMember", module.TypeSystem.Void, runtimeType)
        {
            HasThis = false,
            Parameters =
            {
                new ParameterDefinition(module.TypeSystem.Object),
                new ParameterDefinition(module.TypeSystem.String),
                new ParameterDefinition(module.TypeSystem.Object),
            },
        });

        var mockCall = new MethodReference("MockCall", module.TypeSystem.Void, runtimeType)
        {
            HasThis = false,
        };
        var genericMockResult = new GenericParameter("T", mockCall);
        mockCall.GenericParameters.Add(genericMockResult);
        mockCall.ReturnType = genericMockResult;
        mockCall.Parameters.Add(new ParameterDefinition(module.TypeSystem.String));
        mockCall.Parameters.Add(new ParameterDefinition(module.TypeSystem.Object));
        mockCall.Parameters.Add(new ParameterDefinition(new ByReferenceType(module.TypeSystem.Boolean)));
        _mockCallDefinition = module.ImportReference(mockCall);

        var specDefault = new MethodReference("SpecDefault", module.TypeSystem.Void, runtimeType)
        {
            HasThis = false,
        };
        var genericDefaultResult = new GenericParameter("T", specDefault);
        specDefault.GenericParameters.Add(genericDefaultResult);
        specDefault.ReturnType = genericDefaultResult;
        _specDefaultDefinition = module.ImportReference(specDefault);

        var arrayType = module.ImportReference(typeof(Array));
        var arrayEmpty = new MethodReference("Empty", module.TypeSystem.Void, arrayType)
        {
            HasThis = false,
        };
        var genericArrayResult = new GenericParameter("T", arrayEmpty);
        arrayEmpty.GenericParameters.Add(genericArrayResult);
        arrayEmpty.ReturnType = new ArrayType(genericArrayResult);
        _arrayEmptyDefinition = module.ImportReference(arrayEmpty);
    }

    private static Dictionary<FieldDefinition, string> GetMockFields(TypeDefinition type)
    {
        var result = new Dictionary<FieldDefinition, string>();
        foreach (FieldDefinition field in type.Fields)
        {
            CustomAttribute? attr = field.CustomAttributes.FirstOrDefault(a => a.AttributeType.FullName == SpecMockAttribute);
            if (attr is not null && !field.IsStatic && attr.ConstructorArguments.Count > 0 && attr.ConstructorArguments[0].Value is string name)
            {
                result[field] = name;
            }
        }

        return result;
    }

    private static bool TryGetOperationName(MethodDefinition method, out string? operation)
    {
        CustomAttribute? attr = method.CustomAttributes.FirstOrDefault(a => a.AttributeType.FullName == SpecOperationAttribute);
        if (attr is not null && attr.ConstructorArguments.Count > 0 && attr.ConstructorArguments[0].Value is string name)
        {
            operation = name;
            return true;
        }

        operation = null;
        return false;
    }

    private bool WeaveOperationEntry(ModuleDefinition module, MethodDefinition method, string operation)
    {
        if (!method.HasBody || method.Body.Instructions.Any(IsEnterOperationCall))
        {
            return false;
        }

        method.Body.SimplifyMacros();
        ILProcessor il = method.Body.GetILProcessor();
        Instruction first = method.Body.Instructions.First();
        foreach (Instruction instruction in BuildOperationEntry(module, method, operation))
        {
            il.InsertBefore(first, instruction);
        }

        method.Body.OptimizeMacros();
        return true;
    }

    private List<Instruction> BuildOperationEntry(ModuleDefinition module, MethodDefinition method, string operation)
    {
        var instructions = new List<Instruction>
        {
            Instruction.Create(OpCodes.Ldstr, operation),
        };
        List<ParameterDefinition> parameters = [.. method.Parameters.Where(p => !p.IsOut && !p.ParameterType.IsByReference)];
        EmitStringArray(module, instructions, [.. parameters.Select(SpecInputName)]);
        EmitObjectArray(module, instructions, parameters);
        instructions.Add(Instruction.Create(OpCodes.Call, _enterOperation ?? throw new InvalidOperationException("Runtime references not initialized.")));
        return instructions;
    }

    private void EmitStringArray(ModuleDefinition module, List<Instruction> instructions, List<string> values)
    {
        if (values.Count == 0)
        {
            instructions.Add(Instruction.Create(OpCodes.Call, MakeArrayEmpty(module.TypeSystem.String)));
            return;
        }

        instructions.Add(LoadInt(values.Count));
        instructions.Add(Instruction.Create(OpCodes.Newarr, module.TypeSystem.String));
        for (int i = 0; i < values.Count; i++)
        {
            instructions.Add(Instruction.Create(OpCodes.Dup));
            instructions.Add(LoadInt(i));
            instructions.Add(Instruction.Create(OpCodes.Ldstr, values[i]));
            instructions.Add(Instruction.Create(OpCodes.Stelem_Ref));
        }
    }

    private void EmitObjectArray(ModuleDefinition module, List<Instruction> instructions, List<ParameterDefinition> parameters)
    {
        if (parameters.Count == 0)
        {
            instructions.Add(Instruction.Create(OpCodes.Call, MakeArrayEmpty(module.TypeSystem.Object)));
            return;
        }

        instructions.Add(LoadInt(parameters.Count));
        instructions.Add(Instruction.Create(OpCodes.Newarr, module.TypeSystem.Object));
        for (int i = 0; i < parameters.Count; i++)
        {
            ParameterDefinition parameter = parameters[i];
            instructions.Add(Instruction.Create(OpCodes.Dup));
            instructions.Add(LoadInt(i));
            instructions.Add(Instruction.Create(OpCodes.Ldarg, parameter));
            BoxIfNeeded(instructions, parameter.ParameterType);
            instructions.Add(Instruction.Create(OpCodes.Stelem_Ref));
        }
    }

    private static string SpecInputName(ParameterDefinition parameter)
    {
        CustomAttribute? attr = parameter.CustomAttributes.FirstOrDefault(a => a.AttributeType.FullName == SpecInputAttribute);
        if (attr is not null && attr.ConstructorArguments.Count > 0 && attr.ConstructorArguments[0].Value is string name)
        {
            return name;
        }

        return parameter.Name.TrimStart('@');
    }

    private bool WeaveEventProperty(ModuleDefinition module, PropertyDefinition property)
    {
        CustomAttribute? attr = property.CustomAttributes.FirstOrDefault(a => a.AttributeType.FullName == SpecEventAttribute);
        MethodDefinition? setter = property.SetMethod;
        if (attr is null || setter is null || setter.IsStatic || !setter.HasBody || setter.Body.Instructions.Any(IsEmitMemberCall))
        {
            return false;
        }

        string eventName = attr.ConstructorArguments.Count > 0 && attr.ConstructorArguments[0].Value is string name
            ? name
            : property.Name.TrimStart('@');
        methodBodySetup(setter);
        ILProcessor il = setter.Body.GetILProcessor();
        foreach (Instruction ret in setter.Body.Instructions.Where(i => i.OpCode == OpCodes.Ret).ToList())
        {
            var emit = new List<Instruction>
            {
                Instruction.Create(OpCodes.Ldarg_0),
                Instruction.Create(OpCodes.Ldstr, eventName),
                Instruction.Create(OpCodes.Ldarg, setter.Parameters[0]),
            };
            BoxIfNeeded(emit, setter.Parameters[0].ParameterType);
            emit.Add(Instruction.Create(OpCodes.Call, _emitMember ?? throw new InvalidOperationException("Runtime references not initialized.")));
            foreach (Instruction instruction in emit)
            {
                il.InsertBefore(ret, instruction);
            }
        }

        setter.Body.OptimizeMacros();
        return true;

        static void methodBodySetup(MethodDefinition method)
        {
            method.Body.SimplifyMacros();
        }
    }

    private bool WeaveMockCalls(ModuleDefinition module, MethodDefinition method, Dictionary<FieldDefinition, string> mockFields)
    {
        if (mockFields.Count == 0 || !method.HasBody)
        {
            return false;
        }

        bool changed = false;
        method.Body.SimplifyMacros();
        method.Body.InitLocals = true;
        var body = method.Body;
        int index = 0;
        while (index < body.Instructions.Count)
        {
            Instruction call = body.Instructions[index];
            if ((call.OpCode == OpCodes.Call || call.OpCode == OpCodes.Callvirt)
                && call.Operand is MethodReference called
                && called.HasThis
                && TryFindMockCallSlice(call, called, mockFields, out FieldDefinition? field, out List<Instruction>? keyInstructions, out Instruction? start)
                && field is not null
                && keyInstructions is not null
                && start is not null)
            {
                Instruction? next = call.Next;
                if (next is not null && IsStoreLocal(body, next, out VariableDefinition? targetLocal) && targetLocal is not null)
                {
                    Instruction? resume = next.Next;
                    ReplaceAssignmentMockCall(module, method, start, next, keyInstructions, mockFields[field], targetLocal);
                    changed = true;
                    index = ResumeIndex(body, resume);
                    continue;
                }

                if (next is not null && next.OpCode == OpCodes.Ret && !method.ReturnType.IsVoid())
                {
                    ReplaceReturnMockCall(module, method, start, next, keyInstructions, mockFields[field], method.ReturnType);
                    changed = true;
                    index = body.Instructions.Count;
                    continue;
                }

                if (called.ReturnType.IsVoid() || (next is not null && next.OpCode == OpCodes.Pop))
                {
                    Instruction end = called.ReturnType.IsVoid() ? call : next!;
                    Instruction? resume = end.Next;
                    ReplaceDiscardMockCall(module, method, start, end, keyInstructions, mockFields[field]);
                    changed = true;
                    index = ResumeIndex(body, resume);
                    continue;
                }
            }

            index++;
        }

        if (changed)
        {
            method.Body.OptimizeMacros();
        }

        return changed;
    }

    private static int ResumeIndex(MethodBody body, Instruction? resume)
    {
        if (resume is null)
        {
            return body.Instructions.Count;
        }

        int index = body.Instructions.IndexOf(resume);
        return index < 0 ? body.Instructions.Count : index;
    }

    private void ReplaceAssignmentMockCall(
        ModuleDefinition module,
        MethodDefinition method,
        Instruction start,
        Instruction end,
        List<Instruction> keyInstructions,
        string mockName,
        VariableDefinition targetLocal)
    {
        var hit = new VariableDefinition(module.TypeSystem.Boolean);
        method.Body.Variables.Add(hit);
        var after = end.Next ?? Instruction.Create(OpCodes.Nop);
        bool needsTrailingNop = end.Next is null;
        var continueInstruction = Instruction.Create(OpCodes.Nop);
        var replacement = new List<Instruction>();
        EmitMockCall(module, replacement, mockName, keyInstructions, targetLocal.VariableType, hit);
        replacement.Add(Instruction.Create(OpCodes.Stloc, targetLocal));
        EmitMissReturn(replacement, method, hit, continueInstruction);
        replacement.Add(continueInstruction);
        ReplaceRange(method.Body, start, end, replacement, after, needsTrailingNop);
    }

    private void ReplaceReturnMockCall(
        ModuleDefinition module,
        MethodDefinition method,
        Instruction start,
        Instruction end,
        List<Instruction> keyInstructions,
        string mockName,
        TypeReference returnType)
    {
        var hit = new VariableDefinition(module.TypeSystem.Boolean);
        var value = new VariableDefinition(returnType);
        method.Body.Variables.Add(hit);
        method.Body.Variables.Add(value);
        var after = Instruction.Create(OpCodes.Ldloc, value);
        var replacement = new List<Instruction>();
        EmitMockCall(module, replacement, mockName, keyInstructions, returnType, hit);
        replacement.Add(Instruction.Create(OpCodes.Stloc, value));
        EmitMissReturn(replacement, method, hit, after);
        replacement.Add(after);
        replacement.Add(Instruction.Create(OpCodes.Ret));
        ReplaceRange(method.Body, start, end, replacement, afterInstruction: null, appendAfterEnd: false);
    }

    private void ReplaceDiscardMockCall(
        ModuleDefinition module,
        MethodDefinition method,
        Instruction start,
        Instruction end,
        List<Instruction> keyInstructions,
        string mockName)
    {
        var hit = new VariableDefinition(module.TypeSystem.Boolean);
        method.Body.Variables.Add(hit);
        var after = end.Next ?? Instruction.Create(OpCodes.Nop);
        bool needsTrailingNop = end.Next is null;
        var continueInstruction = Instruction.Create(OpCodes.Nop);
        var replacement = new List<Instruction>();
        EmitMockCall(module, replacement, mockName, keyInstructions, module.TypeSystem.String, hit);
        replacement.Add(Instruction.Create(OpCodes.Pop));
        EmitMissReturn(replacement, method, hit, continueInstruction);
        replacement.Add(continueInstruction);
        ReplaceRange(method.Body, start, end, replacement, after, needsTrailingNop);
    }

    private void EmitMockCall(
        ModuleDefinition module,
        List<Instruction> instructions,
        string mockName,
        List<Instruction> keyInstructions,
        TypeReference resultType,
        VariableDefinition hit)
    {
        instructions.Add(Instruction.Create(OpCodes.Ldstr, mockName));
        foreach (Instruction keyInstruction in keyInstructions)
        {
            instructions.Add(CloneInstruction(keyInstruction));
        }

        BoxIfNeeded(instructions, InferStackTypeForBox(module, keyInstructions[keyInstructions.Count - 1]));
        instructions.Add(Instruction.Create(OpCodes.Ldloca, hit));
        instructions.Add(Instruction.Create(OpCodes.Call, MakeGenericMethod(_mockCallDefinition ?? throw new InvalidOperationException("Runtime references not initialized."), resultType)));
    }

    private void EmitMissReturn(List<Instruction> instructions, MethodDefinition method, VariableDefinition hit, Instruction continueTarget)
    {
        instructions.Add(Instruction.Create(OpCodes.Ldloc, hit));
        instructions.Add(Instruction.Create(OpCodes.Brtrue, continueTarget));
        if (!method.ReturnType.IsVoid())
        {
            instructions.Add(Instruction.Create(OpCodes.Call, MakeGenericMethod(_specDefaultDefinition ?? throw new InvalidOperationException("Runtime references not initialized."), method.ReturnType)));
        }

        instructions.Add(Instruction.Create(OpCodes.Ret));
    }

    private static void ReplaceRange(MethodBody body, Instruction start, Instruction end, List<Instruction> replacement, Instruction? afterInstruction, bool appendAfterEnd)
    {
        ILProcessor il = body.GetILProcessor();
        foreach (Instruction instruction in replacement)
        {
            il.InsertBefore(start, instruction);
        }

        if (appendAfterEnd && afterInstruction is not null)
        {
            il.InsertAfter(end, afterInstruction);
        }

        Instruction? current = start;
        while (current is not null)
        {
            Instruction? next = current.Next;
            il.Remove(current);
            if (current == end)
            {
                break;
            }

            current = next;
        }
    }

    private static bool TryFindMockCallSlice(
        Instruction call,
        MethodReference called,
        Dictionary<FieldDefinition, string> mockFields,
        out FieldDefinition? field,
        out List<Instruction>? keyInstructions,
        out Instruction? start)
    {
        field = null;
        keyInstructions = null;
        start = null;
        int valueCount = called.Parameters.Count + 1;
        start = FindProducerStart(call.Previous, valueCount);
        if (start is null)
        {
            return false;
        }

        List<Instruction> slice = [.. InstructionsBetween(start, call.Previous)];
        foreach (Instruction instruction in slice)
        {
            if (instruction.OpCode == OpCodes.Ldfld
                && instruction.Operand is FieldReference fieldReference
                && ResolveField(fieldReference, mockFields.Keys) is { } resolvedField)
            {
                field = resolvedField;
                break;
            }
        }

        if (field is null)
        {
            return false;
        }

        if (called.Parameters.Count == 0)
        {
            throw new InvalidOperationException($"[SpecMock] call '{called.FullName}' must have at least one argument.");
        }

        Instruction? keyStart = FindProducerStart(call.Previous, 1);
        if (keyStart is null || !IsWithinRange(start, call.Previous, keyStart))
        {
            return false;
        }

        keyInstructions = [.. InstructionsBetween(keyStart, call.Previous)];
        return keyInstructions.Count > 0;
    }

    private static Instruction? FindProducerStart(Instruction? end, int valueCount)
    {
        int need = valueCount;
        Instruction? current = end;
        while (current is not null)
        {
            StackDelta(current, out int pops, out int pushes);
            need -= pushes;
            need += pops;
            if (need <= 0)
            {
                return current;
            }

            current = current.Previous;
        }

        return null;
    }

    private static IEnumerable<Instruction> InstructionsBetween(Instruction start, Instruction? end)
    {
        Instruction? current = start;
        while (current is not null)
        {
            yield return current;
            if (current == end)
            {
                yield break;
            }

            current = current.Next;
        }
    }

    private static bool IsWithinRange(Instruction start, Instruction? end, Instruction candidate)
    {
        return InstructionsBetween(start, end).Any(i => i == candidate);
    }

    private static FieldDefinition? ResolveField(FieldReference reference, IEnumerable<FieldDefinition> candidates)
    {
        return candidates.FirstOrDefault(candidate => candidate.FullName == reference.FullName);
    }

    private static bool IsStoreLocal(MethodBody body, Instruction instruction, out VariableDefinition? variable)
    {
        variable = instruction.OpCode.Code switch
        {
            Code.Stloc_0 => body.Variables[0],
            Code.Stloc_1 => body.Variables[1],
            Code.Stloc_2 => body.Variables[2],
            Code.Stloc_3 => body.Variables[3],
            Code.Stloc or Code.Stloc_S => instruction.Operand as VariableDefinition,
            _ => null,
        };
        return variable is not null;
    }

    private static void StackDelta(Instruction instruction, out int pops, out int pushes)
    {
        pops = instruction.OpCode.StackBehaviourPop switch
        {
            StackBehaviour.Pop0 => 0,
            StackBehaviour.Pop1 or StackBehaviour.Popi or StackBehaviour.Popref => 1,
            StackBehaviour.Pop1_pop1 or StackBehaviour.Popi_pop1 or StackBehaviour.Popi_popi or StackBehaviour.Popi_popi8 or StackBehaviour.Popi_popr4 or StackBehaviour.Popi_popr8 or StackBehaviour.Popref_pop1 or StackBehaviour.Popref_popi => 2,
            StackBehaviour.Popi_popi_popi or StackBehaviour.Popref_popi_popi or StackBehaviour.Popref_popi_popi8 or StackBehaviour.Popref_popi_popr4 or StackBehaviour.Popref_popi_popr8 or StackBehaviour.Popref_popi_popref => 3,
            StackBehaviour.Varpop => VarPop(instruction),
            _ => 0,
        };
        pushes = instruction.OpCode.StackBehaviourPush switch
        {
            StackBehaviour.Push0 => 0,
            StackBehaviour.Push1 or StackBehaviour.Pushi or StackBehaviour.Pushi8 or StackBehaviour.Pushr4 or StackBehaviour.Pushr8 or StackBehaviour.Pushref => 1,
            StackBehaviour.Push1_push1 => 2,
            StackBehaviour.Varpush => VarPush(instruction),
            _ => 0,
        };
    }

    private static int VarPop(Instruction instruction)
    {
        if (instruction.Operand is MethodReference method)
        {
            return method.Parameters.Count + (method.HasThis ? 1 : 0);
        }

        return 0;
    }

    private static int VarPush(Instruction instruction)
    {
        if (instruction.Operand is MethodReference method && method.ReturnType.IsVoid())
        {
            return 0;
        }

        return 1;
    }

    private GenericInstanceMethod MakeArrayEmpty(TypeReference type)
    {
        return MakeGenericMethod(_arrayEmptyDefinition ?? throw new InvalidOperationException("Runtime references not initialized."), type);
    }

    private static GenericInstanceMethod MakeGenericMethod(MethodReference definition, TypeReference type)
    {
        var generic = new GenericInstanceMethod(definition);
        generic.GenericArguments.Add(type);
        return generic;
    }

    private static void BoxIfNeeded(List<Instruction> instructions, TypeReference type)
    {
        if (type.IsValueType || type.IsGenericParameter)
        {
            instructions.Add(Instruction.Create(OpCodes.Box, type));
        }
    }

    private static TypeReference InferStackTypeForBox(ModuleDefinition module, Instruction instruction)
    {
        return instruction.OpCode.Code switch
        {
            Code.Ldarg or Code.Ldarg_S => ((ParameterDefinition)instruction.Operand).ParameterType,
            Code.Ldarg_0 => module.TypeSystem.Object,
            Code.Ldarg_1 => module.TypeSystem.Object,
            Code.Ldarg_2 => module.TypeSystem.Object,
            Code.Ldarg_3 => module.TypeSystem.Object,
            Code.Ldloc or Code.Ldloc_S => ((VariableDefinition)instruction.Operand).VariableType,
            Code.Ldc_I4 or Code.Ldc_I4_S or Code.Ldc_I4_0 or Code.Ldc_I4_1 or Code.Ldc_I4_2 or Code.Ldc_I4_3 or Code.Ldc_I4_4 or Code.Ldc_I4_5 or Code.Ldc_I4_6 or Code.Ldc_I4_7 or Code.Ldc_I4_8 or Code.Ldc_I4_M1 => module.TypeSystem.Int32,
            _ => module.TypeSystem.Object,
        };
    }

    private static Instruction LoadInt(int value)
    {
        return value switch
        {
            -1 => Instruction.Create(OpCodes.Ldc_I4_M1),
            0 => Instruction.Create(OpCodes.Ldc_I4_0),
            1 => Instruction.Create(OpCodes.Ldc_I4_1),
            2 => Instruction.Create(OpCodes.Ldc_I4_2),
            3 => Instruction.Create(OpCodes.Ldc_I4_3),
            4 => Instruction.Create(OpCodes.Ldc_I4_4),
            5 => Instruction.Create(OpCodes.Ldc_I4_5),
            6 => Instruction.Create(OpCodes.Ldc_I4_6),
            7 => Instruction.Create(OpCodes.Ldc_I4_7),
            8 => Instruction.Create(OpCodes.Ldc_I4_8),
            >= sbyte.MinValue and <= sbyte.MaxValue => Instruction.Create(OpCodes.Ldc_I4_S, (sbyte)value),
            _ => Instruction.Create(OpCodes.Ldc_I4, value),
        };
    }

    private static Instruction CloneInstruction(Instruction instruction)
    {
        return instruction.Operand switch
        {
            null => Instruction.Create(instruction.OpCode),
            string value => Instruction.Create(instruction.OpCode, value),
            sbyte value => Instruction.Create(instruction.OpCode, value),
            byte value => Instruction.Create(instruction.OpCode, value),
            int value => Instruction.Create(instruction.OpCode, value),
            long value => Instruction.Create(instruction.OpCode, value),
            float value => Instruction.Create(instruction.OpCode, value),
            double value => Instruction.Create(instruction.OpCode, value),
            Instruction target => Instruction.Create(instruction.OpCode, target),
            Instruction[] targets => Instruction.Create(instruction.OpCode, targets),
            VariableDefinition variable => Instruction.Create(instruction.OpCode, variable),
            ParameterDefinition parameter => Instruction.Create(instruction.OpCode, parameter),
            MethodReference method => Instruction.Create(instruction.OpCode, method),
            FieldReference field => Instruction.Create(instruction.OpCode, field),
            TypeReference type => Instruction.Create(instruction.OpCode, type),
            CallSite site => Instruction.Create(instruction.OpCode, site),
            _ => throw new NotSupportedException($"Cannot clone IL operand for {instruction}."),
        };
    }

    private static bool IsEnterOperationCall(Instruction instruction)
    {
        return instruction.Operand is MethodReference method
            && method.Name == "EnterOperation"
            && method.DeclaringType.FullName == "SpecGate.Runtime.SpecGateRuntime";
    }

    private static bool IsEmitMemberCall(Instruction instruction)
    {
        return instruction.Operand is MethodReference method
            && method.Name == "EmitMember"
            && method.DeclaringType.FullName == "SpecGate.Runtime.SpecGateRuntime";
    }
}

internal static class CecilExtensions
{
    /// <summary>
    /// Returns whether the type is <see cref="void"/>.
    /// </summary>
    /// <param name="type">The type reference to inspect.</param>
    /// <returns><see langword="true"/> when the type is <see cref="void"/>.</returns>
    public static bool IsVoid(this TypeReference type) => type.MetadataType == MetadataType.Void;
}
