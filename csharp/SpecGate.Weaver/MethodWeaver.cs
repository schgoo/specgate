using System.Collections.Generic;
using Mono.Cecil;
using Mono.Cecil.Cil;
using Mono.Cecil.Rocks;

namespace SpecGate.Weaver;

/// <summary>
/// Rewrites one annotated method so it opens a capture scope, records its
/// inputs, and reports a terminal status on every exit path.
/// </summary>
/// <remarks>
/// The woven shape is:
/// <code>
/// scope = EnterOperation(operation, component);
/// RecordInput(scope, "a", a);
/// try { ...original body, every `ret` becomes `leave epilogue`... }
/// catch (Exception e) { ExitError(scope, e); throw; }
/// epilogue: ExitOk(scope, result); return result;
/// </code>
/// The catch rethrows, so instrumentation never changes observable behaviour.
/// </remarks>
internal sealed class MethodWeaver(ModuleDefinition module, RuntimeReferences runtime)
{
    internal void Weave(MethodDefinition method, OperationDeclaration declaration)
    {
        MethodBody body = method.Body;
        body.SimplifyMacros();
        body.InitLocals = true;

        ILProcessor il = body.GetILProcessor();
        TypeReference exceptionType = module.ImportReference(runtime.ExitError.Parameters[1].ParameterType);

        VariableDefinition scope = AddVariable(body, runtime.ScopeType);
        VariableDefinition error = AddVariable(body, exceptionType);
        bool returnsValue = method.ReturnType.MetadataType != MetadataType.Void;
        VariableDefinition? result = returnsValue ? AddVariable(body, method.ReturnType) : null;

        Instruction firstOriginal = body.Instructions[0];
        foreach (Instruction instruction in BuildPrologue(il, declaration, scope))
        {
            il.InsertBefore(firstOriginal, instruction);
        }

        List<Instruction> epilogue = BuildEpilogue(il, scope, result);
        Instruction epilogueStart = epilogue[0];
        RedirectReturns(il, body, epilogueStart, result);

        List<Instruction> handler =
        [
            il.Create(OpCodes.Stloc, error),
            il.Create(OpCodes.Ldloc, scope),
            il.Create(OpCodes.Ldloc, error),
            il.Create(OpCodes.Call, runtime.ExitError),
            il.Create(OpCodes.Rethrow),
        ];

        foreach (Instruction instruction in handler)
        {
            il.Append(instruction);
        }

        foreach (Instruction instruction in epilogue)
        {
            il.Append(instruction);
        }

        body.ExceptionHandlers.Add(new ExceptionHandler(ExceptionHandlerType.Catch)
        {
            CatchType = exceptionType,
            TryStart = firstOriginal,
            TryEnd = handler[0],
            HandlerStart = handler[0],
            HandlerEnd = epilogueStart,
        });

        body.OptimizeMacros();
    }

    private List<Instruction> BuildPrologue(
        ILProcessor il,
        OperationDeclaration declaration,
        VariableDefinition scope)
    {
        List<Instruction> prologue = [il.Create(OpCodes.Ldstr, declaration.Operation)];

        if (declaration.Components.Length == 1)
        {
            prologue.Add(il.Create(OpCodes.Ldstr, declaration.Components[0]));
            prologue.Add(il.Create(OpCodes.Call, runtime.EnterOperation));
        }
        else
        {
            prologue.Add(il.Create(OpCodes.Ldc_I4, declaration.Components.Length));
            prologue.Add(il.Create(OpCodes.Newarr, module.TypeSystem.String));
            for (int index = 0; index < declaration.Components.Length; index++)
            {
                prologue.Add(il.Create(OpCodes.Dup));
                prologue.Add(il.Create(OpCodes.Ldc_I4, index));
                prologue.Add(il.Create(OpCodes.Ldstr, declaration.Components[index]));
                prologue.Add(il.Create(OpCodes.Stelem_Ref));
            }

            prologue.Add(il.Create(OpCodes.Call, runtime.EnterOperationMulti));
        }

        prologue.Add(il.Create(OpCodes.Stloc, scope));

        foreach (InputDeclaration input in declaration.Inputs)
        {
            prologue.Add(il.Create(OpCodes.Ldloc, scope));
            prologue.Add(il.Create(OpCodes.Ldstr, input.Name));
            prologue.Add(il.Create(OpCodes.Ldarg, input.Parameter));
            AppendBoxIfNeeded(il, prologue, input.Parameter.ParameterType);
            prologue.Add(il.Create(OpCodes.Call, runtime.RecordInput));
        }

        return prologue;
    }

    private List<Instruction> BuildEpilogue(
        ILProcessor il,
        VariableDefinition scope,
        VariableDefinition? result)
    {
        List<Instruction> epilogue = [il.Create(OpCodes.Ldloc, scope)];
        if (result is null)
        {
            epilogue.Add(il.Create(OpCodes.Call, runtime.ExitVoid));
        }
        else
        {
            epilogue.Add(il.Create(OpCodes.Ldloc, result));
            AppendBoxIfNeeded(il, epilogue, result.VariableType);
            epilogue.Add(il.Create(OpCodes.Call, runtime.ExitOk));
            epilogue.Add(il.Create(OpCodes.Ldloc, result));
        }

        epilogue.Add(il.Create(OpCodes.Ret));
        return epilogue;
    }

    /// <summary>
    /// Turns every <c>ret</c> in the original body into a jump to the epilogue, so
    /// the terminal status is recorded exactly once however the body returns.
    /// </summary>
    /// <remarks>
    /// Each <c>ret</c> is mutated in place rather than replaced. Branch targets and
    /// the boundaries of any exception handler already in the body are instruction
    /// references, so keeping the same instance keeps them valid.
    /// </remarks>
    private static void RedirectReturns(
        ILProcessor il,
        MethodBody body,
        Instruction epilogueStart,
        VariableDefinition? result)
    {
        List<Instruction> returns = [];
        foreach (Instruction instruction in body.Instructions)
        {
            if (instruction.OpCode.Code == Code.Ret)
            {
                returns.Add(instruction);
            }
        }

        foreach (Instruction instruction in returns)
        {
            if (result is null)
            {
                instruction.OpCode = OpCodes.Leave;
                instruction.Operand = epilogueStart;
            }
            else
            {
                instruction.OpCode = OpCodes.Stloc;
                instruction.Operand = result;
                il.InsertAfter(instruction, il.Create(OpCodes.Leave, epilogueStart));
            }
        }
    }

    private void AppendBoxIfNeeded(ILProcessor il, List<Instruction> target, TypeReference type)
    {
        if (RequiresBoxing(type))
        {
            target.Add(il.Create(OpCodes.Box, module.ImportReference(type)));
        }
    }

    private static bool RequiresBoxing(TypeReference type)
    {
        if (type.IsGenericParameter)
        {
            return true;
        }

        if (type.IsValueType)
        {
            return true;
        }

        TypeDefinition? definition = type.Resolve();
        return definition is not null && definition.IsValueType;
    }

    private static VariableDefinition AddVariable(MethodBody body, TypeReference type)
    {
        VariableDefinition variable = new(type);
        body.Variables.Add(variable);
        return variable;
    }
}
